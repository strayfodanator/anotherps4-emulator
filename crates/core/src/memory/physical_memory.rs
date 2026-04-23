//! Physical memory (Direct Memory / Flexible Memory) allocation.
//!
//! Tracks allocation of physical memory regions that can be mapped
//! into the virtual address space. Mirrors the PS4's DMEM/FMEM model.

use anotherps4_common::*;
use std::collections::BTreeMap;

/// State of a physical memory area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalMemoryType {
    Free,
    Allocated,
    Mapped,
    Pooled,
    Committed,
    Flexible,
}

/// A contiguous region of physical memory.
#[derive(Debug, Clone)]
pub struct PhysicalMemoryArea {
    pub base: PAddr,
    pub size: u64,
    pub memory_type: i32,
    pub dma_type: PhysicalMemoryType,
}

impl PhysicalMemoryArea {
    pub fn end(&self) -> PAddr {
        self.base + self.size
    }

    pub fn can_merge_with(&self, next: &PhysicalMemoryArea) -> bool {
        self.base + self.size == next.base
            && self.memory_type == next.memory_type
            && self.dma_type == next.dma_type
    }
}

/// Manages physical memory allocation (Direct Memory).
pub struct PhysicalMemoryManager {
    /// Map of physical address → area.
    areas: BTreeMap<PAddr, PhysicalMemoryArea>,
    /// Total direct memory size.
    total_size: u64,
}

impl PhysicalMemoryManager {
    /// Create a new physical memory manager with the given total size.
    pub fn new(total_size: u64) -> Self {
        let mut areas = BTreeMap::new();
        // Start with one big free area
        areas.insert(
            0,
            PhysicalMemoryArea {
                base: 0,
                size: total_size,
                memory_type: 0,
                dma_type: PhysicalMemoryType::Free,
            },
        );

        tracing::info!(
            total = format_size(total_size),
            "Physical memory manager initialized"
        );

        PhysicalMemoryManager { areas, total_size }
    }

    /// Total physical memory size.
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Allocate a region of physical memory.
    pub fn allocate(
        &mut self,
        search_start: PAddr,
        search_end: PAddr,
        size: u64,
        alignment: u64,
    ) -> Option<PAddr> {
        let alignment = alignment.max(PAGE_SIZE);

        // Find a free area that fits
        for area in self.areas.values() {
            if area.dma_type != PhysicalMemoryType::Free {
                continue;
            }

            let aligned_base = align_up(area.base.max(search_start), alignment);
            let end = aligned_base + size;

            if aligned_base >= area.base && end <= area.end() && end <= search_end {
                let addr = aligned_base;

                // Split the free area around the allocation
                self.carve_area(addr, size);

                // Mark as allocated
                if let Some(area) = self.areas.get_mut(&addr) {
                    area.dma_type = PhysicalMemoryType::Allocated;
                }

                tracing::debug!(
                    addr = format!("0x{:X}", addr),
                    size = format_size(size),
                    "Physical memory allocated"
                );

                return Some(addr);
            }
        }

        tracing::warn!(
            size = format_size(size),
            "Physical memory allocation failed — no suitable free region"
        );
        None
    }

    /// Free a previously allocated region.
    pub fn free(&mut self, addr: PAddr, size: u64) -> bool {
        if let Some(area) = self.areas.get_mut(&addr) {
            if area.size == size {
                area.dma_type = PhysicalMemoryType::Free;
                self.merge_adjacent(addr);
                return true;
            }
        }
        false
    }

    /// Carve a region out of existing areas at the given address and size.
    fn carve_area(&mut self, addr: PAddr, size: u64) {
        // Find the area containing this address
        let containing_addr = {
            let mut found = None;
            for (&base, area) in &self.areas {
                if base <= addr && addr < area.end() {
                    found = Some(base);
                    break;
                }
            }
            match found {
                Some(a) => a,
                None => return,
            }
        };

        let area = self.areas.remove(&containing_addr).unwrap();

        // Area before the carved region
        if addr > area.base {
            self.areas.insert(
                area.base,
                PhysicalMemoryArea {
                    base: area.base,
                    size: addr - area.base,
                    memory_type: area.memory_type,
                    dma_type: area.dma_type,
                },
            );
        }

        // The carved region itself
        self.areas.insert(
            addr,
            PhysicalMemoryArea {
                base: addr,
                size,
                memory_type: area.memory_type,
                dma_type: area.dma_type,
            },
        );

        // Area after the carved region
        let end = addr + size;
        if end < area.end() {
            self.areas.insert(
                end,
                PhysicalMemoryArea {
                    base: end,
                    size: area.end() - end,
                    memory_type: area.memory_type,
                    dma_type: area.dma_type,
                },
            );
        }
    }

    /// Merge adjacent free areas around the given address.
    fn merge_adjacent(&mut self, _addr: PAddr) {
        // Collect all areas sorted by address
        let keys: Vec<PAddr> = self.areas.keys().cloned().collect();

        for i in 0..keys.len().saturating_sub(1) {
            let current_key = keys[i];
            let next_key = keys[i + 1];

            let can_merge = {
                let current = &self.areas[&current_key];
                let next = &self.areas[&next_key];
                current.can_merge_with(next)
            };

            if can_merge {
                let next = self.areas.remove(&next_key).unwrap();
                let current = self.areas.get_mut(&current_key).unwrap();
                current.size += next.size;
            }
        }
    }
}
