//! Virtual address space management using OS-level memory mapping.
//!
//! Reserves a large contiguous virtual address range using `mmap` and
//! manages sub-regions for the PS4's system managed, system reserved,
//! and user address spaces.

use anotherps4_common::*;
use std::ptr;

/// Manages the host-side virtual address space backing for PS4 memory.
pub struct AddressSpace {
    /// Base pointer of the backing memory (direct memory file mapping).
    backing_base: *mut u8,
    /// Size of backing memory.
    backing_size: u64,

    /// Base of system-managed region.
    system_managed_base: *mut u8,
    /// Size of system-managed region.
    system_managed_size: u64,

    /// Base of system-reserved region.
    system_reserved_base: *mut u8,
    /// Size of system-reserved region.
    system_reserved_size: u64,

    /// Base of user region.
    user_base: *mut u8,
    /// Size of user region.
    user_size: u64,
}

// SAFETY: The raw pointers in AddressSpace are to memory-mapped regions
// that persist for the lifetime of the struct. Access is synchronized
// through the MemoryManager's mutexes.
unsafe impl Send for AddressSpace {}
unsafe impl Sync for AddressSpace {}

/// Default backing memory size (8 GB, matching PS4's unified memory).
const BACKING_SIZE: u64 = 8 * 1024 * 1024 * 1024;

impl AddressSpace {
    /// Create a new address space with the PS4's standard layout.
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("Initializing PS4 address space");

        // Allocate the backing memory (represents physical GDDR5)
        let backing_base = Self::mmap_anonymous(BACKING_SIZE)?;
        tracing::info!(
            base = format!("0x{:X}", backing_base as u64),
            size = format_size(BACKING_SIZE),
            "Backing memory allocated"
        );

        // Reserve the user virtual address space
        let user_size = USER_SIZE;
        let user_base = Self::mmap_reserve(user_size)?;
        tracing::info!(
            base = format!("0x{:X}", user_base as u64),
            size = format_size(user_size),
            "User address space reserved"
        );

        // System managed and reserved share the same mapping for now
        let system_managed_size = SYSTEM_MANAGED_SIZE;
        let system_managed_base = Self::mmap_reserve(system_managed_size)?;

        let system_reserved_size = SYSTEM_RESERVED_SIZE;
        let system_reserved_base = Self::mmap_reserve(system_reserved_size)?;

        Ok(AddressSpace {
            backing_base,
            backing_size: BACKING_SIZE,
            system_managed_base,
            system_managed_size,
            system_reserved_base,
            system_reserved_size,
            user_base,
            user_size,
        })
    }

    /// Get the backing memory base pointer.
    pub fn backing_base(&self) -> *mut u8 {
        self.backing_base
    }

    /// Get the user virtual address space base.
    pub fn user_base(&self) -> VAddr {
        self.user_base as VAddr
    }

    /// Get the user virtual address space size.
    pub fn user_size(&self) -> u64 {
        self.user_size
    }

    /// Get the system-managed base.
    pub fn system_managed_base(&self) -> VAddr {
        self.system_managed_base as VAddr
    }

    /// Get the system-reserved base.
    pub fn system_reserved_base(&self) -> VAddr {
        self.system_reserved_base as VAddr
    }

    /// Map backing memory into a virtual address range.
    ///
    /// This creates a mapping from the virtual address to a region of the
    /// backing file, allowing the same physical memory to be aliased at
    /// multiple virtual addresses (as the PS4 GPU does).
    pub fn map(
        &self,
        virtual_addr: VAddr,
        size: u64,
        phys_offset: u64,
        executable: bool,
    ) -> anyhow::Result<*mut u8> {
        let prot = if executable {
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC
        } else {
            libc::PROT_READ | libc::PROT_WRITE
        };

        let result = unsafe {
            libc::mmap(
                virtual_addr as *mut libc::c_void,
                size as usize,
                prot,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                -1,
                0,
            )
        };

        if result == libc::MAP_FAILED {
            anyhow::bail!(
                "mmap failed for addr=0x{:X} size=0x{:X}: {}",
                virtual_addr,
                size,
                std::io::Error::last_os_error()
            );
        }

        tracing::trace!(
            addr = format!("0x{:X}", virtual_addr),
            size = format_size(size),
            phys = format!("0x{:X}", phys_offset),
            exec = executable,
            "Memory mapped"
        );

        Ok(result as *mut u8)
    }

    /// Unmap a virtual address range.
    pub fn unmap(&self, virtual_addr: VAddr, size: u64) -> anyhow::Result<()> {
        let result = unsafe { libc::munmap(virtual_addr as *mut libc::c_void, size as usize) };

        if result != 0 {
            anyhow::bail!(
                "munmap failed for addr=0x{:X} size=0x{:X}: {}",
                virtual_addr,
                size,
                std::io::Error::last_os_error()
            );
        }

        Ok(())
    }

    /// Change memory protection on a range.
    pub fn protect(&self, virtual_addr: VAddr, size: u64, read: bool, write: bool, exec: bool) -> anyhow::Result<()> {
        let mut prot = libc::PROT_NONE;
        if read {
            prot |= libc::PROT_READ;
        }
        if write {
            prot |= libc::PROT_WRITE;
        }
        if exec {
            prot |= libc::PROT_EXEC;
        }

        let result =
            unsafe { libc::mprotect(virtual_addr as *mut libc::c_void, size as usize, prot) };

        if result != 0 {
            anyhow::bail!(
                "mprotect failed for addr=0x{:X} size=0x{:X}: {}",
                virtual_addr,
                size,
                std::io::Error::last_os_error()
            );
        }

        Ok(())
    }

    /// Reserve a virtual address range without committing memory.
    fn mmap_reserve(size: u64) -> anyhow::Result<*mut u8> {
        let result = unsafe {
            libc::mmap(
                0x10_0000_0000 as *mut libc::c_void, // Force allocation in low memory (without 0x7F prefix) for pointer tagging compatibility
                size as usize,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if result == libc::MAP_FAILED {
            anyhow::bail!(
                "Failed to reserve {} of virtual memory: {}",
                format_size(size),
                std::io::Error::last_os_error()
            );
        }

        Ok(result as *mut u8)
    }

    /// Allocate anonymous read/write memory.
    fn mmap_anonymous(size: u64) -> anyhow::Result<*mut u8> {
        let result = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if result == libc::MAP_FAILED {
            anyhow::bail!(
                "Failed to allocate {} of memory: {}",
                format_size(size),
                std::io::Error::last_os_error()
            );
        }

        Ok(result as *mut u8)
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        unsafe {
            if !self.backing_base.is_null() {
                libc::munmap(
                    self.backing_base as *mut libc::c_void,
                    self.backing_size as usize,
                );
            }
            if !self.user_base.is_null() {
                libc::munmap(
                    self.user_base as *mut libc::c_void,
                    self.user_size as usize,
                );
            }
            if !self.system_managed_base.is_null() {
                libc::munmap(
                    self.system_managed_base as *mut libc::c_void,
                    self.system_managed_size as usize,
                );
            }
            if !self.system_reserved_base.is_null() {
                libc::munmap(
                    self.system_reserved_base as *mut libc::c_void,
                    self.system_reserved_size as usize,
                );
            }
        }
    }
}
