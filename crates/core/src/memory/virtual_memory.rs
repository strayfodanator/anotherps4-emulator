//! Virtual Memory Area (VMA) management.
//!
//! Tracks virtual address space allocations, mapping state, and memory
//! protection. This is the main interface used by kernel syscalls for
//! memory operations (mmap, munmap, mprotect, etc.).

use anotherps4_common::*;
use parking_lot::RwLock;
use std::collections::BTreeMap;

use super::address_space::AddressSpace;
use super::physical_memory::PhysicalMemoryManager;

/// Memory protection flags (CPU-side).
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MemoryProt: u32 {
        const NONE      = 0;
        const CPU_READ  = 1;
        const CPU_WRITE = 2;
        const CPU_RW    = 3;
        const CPU_EXEC  = 4;
        const GPU_READ  = 16;
        const GPU_WRITE = 32;
        const GPU_RW    = 48;
    }
}

/// Flags for memory mapping operations.
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MemoryMapFlags: u32 {
        const NONE          = 0;
        const SHARED        = 1;
        const PRIVATE       = 2;
        const FIXED         = 0x10;
        const NO_OVERWRITE  = 0x80;
        const VOID          = 0x100;
        const STACK         = 0x400;
        const NO_SYNC       = 0x800;
        const ANONYMOUS     = 0x1000;
        const NO_CORE       = 0x20000;
        const NO_COALESCE   = 0x400000;
    }
}

/// Type of a virtual memory area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMAType {
    /// Unmapped/available region.
    Free,
    /// Reserved but not yet committed.
    Reserved,
    /// Backed by direct physical memory.
    Direct,
    /// Backed by flexible memory.
    Flexible,
    /// Pool memory.
    Pooled,
    /// Reserved for pool.
    PoolReserved,
    /// Thread stack.
    Stack,
    /// Thread Control Block and general thread data.
    ThreadData,
    /// Executable code.
    Code,
    /// Memory-mapped file.
    File,
}

/// A contiguous region of virtual memory with uniform properties.
#[derive(Debug, Clone)]
pub struct VirtualMemoryArea {
    /// Base virtual address.
    pub base: VAddr,
    /// Size in bytes.
    pub size: u64,
    /// Type of mapping.
    pub vma_type: VMAType,
    /// Memory protection flags.
    pub prot: MemoryProt,
    /// Human-readable name for debugging.
    pub name: String,
    /// Whether this VMA should not be merged with adjacent VMAs.
    pub disallow_merge: bool,
}

impl VirtualMemoryArea {
    /// Check if this VMA contains the given address range.
    pub fn contains(&self, addr: VAddr, size: u64) -> bool {
        addr >= self.base && (addr + size) <= (self.base + self.size)
    }

    /// Check if this VMA overlaps with the given address range.
    pub fn overlaps(&self, addr: VAddr, size: u64) -> bool {
        addr < (self.base + self.size) && (addr + size) > self.base
    }

    /// Check if this VMA is free.
    pub fn is_free(&self) -> bool {
        self.vma_type == VMAType::Free
    }

    /// Check if this VMA is actively mapped.
    pub fn is_mapped(&self) -> bool {
        !matches!(
            self.vma_type,
            VMAType::Free | VMAType::Reserved | VMAType::PoolReserved
        )
    }

    /// Check if this VMA can be merged with the next one.
    pub fn can_merge_with(&self, next: &VirtualMemoryArea) -> bool {
        if self.disallow_merge || next.disallow_merge {
            return false;
        }
        if self.base + self.size != next.base {
            return false;
        }
        if self.prot != next.prot || self.vma_type != next.vma_type {
            return false;
        }
        if self.name != next.name {
            return false;
        }
        true
    }
}

/// The main memory manager for the emulator.
///
/// Coordinates virtual address space management, physical memory allocation,
/// and provides the interface used by kernel HLE syscalls.
pub struct MemoryManager {
    /// The underlying address space (mmap-based).
    address_space: AddressSpace,
    /// Physical (direct) memory manager.
    physical: PhysicalMemoryManager,
    /// Virtual memory area map: base address → VMA.
    vma_map: RwLock<BTreeMap<VAddr, VirtualMemoryArea>>,
    /// Total direct memory size.
    total_direct_size: u64,
    /// Total flexible memory size.
    total_flexible_size: u64,
    /// Current flexible memory usage.
    flexible_usage: u64,
}

impl MemoryManager {
    /// Create a new memory manager.
    pub fn new() -> anyhow::Result<Self> {
        let address_space = AddressSpace::new()?;

        // PS4 has ~5.5 GB usable direct memory (out of 8 GB total, rest is system)
        let total_direct_size = 5632 * 1024 * 1024; // 5.5 GB
        let total_flexible_size = 448 * 1024 * 1024; // 448 MB

        let physical = PhysicalMemoryManager::new(total_direct_size);

        // Initialize VMA map with one large free region covering user space
        let user_base = address_space.user_base();
        let user_size = address_space.user_size();

        let mut vma_map = BTreeMap::new();
        vma_map.insert(
            user_base,
            VirtualMemoryArea {
                base: user_base,
                size: user_size,
                vma_type: VMAType::Free,
                prot: MemoryProt::NONE,
                name: String::new(),
                disallow_merge: false,
            },
        );

        tracing::info!(
            direct = format_size(total_direct_size),
            flexible = format_size(total_flexible_size),
            user_base = format!("0x{:X}", user_base),
            user_size = format_size(user_size),
            "Memory manager initialized"
        );

        Ok(MemoryManager {
            address_space,
            physical,
            vma_map: RwLock::new(vma_map),
            total_direct_size,
            total_flexible_size,
            flexible_usage: 0,
        })
    }

    /// Get a reference to the address space.
    pub fn address_space(&self) -> &AddressSpace {
        &self.address_space
    }

    /// Get a cloned list of all active Virtual Memory Areas.
    pub fn get_vmas(&self) -> Vec<VirtualMemoryArea> {
        self.vma_map.read().values().cloned().collect()
    }

    /// Get total direct memory size.
    pub fn total_direct_size(&self) -> u64 {
        self.total_direct_size
    }

    /// Get total flexible memory size.
    pub fn total_flexible_size(&self) -> u64 {
        self.total_flexible_size
    }

    /// Get available flexible memory.
    pub fn available_flexible_size(&self) -> u64 {
        self.total_flexible_size - self.flexible_usage
    }

    /// Allocate direct (physical) memory.
    pub fn allocate_direct_memory(
        &mut self,
        search_start: PAddr,
        search_end: PAddr,
        size: u64,
        alignment: u64,
        _memory_type: i32,
    ) -> Option<PAddr> {
        let aligned_size = align_up(size, PAGE_SIZE);
        self.physical
            .allocate(search_start, search_end, aligned_size, alignment)
    }

    /// Map memory into the virtual address space.
    pub fn map_memory(
        &mut self,
        virtual_addr: VAddr,
        size: u64,
        prot: MemoryProt,
        flags: MemoryMapFlags,
        vma_type: VMAType,
        name: &str,
    ) -> anyhow::Result<VAddr> {
        let aligned_size = align_up(size, PAGE_SIZE);
        let addr = if virtual_addr == 0 || !flags.contains(MemoryMapFlags::FIXED) {
            self.find_free_region(aligned_size, PAGE_SIZE)?
        } else {
            virtual_addr
        };

        // Perform the actual mmap
        let executable = prot.contains(MemoryProt::CPU_EXEC);
        self.address_space.map(addr, aligned_size, 0, executable)?;

        // Update the VMA map
        {
            let mut map = self.vma_map.write();
            self.carve_vma(&mut map, addr, aligned_size);

            map.insert(
                addr,
                VirtualMemoryArea {
                    base: addr,
                    size: aligned_size,
                    vma_type,
                    prot,
                    name: name.to_string(),
                    disallow_merge: flags.contains(MemoryMapFlags::NO_COALESCE),
                },
            );
        }

        tracing::debug!(
            addr = format!("0x{:X}", addr),
            size = format_size(aligned_size),
            name,
            vma_type = ?vma_type,
            prot = ?prot,
            "Memory mapped"
        );

        Ok(addr)
    }

    /// Unmap memory from the virtual address space.
    pub fn unmap_memory(&mut self, virtual_addr: VAddr, size: u64) -> anyhow::Result<()> {
        let aligned_size = align_up(size, PAGE_SIZE);

        self.address_space.unmap(virtual_addr, aligned_size)?;

        let mut map = self.vma_map.write();
        self.carve_vma(&mut map, virtual_addr, aligned_size);

        map.insert(
            virtual_addr,
            VirtualMemoryArea {
                base: virtual_addr,
                size: aligned_size,
                vma_type: VMAType::Free,
                prot: MemoryProt::NONE,
                name: String::new(),
                disallow_merge: false,
            },
        );

        Ok(())
    }

    /// Change memory protection on a range.
    pub fn protect_memory(
        &self,
        addr: VAddr,
        size: u64,
        prot: MemoryProt,
    ) -> anyhow::Result<()> {
        let read = prot.contains(MemoryProt::CPU_READ);
        let write = prot.contains(MemoryProt::CPU_WRITE);
        let exec = prot.contains(MemoryProt::CPU_EXEC);
        self.address_space.protect(addr, size, read, write, exec)
    }

    /// Check if a GPU mapping is valid (within 40-bit address space).
    pub fn is_valid_gpu_mapping(&self, virtual_addr: VAddr, size: u64) -> bool {
        virtual_addr + size < MAX_GPU_ADDRESS
    }

    /// Find a free region of the given size in the VMA map.
    fn find_free_region(&self, size: u64, alignment: u64) -> anyhow::Result<VAddr> {
        let map = self.vma_map.read();

        for vma in map.values() {
            if !vma.is_free() {
                continue;
            }

            let aligned_base = align_up(vma.base, alignment);
            if aligned_base + size <= vma.base + vma.size {
                return Ok(aligned_base);
            }
        }

        anyhow::bail!(
            "No free virtual memory region of size {} found",
            format_size(size)
        );
    }

    /// Carve a region out of existing VMAs.
    fn carve_vma(
        &self,
        map: &mut BTreeMap<VAddr, VirtualMemoryArea>,
        addr: VAddr,
        size: u64,
    ) {
        let end = addr + size;

        // Find all VMAs that overlap
        let overlapping: Vec<VAddr> = map
            .range(..end)
            .filter(|(_, vma)| vma.overlaps(addr, size))
            .map(|(&base, _)| base)
            .collect();

        for base in overlapping {
            let vma = map.remove(&base).unwrap();

            // Part before the carve
            if vma.base < addr {
                map.insert(
                    vma.base,
                    VirtualMemoryArea {
                        base: vma.base,
                        size: addr - vma.base,
                        ..vma.clone()
                    },
                );
            }

            // Part after the carve
            let vma_end = vma.base + vma.size;
            if vma_end > end {
                map.insert(
                    end,
                    VirtualMemoryArea {
                        base: end,
                        size: vma_end - end,
                        ..vma
                    },
                );
            }
        }
    }

    /// Dump the current VMA map for debugging.
    pub fn dump_vma_map(&self) {
        let map = self.vma_map.read();
        tracing::info!("=== Virtual Memory Areas ({} entries) ===", map.len());
        for vma in map.values() {
            if !vma.is_free() {
                tracing::info!(
                    "  0x{:012X} - 0x{:012X}  {}  {:?}  {:?}  \"{}\"",
                    vma.base,
                    vma.base + vma.size,
                    format_size(vma.size),
                    vma.vma_type,
                    vma.prot,
                    vma.name
                );
            }
        }
    }
}
