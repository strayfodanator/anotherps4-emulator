//! Kernel memory syscall implementations.
//!
//! Implements the PS4 kernel functions for memory allocation and mapping
//! that games call through libkernel.

use crate::memory::{MemoryManager, MemoryMapFlags, MemoryProt, VMAType};
use anotherps4_common::*;
use super::OrbisError;

/// `sceKernelAllocateDirectMemory` — Allocate physical (direct) memory.
///
/// Finds a free region in the direct memory pool and marks it as allocated.
/// This does NOT map it into the virtual address space; use `sceKernelMapDirectMemory` for that.
pub fn sce_kernel_allocate_direct_memory(
    memory: &mut MemoryManager,
    search_start: PAddr,
    search_end: PAddr,
    len: u64,
    alignment: u64,
    memory_type: i32,
    phys_addr_out: &mut PAddr,
) -> i32 {
    tracing::debug!(
        search_start = format!("0x{:X}", search_start),
        search_end = format!("0x{:X}", search_end),
        len = format_size(len),
        alignment = format!("0x{:X}", alignment),
        memory_type,
        "sceKernelAllocateDirectMemory"
    );

    if len == 0 || !is_aligned(len, PAGE_SIZE) {
        tracing::error!("Invalid length: 0x{:X}", len);
        return OrbisError::EINVAL.into();
    }

    match memory.allocate_direct_memory(search_start, search_end, len, alignment, memory_type) {
        Some(addr) => {
            *phys_addr_out = addr;
            tracing::debug!(addr = format!("0x{:X}", addr), "Direct memory allocated");
            OrbisError::OK.into()
        }
        None => {
            tracing::error!("Failed to allocate {} of direct memory", format_size(len));
            OrbisError::ENOMEM.into()
        }
    }
}

/// `sceKernelMapDirectMemory` — Map physical memory into the virtual address space.
pub fn sce_kernel_map_direct_memory(
    memory: &mut MemoryManager,
    addr: &mut VAddr,
    len: u64,
    prot: u32,
    flags: u32,
    _direct_memory_start: PAddr,
    _alignment: u64,
) -> i32 {
    let mem_prot = MemoryProt::from_bits_truncate(prot);
    let mem_flags = MemoryMapFlags::from_bits_truncate(flags);

    tracing::debug!(
        addr = format!("0x{:X}", *addr),
        len = format_size(len),
        prot = ?mem_prot,
        flags = ?mem_flags,
        "sceKernelMapDirectMemory"
    );

    match memory.map_memory(*addr, len, mem_prot, mem_flags, VMAType::Direct, "direct") {
        Ok(mapped_addr) => {
            *addr = mapped_addr;
            OrbisError::OK.into()
        }
        Err(e) => {
            tracing::error!("Map failed: {}", e);
            OrbisError::ENOMEM.into()
        }
    }
}

/// `sceKernelMapFlexibleMemory` — Map flexible memory.
pub fn sce_kernel_map_flexible_memory(
    memory: &mut MemoryManager,
    addr: &mut VAddr,
    len: u64,
    prot: u32,
    flags: u32,
) -> i32 {
    let mem_prot = MemoryProt::from_bits_truncate(prot);
    let mem_flags = MemoryMapFlags::from_bits_truncate(flags);

    tracing::debug!(
        addr = format!("0x{:X}", *addr),
        len = format_size(len),
        prot = ?mem_prot,
        "sceKernelMapFlexibleMemory"
    );

    match memory.map_memory(*addr, len, mem_prot, mem_flags, VMAType::Flexible, "flexible") {
        Ok(mapped_addr) => {
            *addr = mapped_addr;
            OrbisError::OK.into()
        }
        Err(e) => {
            tracing::error!("Flexible map failed: {}", e);
            OrbisError::ENOMEM.into()
        }
    }
}

/// `sceKernelMunmap` — Unmap virtual memory.
pub fn sce_kernel_munmap(memory: &mut MemoryManager, addr: VAddr, len: u64) -> i32 {
    tracing::debug!(
        addr = format!("0x{:X}", addr),
        len = format_size(len),
        "sceKernelMunmap"
    );

    match memory.unmap_memory(addr, len) {
        Ok(()) => OrbisError::OK.into(),
        Err(e) => {
            tracing::error!("Unmap failed: {}", e);
            OrbisError::EINVAL.into()
        }
    }
}

/// `sceKernelQueryMemoryProtection` — Query memory protection of a range.
pub fn sce_kernel_query_memory_protection(
    _memory: &MemoryManager,
    addr: VAddr,
    start_out: &mut VAddr,
    end_out: &mut VAddr,
    prot_out: &mut u32,
) -> i32 {
    tracing::debug!(
        addr = format!("0x{:X}", addr),
        "sceKernelQueryMemoryProtection (stub)"
    );

    // TODO: implement proper query
    *start_out = align_down(addr, PAGE_SIZE);
    *end_out = *start_out + PAGE_SIZE;
    *prot_out = (MemoryProt::CPU_READ | MemoryProt::CPU_WRITE).bits();

    OrbisError::OK.into()
}
