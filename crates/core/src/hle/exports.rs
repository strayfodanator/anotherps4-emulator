//! Native interception and dispatching for HLE exports.
//!
//! Unlike syscalls, exported functions are identified by their symbol name
//! (or translated NID alias) and are dispatched here directly upon UD2 stub execution.

use crate::memory::MemoryManager;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// Global memory manager reference for allocators.
static MEMORY_MANAGER: Mutex<Option<Arc<Mutex<MemoryManager>>>> = Mutex::new(None);

/// Next event queue ID counter.
static NEXT_EQUEUE_ID: AtomicI32 = AtomicI32::new(1);

/// Next fake FILE* address counter (allocated from a unique range).
static NEXT_FILE_PTR: AtomicU64 = AtomicU64::new(0xBEEF_0000_0000);

/// Open FILE* tracking: maps fake FILE* address → host File + metadata.
struct OpenCFile {
    file: std::fs::File,
    path: String,
}

static OPEN_CFILES: Mutex<Option<HashMap<u64, OpenCFile>>> = Mutex::new(None);

/// Filesystem mount points: PS4 prefix → host path.
static FS_MOUNTS: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

/// Set the memory manager instance for use by exports.
pub fn initialize_exports(memory: Arc<Mutex<MemoryManager>>) {
    *MEMORY_MANAGER.lock() = Some(memory);
    *OPEN_CFILES.lock() = Some(HashMap::new());
    *FS_MOUNTS.lock() = Some(HashMap::new());
    *PHYS_MAPPINGS.lock() = Some(Vec::new());
}

/// Physical-to-virtual address mapping entry.
struct PhysMappingEntry {
    phys_start: u64,
    virt_start: u64,
    size: u64,
}

/// Tracks physical→virtual address mappings created by sceKernelMapDirectMemory.
static PHYS_MAPPINGS: Mutex<Option<Vec<PhysMappingEntry>>> = Mutex::new(None);

/// Register a phys→virt mapping.
fn register_phys_mapping(phys: u64, virt_addr: u64, size: u64) {
    let mut guard = PHYS_MAPPINGS.lock();
    if let Some(ref mut mappings) = *guard {
        mappings.push(PhysMappingEntry {
            phys_start: phys,
            virt_start: virt_addr,
            size,
        });
        tracing::debug!(
            phys = format!("0x{:X}", phys),
            virt = format!("0x{:X}", virt_addr),
            size = format!("0x{:X}", size),
            "Registered phys->virt mapping"
        );
    }
}

/// Translate a physical address to virtual using registered mappings.
#[allow(dead_code)]
fn phys_to_virt(phys: u64) -> Option<u64> {
    let guard = PHYS_MAPPINGS.lock();
    if let Some(ref mappings) = *guard {
        for entry in mappings {
            if phys >= entry.phys_start && phys < entry.phys_start + entry.size {
                return Some(entry.virt_start + (phys - entry.phys_start));
            }
        }
    }
    None
}

// mount a ps4 virtual path to a host directory
pub fn mount_filesystem(ps4_prefix: &str, host_path: &std::path::Path) {
    let mut guard = FS_MOUNTS.lock();
    if let Some(ref mut mounts) = *guard {
        tracing::info!(
            ps4 = ps4_prefix,
            host = %host_path.display(),
            "Mounted filesystem"
        );
        mounts.insert(ps4_prefix.to_string(), host_path.to_path_buf());
    }
}

/// Resolve a PS4 path (e.g. `/app0/clean_vv.sb`) to a host path.
fn resolve_ps4_path(ps4_path: &str) -> Option<PathBuf> {
    let guard = FS_MOUNTS.lock();
    if let Some(ref mounts) = *guard {
        for (prefix, host_base) in mounts {
            if ps4_path.starts_with(prefix.as_str()) {
                let relative = &ps4_path[prefix.len()..];
                let relative = relative.trim_start_matches('/');
                let resolved = host_base.join(relative);
                return Some(resolved);
            }
        }
    }
    None
}

// helper: allocate memory via global mm
fn alloc_memory(len: u64, name: &str) -> u64 {
    let guard = MEMORY_MANAGER.lock();
    if let Some(ref mm) = *guard {
        let mut mm = mm.lock();
        let flags = crate::memory::MemoryMapFlags::ANONYMOUS | crate::memory::MemoryMapFlags::PRIVATE;
        let prot = crate::memory::MemoryProt::CPU_RW;
        match mm.map_memory(0, len, prot, flags, crate::memory::VMAType::Flexible, name) {
            Ok(addr) => addr,
            Err(e) => {
                tracing::error!("alloc_memory({}) failed: {}", name, e);
                0
            }
        }
    } else {
        0
    }
}

// main export dispatcher: intercepts game calls
pub fn hle_export_dispatcher(
    name: &str,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    r8: u64,
    r9: u64,
    stack_arg7: u64, // 7th arg — first stack-passed arg in SysV AMD64
    xmm0_bits: u64,
    xmm1_bits: u64,
) -> u64 {
    // Don't log math functions at info level to avoid spam
    if !is_float_function(name)
        && name != "sceUserServiceGetEvent"
        && name != "sceSystemServiceReceiveEvent"
        && name != "scePadReadState"
    {
        tracing::debug!(
            name,
            rdi = format!("0x{:X}", rdi),
            rsi = format!("0x{:X}", rsi),
            rdx = format!("0x{:X}", rdx),
            rcx = format!("0x{:X}", rcx),
            "Intercepted Export Call"
        );
    }

    match name {
        // ============================================================
        // C Runtime / LibcInternal
        // ============================================================
        "_init_env" => {
            tracing::info!("HLE handled _init_env");
            0
        }
        "atexit" | "__cxa_atexit" => {
            tracing::info!("HLE handled atexit/__cxa_atexit");
            0
        }
        "__cxa_guard_acquire" => {
            // Guard variable for static initialization.
            // arg0 (rdi) = guard_object pointer (i64*)
            // If *guard == 0, we set it to 1 and return 1 (needs init).
            // If *guard != 0, return 0 (already initialized).
            let guard_ptr = rdi as *mut i64;
            if !guard_ptr.is_null() {
                let val = unsafe { *guard_ptr };
                if val == 0 {
                    unsafe { *guard_ptr = 1 };
                    1 // needs initialization
                } else {
                    0 // already initialized
                }
            } else {
                0
            }
        }
        "__cxa_guard_release" => {
            // Companion to __cxa_guard_acquire — marks guard as fully initialized.
            // The guard_object's first byte should be set to indicate completion.
            let guard_ptr = rdi as *mut u8;
            if !guard_ptr.is_null() {
                unsafe { *guard_ptr = 1 };
            }
            0
        }
        "sceKernelIsNeoMode" => {
            // Returns 1 if PS4 Pro mode, 0 for base PS4.
            tracing::info!("HLE sceKernelIsNeoMode -> 0 (base PS4)");
            0
        }
        "catchReturnFromMain" | "exit" => {
            tracing::info!("══════════════════════════════════════════");
            tracing::info!("  Guest called exit — clean shutdown!");
            tracing::info!("══════════════════════════════════════════");
            std::process::exit(0);
        }

        // ============================================================
        // Memory allocation (libc)
        // ============================================================
        "malloc" | "_Znwm" => {
            let len = rdi;
            let alloc_size = if len == 0 { 4096 } else { len };
            tracing::info!(len = format!("0x{:X}", alloc_size), "HLE handled malloc/_Znwm");
            alloc_memory(alloc_size, "heap_alloc")
        }
        "free" | "_ZdlPv" => {
            // free(ptr) / operator delete -- we leak for now.
            tracing::debug!(ptr = format!("0x{:X}", rdi), "HLE free/_ZdlPv (no-op)");
            0
        }

        // ============================================================
        // String / Memory operations (libc passthrough)
        // ============================================================
        "memcpy" => {
            let dst = rdi as *mut u8;
            let src = rsi as *const u8;
            let n = rdx as usize;
            if !dst.is_null() && !src.is_null() && n > 0 {
                unsafe { std::ptr::copy_nonoverlapping(src, dst, n); }
            }
            rdi // return dst
        }
        "memset" => {
            let dst = rdi as *mut u8;
            let val = rsi as u8;
            let n = rdx as usize;
            if !dst.is_null() && n > 0 {
                unsafe { std::ptr::write_bytes(dst, val, n); }
            }
            rdi // return dst
        }
        "strcpy" => {
            let dst = rdi as *mut u8;
            let src = rsi as *const u8;
            if !dst.is_null() && !src.is_null() {
                unsafe {
                    let mut i = 0usize;
                    loop {
                        let c = *src.add(i);
                        *dst.add(i) = c;
                        if c == 0 { break; }
                        i += 1;
                        if i > 0x100000 { break; } // safety limit
                    }
                }
            }
            rdi
        }
        "strlen" => {
            let s = rdi as *const u8;
            if s.is_null() {
                return 0;
            }
            unsafe {
                let cstr = std::ffi::CStr::from_ptr(s as *const i8);
                cstr.to_bytes().len() as u64
            }
        }
        "strcmp" => {
            let s1 = rdi as *const u8;
            let s2 = rsi as *const u8;
            if s1.is_null() || s2.is_null() {
                return if s1 == s2 { 0 } else { 1 };
            }
            unsafe {
                let c1 = std::ffi::CStr::from_ptr(s1 as *const i8);
                let c2 = std::ffi::CStr::from_ptr(s2 as *const i8);
                match c1.cmp(c2) {
                    std::cmp::Ordering::Equal => 0u64,
                    std::cmp::Ordering::Less => (-1i64) as u64,
                    std::cmp::Ordering::Greater => 1u64,
                }
            }
        }
        "memcmp" => {
            let s1 = rdi as *const u8;
            let s2 = rsi as *const u8;
            let n = rdx as usize;
            if n == 0 { return 0; }
            if s1.is_null() || s2.is_null() { return 1; }
            let r = unsafe { libc::memcmp(s1 as *const _, s2 as *const _, n) };
            r as u64
        }
        "strcat" => {
            let dst = rdi as *mut u8;
            let src = rsi as *const u8;
            if !dst.is_null() && !src.is_null() {
                unsafe {
                    // Find end of dst
                    let mut i = 0usize;
                    while *dst.add(i) != 0 { i += 1; if i > 0x100000 { return rdi; } }
                    // Copy src
                    let mut j = 0usize;
                    loop {
                        let c = *src.add(j);
                        *dst.add(i + j) = c;
                        if c == 0 { break; }
                        j += 1;
                        if j > 0x100000 { break; }
                    }
                }
            }
            rdi
        }

        // ============================================================
        // Standard Output / Standard Error (libc)
        // ============================================================
        "printf" | "sceClibPrintf" => {
            let fmt_ptr = rdi as *const i8;
            let fmt = if !fmt_ptr.is_null() {
                unsafe { std::ffi::CStr::from_ptr(fmt_ptr).to_string_lossy().into_owned() }
            } else {
                "null".to_string()
            };

            // Primitive vararg parsing for the standard error format
            if fmt.contains("%s(%u): error in %s(): %s") {
                let file = if rsi != 0 { unsafe { std::ffi::CStr::from_ptr(rsi as *const i8).to_string_lossy() } } else { "?".into() };
                let line = rdx;
                let func = if rcx != 0 { unsafe { std::ffi::CStr::from_ptr(rcx as *const i8).to_string_lossy() } } else { "?".into() };
                let msg = if r8 != 0 { unsafe { std::ffi::CStr::from_ptr(r8 as *const i8).to_string_lossy() } } else { "?".into() };
                tracing::error!("GUEST ERROR FATAL: {}({}): error in {}(): {}", file, line, func, msg);
            } else {
                tracing::info!(fmt = %fmt, arg1=rsi, arg2=rdx, arg3=rcx, arg4=r8, "HLE printf");
            }
            0
        }
        "puts" => {
            if rdi != 0 {
                let s = unsafe { std::ffi::CStr::from_ptr(rdi as *const i8).to_string_lossy() };
                tracing::info!(msg = %s, "HLE puts");
            }
            0
        }
        "vsnprintf" => {
            // vsnprintf(buf, size, fmt, va_list) -> int
            // We can't truly implement va_list parsing, but we can at least
            // copy the format string as-is to prevent crashes.
            let buf = rdi as *mut u8;
            let size = rsi as usize;
            let fmt = rdx;
            if buf.is_null() || size == 0 {
                return 0;
            }
            if fmt != 0 {
                let fmt_str = unsafe { std::ffi::CStr::from_ptr(fmt as *const i8) };
                let fmt_bytes = fmt_str.to_bytes();
                let copy_len = std::cmp::min(fmt_bytes.len(), size - 1);
                unsafe {
                    std::ptr::copy_nonoverlapping(fmt_bytes.as_ptr(), buf, copy_len);
                    *buf.add(copy_len) = 0;
                }
                let fmt_str_cow = fmt_str.to_string_lossy();
                tracing::debug!(fmt = ?fmt_str, "HLE vsnprintf (format pass-through)");
                
                // Hack: If it's an assertion containing "%s", try to parse the arg from va_list (RCX).
                // x86_64 va_list struct: [0: gp_offset(u32), 4: fp_offset(u32), 8: overflow_arg_area(ptr), 16: reg_save_area(ptr)]
                if fmt_str_cow.contains("%s") && rcx != 0 {
                    unsafe {
                        let va_list_ptr = rcx as *const u32;
                        let gp_offset = std::ptr::read_unaligned(va_list_ptr);
                        let reg_save_area = std::ptr::read_unaligned(va_list_ptr.add(4) as *const u64); // offset 16
                        if gp_offset <= 40 && reg_save_area != 0 {
                            let arg_ptr_addr = reg_save_area + (gp_offset as u64);
                            let arg_ptr = std::ptr::read_unaligned(arg_ptr_addr as *const u64);
                            if arg_ptr != 0 {
                                let arg_str = std::ffi::CStr::from_ptr(arg_ptr as *const i8).to_string_lossy();
                                tracing::error!("vsnprintf extracted string arg: '{}'", arg_str);
                            }
                        }
                    }
                }

                copy_len as u64
            } else {
                unsafe { *buf = 0; }
                0
            }
        }
        "fopen" => {
            // fopen(filename, mode) -> FILE*
            if rdi == 0 { return 0; }
            let filename = unsafe { std::ffi::CStr::from_ptr(rdi as *const i8).to_string_lossy() };
            let mode = if rsi != 0 {
                unsafe { std::ffi::CStr::from_ptr(rsi as *const i8).to_string_lossy().to_string() }
            } else {
                "r".to_string()
            };

            // Resolve PS4 path → host path
            if let Some(host_path) = resolve_ps4_path(&filename) {
                if host_path.exists() {
                    match std::fs::File::open(&host_path) {
                        Ok(file) => {
                            let fake_ptr = NEXT_FILE_PTR.fetch_add(0x100, Ordering::SeqCst);
                            let mut guard = OPEN_CFILES.lock();
                            if let Some(ref mut map) = *guard {
                                map.insert(fake_ptr, OpenCFile {
                                    file,
                                    path: filename.to_string(),
                                });
                            }
                            tracing::info!(
                                filename = %filename,
                                host = %host_path.display(),
                                ptr = format!("0x{:X}", fake_ptr),
                                "HLE fopen OK"
                            );
                            fake_ptr
                        }
                        Err(e) => {
                            tracing::warn!(filename = %filename, error = %e, "HLE fopen failed to open");
                            0
                        }
                    }
                } else {
                    tracing::warn!(filename = %filename, host = %host_path.display(), "HLE fopen: file not found");
                    0
                }
            } else {
                tracing::warn!(filename = %filename, mode = %mode, "HLE fopen: path not mounted");
                0
            }
        }
        "fread" => {
            // fread(ptr, size, count, FILE*) -> items_read
            let buf = rdi as *mut u8;
            let size = rsi as usize;
            let count = rdx as usize;
            let file_ptr = rcx;
            let total = size * count;

            if buf.is_null() || total == 0 {
                return 0;
            }

            let mut guard = OPEN_CFILES.lock();
            if let Some(ref mut map) = *guard {
                if let Some(open_file) = map.get_mut(&file_ptr) {
                    let slice = unsafe { std::slice::from_raw_parts_mut(buf, total) };
                    match open_file.file.read(slice) {
                        Ok(bytes_read) => {
                            let items = bytes_read / size;
                            tracing::debug!(
                                ptr = format!("0x{:X}", file_ptr),
                                size, count, bytes_read, items,
                                "HLE fread"
                            );
                            items as u64
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "HLE fread error");
                            0
                        }
                    }
                } else {
                    tracing::warn!(ptr = format!("0x{:X}", file_ptr), "HLE fread: invalid FILE*");
                    0
                }
            } else {
                0
            }
        }
        "fclose" => {
            // fclose(FILE*) -> 0 on success
            let file_ptr = rdi;
            let mut guard = OPEN_CFILES.lock();
            if let Some(ref mut map) = *guard {
                if let Some(removed) = map.remove(&file_ptr) {
                    tracing::info!(path = %removed.path, "HLE fclose");
                    0
                } else {
                    tracing::warn!(ptr = format!("0x{:X}", file_ptr), "HLE fclose: invalid FILE*");
                    -1i64 as u64 // EOF
                }
            } else {
                -1i64 as u64
            }
        }
        "fseek" => {
            // fseek(FILE*, offset, whence) -> 0 on success
            let file_ptr = rdi;
            let offset = rsi as i64;
            let whence = rdx as i32;

            let seek_from = match whence {
                0 => SeekFrom::Start(offset as u64),  // SEEK_SET
                1 => SeekFrom::Current(offset),         // SEEK_CUR
                2 => SeekFrom::End(offset),             // SEEK_END
                _ => return -1i64 as u64,
            };

            let mut guard = OPEN_CFILES.lock();
            if let Some(ref mut map) = *guard {
                if let Some(open_file) = map.get_mut(&file_ptr) {
                    match open_file.file.seek(seek_from) {
                        Ok(_) => 0,
                        Err(e) => {
                            tracing::error!(error = %e, "HLE fseek error");
                            -1i64 as u64
                        }
                    }
                } else {
                    -1i64 as u64
                }
            } else {
                -1i64 as u64
            }
        }
        "ftell" => {
            // ftell(FILE*) -> current position
            let file_ptr = rdi;
            let mut guard = OPEN_CFILES.lock();
            if let Some(ref mut map) = *guard {
                if let Some(open_file) = map.get_mut(&file_ptr) {
                    match open_file.file.stream_position() {
                        Ok(pos) => pos,
                        Err(_) => -1i64 as u64,
                    }
                } else {
                    -1i64 as u64
                }
            } else {
                -1i64 as u64
            }
        }

        // ============================================================
        // Kernel Memory Management
        // ============================================================
        "sceKernelGetDirectMemorySize" => {
            tracing::info!("HLE sceKernelGetDirectMemorySize");
            5632 * 1024 * 1024 // 5.5 GB
        }
        "sceKernelAllocateDirectMemory" => {
            let _search_start = rdi;
            let _search_end = rsi;
            let len = rdx;
            let alignment = rcx;
            let memory_type = r8 as i32;
            let phys_addr_out = r9 as *mut u64;

            tracing::info!(
                len = format!("0x{:X}", len),
                alignment = format!("0x{:X}", alignment),
                memory_type,
                "HLE sceKernelAllocateDirectMemory"
            );

            // On PS4, direct memory is GPU-accessible and phys==virt.
            // We allocate virtual memory immediately and return that as the "physical" address.
            // sceKernelMapDirectMemory will then just return this same address.
            let guard = MEMORY_MANAGER.lock();
            if let Some(ref mm) = *guard {
                let mut mm = mm.lock();
                let flags = crate::memory::MemoryMapFlags::ANONYMOUS | crate::memory::MemoryMapFlags::PRIVATE;
                let prot = crate::memory::MemoryProt::CPU_RW;
                match mm.map_memory(0, len, prot, flags, crate::memory::VMAType::Direct, "direct_phys") {
                    Ok(virt_addr) => {
                        if !phys_addr_out.is_null() {
                            unsafe { *phys_addr_out = virt_addr };
                        }
                        // Track phys==virt mapping
                        register_phys_mapping(virt_addr, virt_addr, len);
                        tracing::info!(
                            phys_virt = format!("0x{:X}", virt_addr),
                            "Direct memory allocated (phys==virt)"
                        );
                        0 // ORBIS_OK
                    }
                    Err(e) => {
                        tracing::error!("sceKernelAllocateDirectMemory: {}", e);
                        0x8002000C
                    }
                }
            } else {
                0x8002000C
            }
        }
        "sceKernelAllocateMainDirectMemory" => {
            let len = rdi;
            let alignment = rsi;
            let memory_type = rdx as i32;
            let phys_addr_out = rcx as *mut u64;

            tracing::info!(
                len = format!("0x{:X}", len),
                alignment = format!("0x{:X}", alignment),
                memory_type,
                "HLE sceKernelAllocateMainDirectMemory"
            );

            let guard = MEMORY_MANAGER.lock();
            if let Some(ref mm) = *guard {
                let mut mm = mm.lock();
                let flags = crate::memory::MemoryMapFlags::ANONYMOUS | crate::memory::MemoryMapFlags::PRIVATE;
                let prot = crate::memory::MemoryProt::CPU_RW;
                match mm.map_memory(0, len, prot, flags, crate::memory::VMAType::Direct, "direct_main_phys") {
                    Ok(virt_addr) => {
                        if !phys_addr_out.is_null() {
                            unsafe { *phys_addr_out = virt_addr };
                        }
                        register_phys_mapping(virt_addr, virt_addr, len);
                        tracing::info!(
                            phys_virt = format!("0x{:X}", virt_addr),
                            "Main direct memory allocated (phys==virt)"
                        );
                        0
                    }
                    Err(e) => {
                        tracing::error!("sceKernelAllocateMainDirectMemory: {}", e);
                        0x8002000C
                    }
                }
            } else {
                0x8002000C
            }
        }
        "sceKernelMapDirectMemory" => {
            let addr_ptr = rdi as *mut u64;
            let len = rsi;
            let prot = rdx as u32;
            let flags = rcx as u32;
            let phys_addr = r8;
            let _alignment = r9;

            tracing::info!(
                len = format!("0x{:X}", len),
                prot = format!("0x{:X}", prot),
                flags = format!("0x{:X}", flags),
                phys = format!("0x{:X}", phys_addr),
                "HLE sceKernelMapDirectMemory"
            );

            // Since we already allocated virt memory in AllocateDirectMemory (phys==virt),
            // just return the physical address as the virtual address.
            // The memory is already mapped and accessible.
            if !addr_ptr.is_null() {
                unsafe { *addr_ptr = phys_addr };
            }
            tracing::info!(
                addr = format!("0x{:X}", phys_addr),
                "Direct memory mapped (phys==virt, already allocated)"
            );
            0
        }
        "sceKernelMunmap" | "sceKernelReleaseDirectMemory" => {
            tracing::info!("HLE {} (stubbed OK)", name);
            0
        }

        // ============================================================
        // Kernel Event Queues
        // ============================================================
        "sceKernelCreateEqueue" => {
            // int sceKernelCreateEqueue(SceKernelEqueue* eq, const char* name)
            // eq = rdi (output pointer), name = rsi
            let eq_out = rdi as *mut i32;
            let eq_name = if rsi != 0 {
                unsafe { std::ffi::CStr::from_ptr(rsi as *const i8).to_string_lossy().to_string() }
            } else {
                "unnamed".to_string()
            };
            let id = NEXT_EQUEUE_ID.fetch_add(1, Ordering::SeqCst);
            if !eq_out.is_null() {
                unsafe { *eq_out = id };
            }
            tracing::info!(id, name = %eq_name, "HLE sceKernelCreateEqueue");
            0
        }
        "sceKernelDeleteEqueue" => {
            tracing::info!("HLE sceKernelDeleteEqueue (stubbed OK)");
            0
        }

        // ============================================================
        // Threading / Sync
        // ============================================================
        "scePthreadMutexDestroy" | "scePthreadMutexInit"
        | "scePthreadMutexLock" | "scePthreadMutexUnlock" => {
            tracing::debug!("HLE {} (stubbed OK)", name);
            0
        }
        "scePthreadCreate" => {
            // scePthreadCreate(thread*, attr*, start_routine, arg, name)
            let thread_out = rdi as *mut u64;
            let _attr = rsi;
            let start_routine = rdx;
            let arg = rcx;
            let name = if r8 != 0 {
                unsafe { std::ffi::CStr::from_ptr(r8 as *const i8).to_string_lossy().to_string() }
            } else {
                "unnamed".to_string()
            };
            
            tracing::info!(name = %name, "HLE scePthreadCreate (SPAWNING NATiVE THREAD)");

            let memory_arc = {
                let guard = MEMORY_MANAGER.lock();
                guard.as_ref().cloned()
            };

            if let Some(mem_arc) = memory_arc {
                let tcb_size = 4096;
                let tcb_base = {
                    let mut mem = mem_arc.lock();
                    mem.map_memory(
                        0,
                        tcb_size,
                        crate::memory::MemoryProt::CPU_READ | crate::memory::MemoryProt::CPU_WRITE,
                        crate::memory::MemoryMapFlags::ANONYMOUS | crate::memory::MemoryMapFlags::PRIVATE,
                        crate::memory::VMAType::ThreadData,
                        &format!("Thread TCB: {}", name),
                    ).unwrap_or(0)
                };

                let stack_size = 2 * 1024 * 1024;
                let stack_base = {
                    let mut mem = mem_arc.lock();
                    mem.map_memory(
                        0,
                        stack_size,
                        crate::memory::MemoryProt::CPU_READ | crate::memory::MemoryProt::CPU_WRITE,
                        crate::memory::MemoryMapFlags::ANONYMOUS | crate::memory::MemoryMapFlags::PRIVATE,
                        crate::memory::VMAType::ThreadData,
                        &format!("Thread Stack: {}", name),
                    ).unwrap_or(0)
                };

                if tcb_base != 0 && stack_base != 0 {
                    let mut rsp = stack_base + stack_size;
                    
                    // ABI: RSP % 16 == 8 upon entry into the function
                    rsp &= !0xF;
                    rsp -= 8;
                    unsafe { std::ptr::write(rsp as *mut u64, 0); }

                    if !thread_out.is_null() {
                        unsafe { *thread_out = tcb_base };
                    }

                    // Spawn the OS thread!
                    let _ = std::thread::Builder::new().name(name.clone()).spawn(move || {
                        tracing::info!(name = %name, "Thread started natively, switching context to guest thread");
                        
                        unsafe {
                            let mut host_fs_base: u64 = 0;
                            let p_host_fs_base = &mut host_fs_base as *mut u64;
                            if libc::syscall(libc::SYS_arch_prctl, 0x1003, p_host_fs_base) == 0 {
                                crate::hle::dispatcher::register_thread_fs(host_fs_base);
                            }

                            let tcb_ptr = tcb_base as *mut u64;
                            std::ptr::write(tcb_ptr, tcb_base);

                            tracing::info!(
                                entry = format!("0x{:016X}", start_routine),
                                arg = format!("0x{:016X}", arg),
                                stack = format!("0x{:016X}", rsp),
                                tcb = format!("0x{:016X}", tcb_base),
                                "Thread switching TLS and jumping to guest code"
                            );

                            if libc::syscall(libc::SYS_arch_prctl, 0x1002, tcb_base) != 0 {
                                std::process::abort();
                            }

                            crate::cpu::jump::jump_to_guest_thread(start_routine, rsp, arg);
                        }
                    });

                    0 
                } else {
                    tracing::error!("Failed to allocate TCB/Stack for thread");
                    0x8002000C
                }
            } else {
                0x8002000C
            }
        }

        // ============================================================
        // Video Out
        // ============================================================
        "sceVideoOutOpen" => {
            // Returns a video output handle. Game checks for > 0.
            tracing::info!("HLE sceVideoOutOpen -> handle 1");
            1 // Return a valid handle
        }
        "sceVideoOutSetFlipRate" | "sceVideoOutClose" => {
            tracing::info!("HLE {} (stubbed OK)", name);
            0
        }
        "sceVideoOutGetResolutionStatus" => {
            // sceVideoOutGetResolutionStatus(handle, status_out)
            // Writes a SceVideoOutResolutionStatus struct.
            // We fill in 1920x1080 @ 60Hz.
            let status_ptr = rsi as *mut u8;
            if !status_ptr.is_null() {
                unsafe {
                    // Zero out the struct first (assume ~32 bytes)
                    std::ptr::write_bytes(status_ptr, 0, 32);
                    // fullWidth = 1920 (u32 at offset 0)
                    *(status_ptr as *mut u32) = 1920;
                    // fullHeight = 1080 (u32 at offset 4)
                    *(status_ptr.add(4) as *mut u32) = 1080;
                    // paneWidth = 1920 (u32 at offset 8)
                    *(status_ptr.add(8) as *mut u32) = 1920;
                    // paneHeight = 1080 (u32 at offset 12)
                    *(status_ptr.add(12) as *mut u32) = 1080;
                }
            }
            tracing::info!("HLE sceVideoOutGetResolutionStatus -> 1920x1080");
            0
        }
        "sceVideoOutSetBufferAttribute" => {
            // sceVideoOutSetBufferAttribute(
            //   SceVideoOutBufferAttribute* attr,   <- rdi (output struct to fill)
            //   uint32_t pixelFormat,               <- rsi
            //   uint32_t tilingMode,                <- rdx
            //   uint32_t aspectRatio,               <- rcx
            //   uint32_t width,                     <- r8
            //   uint32_t height,                    <- r9
            //   uint32_t pitchInPixel               <- stack arg +8 from RSP
            // )
            // SceVideoOutBufferAttribute layout (matches PS4 SDK / shadPS4):
            //   u32 pixelFormat    @ +0
            //   u32 tilingMode     @ +4
            //   s32 aspectRatio    @ +8
            //   u32 width          @ +12
            //   u32 height         @ +16
            //   u32 pitchInPixel   @ +20
            //   u32 option         @ +24
            //   u32 reserved0      @ +28
            //   u64 reserved1      @ +32
            let attr_ptr = rdi as *mut u32;
            let pixel_format = rsi as u32;
            let tiling_mode  = rdx as u32;
            let aspect_ratio = rcx as u32;
            let width        = r8  as u32;
            let height       = r9  as u32;
            let pitch = if stack_arg7 != 0 { stack_arg7 as u32 } else { width };

            if !attr_ptr.is_null() {
                unsafe {
                    // Do NOT zero the struct - the game might have put other data in here!
                    *attr_ptr.add(0) = pixel_format; // pixelFormat
                    *attr_ptr.add(1) = tiling_mode;     // tilingMode
                    *attr_ptr.add(2) = aspect_ratio;    // aspectRatio
                    *attr_ptr.add(3) = width;           // width
                    *attr_ptr.add(4) = height;          // height
                    *attr_ptr.add(5) = pitch;           // pitchInPixel
                    *attr_ptr.add(6) = 0;               // option
                }
            }
            tracing::info!(
                pixel_format = format!("0x{:08X}", pixel_format),
                tiling_mode,
                aspect_ratio,
                width,
                height,
                pitch,
                "HLE sceVideoOutSetBufferAttribute -> filled attr struct"
            );
            0
        }
        "sceVideoOutRegisterBuffers" => {
            // sceVideoOutRegisterBuffers(handle, startIndex, addresses[], bufNum, attr)
            // addresses is an array of buf_num void* pointers the game has already allocated.
            // The game will write GPU commands using these addresses, so we just acknowledge.
            let start_index = rsi as i32;
            let addresses_ptr = rdx as *const u64;
            let buf_num = rcx as i32;
            tracing::info!(
                start_index,
                buf_num,
                "HLE sceVideoOutRegisterBuffers (acknowledged)"
            );
            if !addresses_ptr.is_null() && buf_num > 0 {
                for i in 0..buf_num {
                    let addr = unsafe { *addresses_ptr.add(i as usize) };
                    tracing::info!(
                        i,
                        addr = format!("0x{:X}", addr),
                        "  -> buffer address"
                    );
                }
            }
            0
        }

        // ============================================================
        // GNM (GPU command submission)
        // ============================================================
        "sceGnmAddEqEvent" | "sceGnmDeleteEqEvent" => {
            tracing::info!("HLE {} (stubbed OK)", name);
            0
        }
        "sceGnmDrawInitDefaultHardwareState350" => {
            // sceGnmDrawInitDefaultHardwareState350(u32* cmdbuf, u32 size) -> u32
            // Must fill cmdbuf with GPU init PM4 packets and return HwInitPacketSize.
            // Returning 0 signals failure and causes the game to skip GPU init → crash.
            const HW_INIT_PACKET_SIZE: u32 = 0x100; // 256 dwords = 1024 bytes
            let cmdbuf = rdi as *mut u32;
            let size = rsi as u32;

            if cmdbuf.is_null() || size < HW_INIT_PACKET_SIZE {
                tracing::warn!("sceGnmDrawInitDefaultHardwareState350: buffer too small");
                return 0;
            }

            unsafe {
                let init_sequence: [u32; 126] = [
                    0xc0001200, 0x00000000, 0xc0017600, 0x00000216,
                    0xffffffff, 0xc0017600, 0x00000217, 0xffffffff,
                    0xc0017600, 0x00000215, 0x00000000, 0xc0016900,
                    0x000002f9, 0x0000002d, 0xc0016900, 0x00000282,
                    0x00000008, 0xc0016900, 0x00000280, 0x00080008,
                    0xc0016900, 0x00000281, 0xffff0000, 0xc0016900,
                    0x00000204, 0x00000000, 0xc0016900, 0x00000206,
                    0x0000043f, 0xc0016900, 0x00000083, 0x0000ffff,
                    0xc0016900, 0x00000317, 0x00000010, 0xc0016900,
                    0x000002fa, 0x3f800000, 0xc0016900, 0x000002fc,
                    0x3f800000, 0xc0016900, 0x000002fb, 0x3f800000,
                    0xc0016900, 0x000002fd, 0x3f800000, 0xc0016900,
                    0x00000202, 0x00cc0010, 0xc0016900, 0x0000030e,
                    0xffffffff, 0xc0016900, 0x0000030f, 0xffffffff,
                    0xc0002f00, 0x00000001, 0xc0017600, 0x00000007,
                    0x001701ff, 0xc0017600, 0x00000046, 0x001701fd,
                    0xc0017600, 0x00000087, 0x001701ff, 0xc0017600,
                    0x000000c7, 0x001701fd, 0xc0017600, 0x00000107,
                    0x00000017, 0xc0017600, 0x00000147, 0x001701fd,
                    0xc0017600, 0x00000047, 0x0000001c, 0xc0016900,
                    0x000001b1, 0x00000002, 0xc0016900, 0x00000101,
                    0x00000000, 0xc0016900, 0x00000100, 0xffffffff,
                    0xc0016900, 0x00000103, 0x00000000, 0xc0016900,
                    0x00000284, 0x00000000, 0xc0016900, 0x00000290,
                    0x00000000, 0xc0016900, 0x000002ae, 0x00000000,
                    0xc0016900, 0x00000102, 0x00000000, 0xc0016900,
                    0x00000292, 0x00000000, 0xc0016900, 0x00000293,
                    0x06020000, 0xc0016900, 0x000002f8, 0x00000000,
                    0xc0016900, 0x000002de, 0x000001e9, 0xc0036900,
                    0x00000295, 0x00000100, 0x00000100, 0x00000004,
                    0xc0017900, 0x00000200, 0xe0000000, 0xc0016900,
                    0x000002aa, 0x000000ff,
                ];

                for (i, val) in init_sequence.iter().enumerate() {
                    *cmdbuf.add(i) = *val;
                }

                let mut offset = init_sequence.len();
                while offset < HW_INIT_PACKET_SIZE as usize {
                    if offset + 2 <= HW_INIT_PACKET_SIZE as usize {
                        *cmdbuf.add(offset) = 0xC0001000;
                        *cmdbuf.add(offset + 1) = 0;
                        offset += 2;
                    } else {
                        *cmdbuf.add(offset) = 0;
                        offset += 1;
                    }
                }
            }

            tracing::info!(
                size,
                "HLE sceGnmDrawInitDefaultHardwareState350 -> filled {} dwords",
                HW_INIT_PACKET_SIZE
            );
            unsafe { cmdbuf.add(HW_INIT_PACKET_SIZE as usize) as u64 }
        }
        "sceGnmInsertWaitFlipDone" => {
            // sceGnmInsertWaitFlipDone(u32* cmdbuf, s32 videoOutHandle, s32 bufIdx)
            // Writes a wait-for-flip PM4 command, size is typically 7 dwords.
            let cmdbuf = rdi as *mut u32;
            let video_out_handle = rsi as i32;
            let buf_idx = rdx as i32;
            let size = 7_usize; // fixed 7 dwords for the wait command

            if !cmdbuf.is_null() {
                unsafe {
                    // NOP packet filling the 7 dwords
                    let header_size = size - 1; // PM4 header counts N-1
                    *cmdbuf = 0xC0001000 | ((header_size as u32 - 1) << 16); // NOP with size
                    for i in 1..size {
                        *cmdbuf.add(i) = 0;
                    }
                    cmdbuf.add(size) as u64
                }
            } else {
                0
            }
        }
        // fzyMKs9kim0 — sceKernelWaitEqueue
        "fzyMKs9kim0" | "fzyMKs9kim0#R#S" => {
            // Args: eq(rdi), out_events(rsi), max_events(rdx), out_count(rcx), timeout(r8)
            let out_count = rcx as *mut i32;
            if !out_count.is_null() {
                unsafe { *out_count = 0; }
            }
            // Sleep slightly to prevent tight loop spinning
            unsafe { libc::usleep(1000); }
            0 // SCE_OK
        }
        // zwY0YV91TTI — sceGnmSubmitCommandBuffers
        // Args: count(rdi), dcb_gpu_addrs*(rsi), dcb_sizes*(rdx), ccb_gpu_addrs*(rcx), ccb_sizes*(r8)
        "zwY0YV91TTI" | "zwY0YV91TTI#R#S" => {
            let count = rdi as u32;
            let dcb_addrs_ptr = rsi as usize;
            let dcb_sizes_ptr = rdx as usize;

            if count > 0 && count < 100 
                && dcb_addrs_ptr > 0x10000 && dcb_addrs_ptr % 8 == 0 
                && dcb_sizes_ptr > 0x10000 && dcb_sizes_ptr % 4 == 0 
            {
                let dcb_addrs = dcb_addrs_ptr as *const *const u32;
                let dcb_sizes = dcb_sizes_ptr as *const u32;

                for i in 0..count as usize {
                    let dcb_ptr = unsafe { *dcb_addrs.add(i) };
                    let dcb_size_bytes = unsafe { *dcb_sizes.add(i) };
                    let dcb_size_dw = dcb_size_bytes / 4;

                    if !dcb_ptr.is_null() && (dcb_ptr as usize) > 0x10000 && dcb_size_dw > 0 && dcb_size_dw < 0x1000000 {
                        let dcb = unsafe {
                            std::slice::from_raw_parts(dcb_ptr, dcb_size_dw as usize)
                        };
                        anotherps4_gpu::with_gpu(|gpu| {
                            gpu.submit_gfx(dcb, &[]);
                        });
                    }
                }
            }
            // Frame pacing
            unsafe { libc::usleep(16000); }
            0 // SCE_OK
        }
        // sceGnmSubmitDone — signal end of frame
        "E90Gz79LdBQ" | "E90Gz79LdBQ#R#S" => {
            anotherps4_gpu::with_gpu(|gpu| {
                gpu.submit_done();
            });
            0
        }
        // Other GNM NIDs called during rendering (return success silently)
        "W1Etj-jlW7Y" | "W1Etj-jlW7Y#B#C"   // VideoOut related
        | "5uFKckiJYRM" | "5uFKckiJYRM#B#C"
        | "+AFvOEXrKJk" | "+AFvOEXrKJk#B#C"
        | "GGsn7jMTxw4" | "GGsn7jMTxw4#B#C"
        | "7qZVNgEu+SY" | "7qZVNgEu+SY#B#C"
        | "gAhCn6UiU4Y" | "gAhCn6UiU4Y#B#C"
        | "HlTPoZ-oY7Y" | "HlTPoZ-oY7Y#B#C"
        | "xbxNatawohc" | "xbxNatawohc#B#C"
        | "yvZ73uQUqrk" | "yvZ73uQUqrk#B#C" => {
            0 // Silent success — these are GNM/VideoOut init functions
        }
        // ============================================================
        // Pad (Controller Input)
        // ============================================================
        "scePadInit" => {
            tracing::info!("HLE scePadInit (stubbed OK)");
            0
        }
        "scePadOpen" => {
            // scePadOpen(userId, type, index, param) -> handle
            // Return a valid pad handle (1-4 based on index).
            let pad_handle = (rdx as i32 + 1).max(1);
            tracing::info!(handle = pad_handle, "HLE scePadOpen");
            pad_handle as u64
        }
        "scePadReadState" => {
            // scePadReadState(handle, ScePadData* data) -> 0
            // Write a zeroed ScePadData struct (no buttons pressed).
            // ScePadData is ~64 bytes: buttons(u32), leftX/Y(u8), rightX/Y(u8),
            // digitalButtons(u8), connected(u8), timestamp(u64), etc.
            let data_ptr = rsi as *mut u8;
            if !data_ptr.is_null() {
                unsafe {
                    std::ptr::write_bytes(data_ptr, 0, 80); // zero out ~80 bytes
                    // offset 4: left analog X center = 128
                    *data_ptr.add(4) = 128;
                    // offset 5: left analog Y center = 128
                    *data_ptr.add(5) = 128;
                    // offset 6: right analog X center = 128
                    *data_ptr.add(6) = 128;
                    // offset 7: right analog Y center = 128
                    *data_ptr.add(7) = 128;
                    // offset 19: connected = 1
                    *data_ptr.add(19) = 1;
                }
            }
            tracing::trace!("HLE scePadReadState -> neutral");
            0
        }
        "scePadClose" => {
            tracing::info!("HLE scePadClose (stubbed OK)");
            0
        }

        // ============================================================
        // User Service
        // ============================================================
        "sceUserServiceInitialize" | "sceUserServiceTerminate" => {
            tracing::info!("HLE {} (stubbed OK)", name);
            0
        }
        "sceUserServiceGetInitialUser" => {
            // Writes a user ID to output pointer. Use user ID 1.
            let user_id_out = rdi as *mut i32;
            if !user_id_out.is_null() {
                unsafe { *user_id_out = 1 };
            }
            tracing::info!("HLE sceUserServiceGetInitialUser -> user 1");
            0
        }
        "sceUserServiceGetEvent" => {
            // sceUserServiceGetEvent(SceUserServiceEvent* event) -> int
            // Return SCE_USER_SERVICE_ERROR_NO_EVENT to indicate no pending events.
            // This is called in a tight poll loop, so log at trace level only.
            tracing::trace!("HLE sceUserServiceGetEvent -> no event");
            0x80990004u64 // SCE_USER_SERVICE_ERROR_NO_EVENT
        }
        "sceUserServiceGetLoginUserIdList" => {
            // Writes an array of user IDs. Provide user 1 in slot 0, rest -1.
            let list_out = rdi as *mut i32;
            if !list_out.is_null() {
                unsafe {
                    *list_out = 1;           // user 1 logged in
                    *list_out.add(1) = -1;   // no more users
                    *list_out.add(2) = -1;
                    *list_out.add(3) = -1;
                }
            }
            tracing::info!("HLE sceUserServiceGetLoginUserIdList");
            0
        }

        // ============================================================
        // System Services (stubs returning ORBIS_OK)
        // ============================================================
        "sceSystemServiceHideSplashScreen" | "sceSystemServiceParamGetInt"
        | "sceSysmoduleLoadModule" | "sceCommonDialogInitialize"
        | "sceSaveDataInitialize3" | "sceSaveDataTerminate"
        | "sceNpSetContentRestriction" | "sceNpSetNpTitleId"
        | "sceNpScoreCreateNpTitleCtxA"
        | "sceScreenShotSetOverlayImageWithOrigin" => {
            tracing::info!("HLE {} (stubbed OK)", name);
            0
        }
        "sceSystemServiceReceiveEvent" => {
            // sceSystemServiceReceiveEvent(SceSystemServiceEvent* event) -> int
            // Polled in the main loop. Return error code = no event.
            tracing::trace!("HLE sceSystemServiceReceiveEvent -> no event");
            0x80A10004u64 // SCE_SYSTEM_SERVICE_ERROR_NO_EVENT
        }
        "sceSystemServiceGetStatus" => {
            // sceSystemServiceGetStatus(SceSystemServiceStatus* status) -> 0
            let status_ptr = rdi as *mut u8;
            if !status_ptr.is_null() {
                unsafe {
                    // Zero out the status struct (assume ~64 bytes)
                    std::ptr::write_bytes(status_ptr, 0, 64);
                }
            }
            tracing::info!("HLE sceSystemServiceGetStatus (stubbed OK)");

            // HACK: Force a Swapchain Present so the window updates despite the
            // dataFormatEncoder inner crash!
            anotherps4_gpu::with_gpu(|gpu| {
                gpu.submit_done();
            });

            0
        }
        "sceSaveDataDialogUpdateStatus" => {
            // Returns the status of the save data dialog.
            // SCE_COMMON_DIALOG_STATUS_NONE = 0
            tracing::debug!("HLE sceSaveDataDialogUpdateStatus -> 0 (NONE)");
            0
        }
        "sceSaveDataDialogGetResult" => {
            tracing::debug!("HLE sceSaveDataDialogGetResult (stubbed OK)");
            0
        }

        // ============================================================
        // NP Trophy
        // ============================================================
        "sceNpTrophyCreateHandle" => {
            // Writes a handle to output pointer. Return handle=1.
            let handle_out = rdi as *mut i32;
            if !handle_out.is_null() {
                unsafe { *handle_out = 1 };
            }
            tracing::info!("HLE sceNpTrophyCreateHandle -> handle 1");
            0
        }
        "sceNpTrophyCreateContext" => {
            // Writes a context to output pointer. Return context=1.
            let ctx_out = rdi as *mut i32;
            if !ctx_out.is_null() {
                unsafe { *ctx_out = 1 };
            }
            tracing::info!("HLE sceNpTrophyCreateContext -> ctx 1");
            0
        }
        // ============================================================
        // Audio Out
        // ============================================================
        "sceAudioOutInit" => {
            tracing::info!("HLE sceAudioOutInit (stubbed OK)");
            0
        }
        "sceAudioOutOpen" => {
            // sceAudioOutOpen(userId, type, index, granularity, sampleRate, paramType)
            // Return a valid audio handle.
            tracing::info!("HLE sceAudioOutOpen -> handle 1");
            1
        }

        "sceNpTrophyRegisterContext" => {
            tracing::info!("HLE sceNpTrophyRegisterContext (stubbed OK)");
            0
        }

        // ============================================================
        // Math functions (float args in XMM0, return in XMM0)
        // ============================================================
        "sinf" => {
            let val = f32::from_bits(xmm0_bits as u32);
            let result = val.sin();
            (result.to_bits() as u64)
        }
        "tanf" => {
            let val = f32::from_bits(xmm0_bits as u32);
            let result = val.tan();
            (result.to_bits() as u64)
        }
        "asinf" => {
            let val = f32::from_bits(xmm0_bits as u32);
            let result = val.asin();
            (result.to_bits() as u64)
        }
        "acosf" => {
            let val = f32::from_bits(xmm0_bits as u32);
            let result = val.acos();
            (result.to_bits() as u64)
        }
        "atan2f" => {
            let y = f32::from_bits(xmm0_bits as u32);
            let x = f32::from_bits(xmm1_bits as u32);
            let result = y.atan2(x);
            (result.to_bits() as u64)
        }
        "rand" => {
            // Simple PRNG — not cryptographic, just for game use.
            // Return a value in [0, RAND_MAX] where RAND_MAX = 0x7FFFFFFF
            static RAND_STATE: AtomicU64 = AtomicU64::new(12345);
            let mut state = RAND_STATE.load(Ordering::SeqCst);
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            RAND_STATE.store(state, Ordering::SeqCst);
            (state >> 16) & 0x7FFFFFFF
        }

        // ============================================================
        // Default: unhandled
        // ============================================================
        _ => {
            use std::sync::atomic::{AtomicU64, Ordering as AO};
            static UNHANDLED_CALLS: AtomicU64 = AtomicU64::new(0);
            let count = UNHANDLED_CALLS.fetch_add(1, AO::Relaxed);
            
            // Only log first occurrence and every 10000th call to avoid spam
            if count < 50 || count % 10000 == 0 {
                tracing::warn!("Export dispatcher: unhandled stub '{}' (call #{}), returning 0", name, count);
            }
            
            // If we're being called in a tight loop (>1000 calls), add a small sleep
            // to prevent CPU spinning. The game is probably in a render/poll loop.
            if count > 1000 {
                unsafe { libc::usleep(1000); } // 1ms
            }
            
            0
        }
    }
}

/// Check if a function returns its result via XMM0 (float return).
pub fn is_float_function(name: &str) -> bool {
    matches!(name, "sinf" | "tanf" | "asinf" | "acosf" | "cosf" | "sqrtf" | "atan2f" | "floorf" | "ceilf" | "fabsf" | "fmodf")
}
