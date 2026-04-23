//! Stub generator for unresolved PS4 functions.
//!
//! Allocates executable memory containing tiny trampoline functions
//! that simply return 0 (ORBIS_OK). Each stub gets a unique address
//! so we can identify which function was called in debug logs.

use std::sync::Mutex;

/// Size of each stub trampoline in bytes.
/// x86_64: `xor eax, eax; ret` = 3 bytes, padded to 16 for alignment.
const STUB_SIZE: usize = 16;

/// Maximum number of stubs.
const MAX_STUBS: usize = 4096;

/// Total memory for stub region.
const STUB_REGION_SIZE: usize = STUB_SIZE * MAX_STUBS;

/// Global stub allocator.
static STUB_ALLOC: Mutex<Option<StubAllocator>> = Mutex::new(None);

struct StubAllocator {
    base: *mut u8,
    used: usize,
    names: Vec<String>,
}

unsafe impl Send for StubAllocator {}

impl StubAllocator {
    /// Allocate a new executable memory region for stubs.
    fn new() -> anyhow::Result<Self> {
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                STUB_REGION_SIZE,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if base == libc::MAP_FAILED {
            anyhow::bail!("Failed to allocate stub memory");
        }

        let base = base as *mut u8;

        // Pre-fill all slots with `ud2` so we can intercept the stub call
        // 0F 0B = ud2
        unsafe {
            for i in 0..MAX_STUBS {
                let slot = base.add(i * STUB_SIZE);
                // ud2
                *slot = 0x0F;
                *slot.add(1) = 0x0B;
                // Fill rest with NOPs
                for j in 2..STUB_SIZE {
                    *slot.add(j) = 0x90;
                }
            }
        }

        tracing::info!(
            base = format!("0x{:X}", base as u64),
            slots = MAX_STUBS,
            "Stub trampoline region allocated"
        );

        Ok(Self { base, used: 0, names: Vec::new() })
    }

    /// Get the next available stub address.
    fn next_stub(&mut self, name: String) -> Option<u64> {
        if self.used >= MAX_STUBS {
            return None;
        }
        let addr = unsafe { self.base.add(self.used * STUB_SIZE) } as u64;
        self.used += 1;
        self.names.push(name);
        Some(addr)
    }
}

/// Initialize the global stub allocator. Must be called once at startup.
pub fn init() -> anyhow::Result<()> {
    let alloc = StubAllocator::new()?;
    *STUB_ALLOC.lock().unwrap() = Some(alloc);
    Ok(())
}

/// Allocate a new unique stub address that returns 0 when called.
pub fn allocate_stub(name: String) -> u64 {
    let mut guard = STUB_ALLOC.lock().unwrap();
    if let Some(ref mut alloc) = *guard {
        alloc.next_stub(name).unwrap_or(0)
    } else {
        0
    }
}

/// Get the name of a stub by index.
pub fn get_stub_name(idx: usize) -> String {
    let guard = STUB_ALLOC.lock().unwrap();
    if let Some(ref alloc) = *guard {
        alloc.names.get(idx).cloned().unwrap_or_else(|| "UnknownStub".to_string())
    } else {
        "UnknownStub".to_string()
    }
}

/// Get the number of stubs allocated so far.
pub fn stub_count() -> usize {
    let guard = STUB_ALLOC.lock().unwrap();
    if let Some(ref alloc) = *guard {
        alloc.used
    } else {
        0
    }
}

/// Check if an address corresponds to an unresolved stub
pub fn is_stub(addr: u64) -> bool {
    let guard = STUB_ALLOC.lock().unwrap();
    if let Some(ref alloc) = *guard {
        let base = alloc.base as u64;
        addr >= base && addr < base + (alloc.used * STUB_SIZE) as u64
    } else {
        false
    }
}

/// Get the index of the stub at the given address
pub fn get_stub_index(addr: u64) -> Option<usize> {
    let guard = STUB_ALLOC.lock().unwrap();
    if let Some(ref alloc) = *guard {
        let base = alloc.base as u64;
        if addr >= base && addr < base + (alloc.used * STUB_SIZE) as u64 {
            Some(((addr - base) as usize) / STUB_SIZE)
        } else {
            None
        }
    } else {
        None
    }
}
