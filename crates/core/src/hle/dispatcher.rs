//! Syscall Dispatcher for HLE using SIGILL traps.
//!
//! Because expanding a 2-byte `SYSCALL` into a 5-byte `CALL` would corrupt surrounding instructions,
//! we instead patch `0x0F 0x05` (SYSCALL) to `0x0F 0x0B` (UD2).
//! UD2 is an undefined instruction that raises a SIGILL signal.
//! We catch SIGILL, read the CPU registers, dispatch the syscall, advance RIP by 2 bytes, and resume.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Once;
use libc::{c_int, siginfo_t, sigaction, ucontext_t, SA_SIGINFO, SIGILL};

static INIT_SIGNAL: Once = Once::new();

/// Map of up to 64 Thread IDs to their Host FS_BASE.
/// Written once per thread creation, read in signal handlers.
pub static mut THREAD_FS_MAP: [(u32, u64); 64] = [(0, 0); 64];

pub fn register_thread_fs(fs_base: u64) {
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as u32;
    unsafe {
        for i in 0..64 {
            if THREAD_FS_MAP[i].0 == 0 || THREAD_FS_MAP[i].0 == tid {
                THREAD_FS_MAP[i] = (tid, fs_base);
                break;
            }
        }
    }
}

fn get_thread_fs() -> u64 {
    let tid: u32;
    unsafe {
        std::arch::asm!(
            "syscall",
            in("rax") libc::SYS_gettid,
            lateout("rax") tid,
            out("rcx") _, out("r11") _,
            options(nostack, preserves_flags)
        );
    }
    unsafe {
        for i in 0..64 {
            if THREAD_FS_MAP[i].0 == tid {
                return THREAD_FS_MAP[i].1;
            }
        }
    }
    0
}

/// Dispatches the syscall intercepted from the UD2 trap.
pub fn hle_syscall_dispatcher(
    sys_id: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    _arg5: u64,
    _arg6: u64,
) -> u64 {
    tracing::trace!(
        sys_id,
        a1 = ?arg1,
        a2 = ?arg2,
        a3 = ?arg3,
        a4 = ?arg4,
        "Intercepted PS4 Syscall"
    );

    match sys_id {
        1 => {
            // sys_exit
            tracing::info!("Guest called sys_exit. Halting execution neatly.");
            std::process::exit(0);
        }
        _ => {
            tracing::warn!(sys_id, "Unhandled syscall, returning 0 (Fake OK)");
            0
        }
    }
}

/// Linux Signal Handler for SIGILL.
extern "C" fn sigill_handler(_sig: c_int, _info: *mut siginfo_t, ucontext: *mut libc::c_void) {
    unsafe {
        // --- CRITICAL: Restore Host TLS via raw syscalls! ---
        let mut guest_fs: u64 = 0;
        let p_guest_fs = &mut guest_fs as *mut u64;
        std::arch::asm!(
            "syscall",
            in("rax") libc::SYS_arch_prctl,
            in("rdi") 0x1003, // ARCH_GET_FS
            in("rsi") p_guest_fs,
            out("rcx") _, out("r11") _,
            options(nostack, preserves_flags)
        );
        
        let host_fs = get_thread_fs();
        
        // If we can't find host FS for this thread, we CANNOT use Rust/tracing.
        // Handle UD2 minimally: just advance RIP and return 0.
        if host_fs == 0 {
            let uc = ucontext as *mut ucontext_t;
            let mcontext = &mut (*uc).uc_mcontext;
            let rip = mcontext.gregs[libc::REG_RIP as usize] as *const u8;
            if *rip == 0x0F && *rip.add(1) == 0x0B {
                // UD2 from a stub - emulate ret with RAX=0
                if crate::hle::stubs::is_stub(rip as u64) {
                    let rsp = mcontext.gregs[libc::REG_RSP as usize] as *const u64;
                    let return_addr = *rsp;
                    mcontext.gregs[libc::REG_RSP as usize] += 8;
                    mcontext.gregs[libc::REG_RIP as usize] = return_addr as i64;
                    mcontext.gregs[libc::REG_RAX as usize] = 0;
                } else {
                    // Patched syscall - advance past UD2
                    mcontext.gregs[libc::REG_RIP as usize] += 2;
                    mcontext.gregs[libc::REG_RAX as usize] = 0;
                }
                // Restore guest FS
                if guest_fs != 0 {
                    std::arch::asm!(
                        "syscall",
                        in("rax") libc::SYS_arch_prctl,
                        in("rdi") 0x1002,
                        in("rsi") guest_fs,
                        out("rcx") _, out("r11") _,
                        options(nostack, preserves_flags)
                    );
                }
                return;
            }
            std::process::abort();
        }
        
        // Restore host TLS for Rust operations
        std::arch::asm!(
            "syscall",
            in("rax") libc::SYS_arch_prctl,
            in("rdi") 0x1002, // ARCH_SET_FS
            in("rsi") host_fs,
            out("rcx") _, out("r11") _,
            options(nostack, preserves_flags)
        );

        let uc = ucontext as *mut ucontext_t;
        // On x86_64 Linux, CPU registers are inside uc_mcontext
        let mcontext = &mut (*uc).uc_mcontext;
        
        // Offset 16 is RIP (REG_RIP)
        let rip = mcontext.gregs[libc::REG_RIP as usize] as *const u8;

        // Check if the faulting instruction is our UD2 (0x0F 0x0B)
        if *rip == 0x0F && *rip.add(1) == 0x0B {
            let rip_u64 = rip as u64;

            if crate::hle::stubs::is_stub(rip_u64) {
                // It's an HLE stub trampoline!
                let stub_idx = crate::hle::stubs::get_stub_index(rip_u64).unwrap_or(0);
                let stub_name = crate::hle::stubs::get_stub_name(stub_idx);
                
                // Extract arguments according to System V AMD64 ABI (functions, not syscalls)
                let rdi = mcontext.gregs[libc::REG_RDI as usize] as u64; // Arg 1
                let rsi = mcontext.gregs[libc::REG_RSI as usize] as u64; // Arg 2
                let rdx = mcontext.gregs[libc::REG_RDX as usize] as u64; // Arg 3
                let rcx = mcontext.gregs[libc::REG_RCX as usize] as u64; // Arg 4
                let r8  = mcontext.gregs[libc::REG_R8  as usize] as u64; // Arg 5
                let r9  = mcontext.gregs[libc::REG_R9  as usize] as u64; // Arg 6
                // Arg 7+ live on the stack. At the UD2 trap RSP+0 = return addr,
                // RSP+8 = 7th argument (first stack-passed arg in SysV AMD64).
                let rsp_ptr = mcontext.gregs[libc::REG_RSP as usize] as *const u64;
                let stack_arg7 = if !rsp_ptr.is_null() { *rsp_ptr.add(1) } else { 0 };

                // Extract XMM0, XMM1 for float arguments (math functions use SSE)
                let (xmm0_bits, xmm1_bits): (u64, u64) = if !mcontext.fpregs.is_null() {
                    let fpregs = &*mcontext.fpregs;
                    // _xmm is an array of _libc_xmmreg, each 16 bytes.
                    let xmm0_ptr = &fpregs._xmm[0] as *const _ as *const u64;
                    let xmm1_ptr = &fpregs._xmm[1] as *const _ as *const u64;
                    (*xmm0_ptr, *xmm1_ptr)
                } else {
                    (0, 0)
                };

                // Dispatch to exports handler!
                let result = crate::hle::exports::hle_export_dispatcher(&stub_name, rdi, rsi, rdx, rcx, r8, r9, stack_arg7, xmm0_bits, xmm1_bits);

                // For float-returning functions, write result to XMM0
                if crate::hle::exports::is_float_function(&stub_name) {
                    if !mcontext.fpregs.is_null() {
                        let fpregs = &mut *mcontext.fpregs;
                        let xmm0_ptr = &mut fpregs._xmm[0] as *mut _ as *mut u64;
                        *xmm0_ptr = result;
                        // Clear high bits
                        *(xmm0_ptr.add(1)) = 0;
                    }
                    // Also set RAX to 0 for safety
                    (*uc).uc_mcontext.gregs[libc::REG_RAX as usize] = 0;
                } else {
                    // Set return value in RAX
                    (*uc).uc_mcontext.gregs[libc::REG_RAX as usize] = result as i64;
                }

                // Advance RIP by 2 bytes (length of UD2) so we don't trap again,
                // BUT wait: the stub should `ret`! If we just advance RIP by 2,
                // it will continue executing NOPs in the stub!
                // We actually want to emulate the `ret` instruction.
                // A `ret` pops the target address from the stack into RIP.
                let rsp = mcontext.gregs[libc::REG_RSP as usize] as *const u64;
                let return_addr = *rsp;
                
                // Emulate `ret`
                (*uc).uc_mcontext.gregs[libc::REG_RSP as usize] += 8;
                (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] = return_addr as i64;
            } else {
                // It's our patched SYSCALL!
                
                // Extract arguments according to PS4 SYSCALL ABI
                let rax = mcontext.gregs[libc::REG_RAX as usize] as u64; // Syscall Number
                let rdi = mcontext.gregs[libc::REG_RDI as usize] as u64; // Arg 1
                let rsi = mcontext.gregs[libc::REG_RSI as usize] as u64; // Arg 2
                let rdx = mcontext.gregs[libc::REG_RDX as usize] as u64; // Arg 3
                let r10 = mcontext.gregs[libc::REG_R10 as usize] as u64; // Arg 4 (Note: R10 instead of RCX)
                let r8  = mcontext.gregs[libc::REG_R8  as usize] as u64; // Arg 5
                let r9  = mcontext.gregs[libc::REG_R9  as usize] as u64; // Arg 6

                // Dispatch
                let result = hle_syscall_dispatcher(rax, rdi, rsi, rdx, r10, r8, r9);

                // Set return value in RAX
                (*uc).uc_mcontext.gregs[libc::REG_RAX as usize] = result as i64;

                // Advance RIP by 2 bytes (length of UD2) so we don't trap again
                (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] += 2;
            }
        } else {
            // A real SIGILL occurred. Crash loudly.
            tracing::error!(rip = format!("{:?}", rip), "Real SIGILL encountered. Aborting.");
            // Restore Guest TLS just in case, though we are aborting
            std::process::abort();
        }

        // --- CRITICAL: Restore Guest TLS before returning to guest! ---
        if guest_fs != 0 {
            libc::syscall(libc::SYS_arch_prctl, 0x1002, guest_fs);
        }
    }
}

/// Maximum number of SIGSEGV recoveries before we give up.
static SIGSEGV_RECOVERY_COUNT: AtomicU64 = AtomicU64::new(0);
const MAX_SIGSEGV_RECOVERIES: u64 = 512;

/// Linux Signal Handler for SIGSEGV (guest memory faults).
/// Attempts to recover from NULL pointer dereferences in guest code by
/// simulating a `ret` (popping return address from stack, setting RAX=0).
extern "C" fn sigsegv_handler(_sig: c_int, info: *mut siginfo_t, ucontext: *mut libc::c_void) {
    unsafe {
        let mut guest_fs: u64 = 0;
        let p_guest_fs = &mut guest_fs as *mut u64;
        std::arch::asm!(
            "syscall",
            in("rax") libc::SYS_arch_prctl,
            in("rdi") 0x1003, // ARCH_GET_FS
            in("rsi") p_guest_fs,
            out("rcx") _, out("r11") _,
            options(nostack, preserves_flags)
        );

        let host_fs = get_thread_fs();
        if host_fs != 0 {
            std::arch::asm!(
                "syscall",
                in("rax") libc::SYS_arch_prctl,
                in("rdi") 0x1002, // ARCH_SET_FS
                in("rsi") host_fs,
                out("rcx") _, out("r11") _,
                options(nostack, preserves_flags)
            );
        }

        let uc = ucontext as *mut ucontext_t;
        let mctx = &mut (*uc).uc_mcontext;
        let rip = mctx.gregs[libc::REG_RIP as usize] as u64;
        let rsp = mctx.gregs[libc::REG_RSP as usize] as u64;
        let fault_addr = (*info).si_addr() as u64;

        // ── Recovery: attempt to survive guest crashes ──
        // For playable emulation, we try to recover from ANY SIGSEGV by
        // unwinding the stack frame and returning 0 to the caller.
        // This lets the game progress past unimplemented subsystems.
        {
            let count = SIGSEGV_RECOVERY_COUNT.fetch_add(1, Ordering::SeqCst);
            if count < MAX_SIGSEGV_RECOVERIES {
                let rbp = mctx.gregs[libc::REG_RBP as usize] as u64;

                // Guest code range check
                let guest_base: u64 = 0x1000000000;
                let guest_end: u64 = 0x1001000000; // 16MB module size

                // Try RBP-based frame unwinding to find a valid return address
                // Frame layout: [RBP] -> saved_rbp, [RBP+8] -> return_addr
                let mut found_ret = 0u64;
                let mut found_rbp = 0u64;
                let mut frame_rbp = rbp;

                for _depth in 0..16 {
                    if frame_rbp == 0 || frame_rbp < 0x1000 {
                        break;
                    }
                    let saved_rbp = *(frame_rbp as *const u64);
                    let ret_addr = *((frame_rbp + 8) as *const u64);

                    if ret_addr >= guest_base && ret_addr < guest_end {
                        found_ret = ret_addr;
                        found_rbp = saved_rbp;
                        break;
                    }
                    frame_rbp = saved_rbp;
                }

                // Fallback: if no valid frame found, try RSP directly
                if found_ret == 0 {
                    let stack_ret = *(rsp as *const u64);
                    if stack_ret >= guest_base && stack_ret < guest_end {
                        found_ret = stack_ret;
                        found_rbp = rbp;
                    }
                }

                if found_ret != 0 {
                    tracing::warn!(
                        rip = format!("0x{:X}", rip),
                        fault = format!("0x{:X}", fault_addr),
                        ret_to = format!("0x{:X}", found_ret),
                        recovery = count + 1,
                        "SIGSEGV recovery: NULL deref → unwinding to caller with RAX=0"
                    );

                    // Set RIP to the recovered return address
                    mctx.gregs[libc::REG_RIP as usize] = found_ret as i64;
                    // Restore RBP to the frame's saved RBP
                    mctx.gregs[libc::REG_RBP as usize] = found_rbp as i64;
                    // Set RSP past the frame (RBP+16 skips saved_rbp and ret_addr)
                    mctx.gregs[libc::REG_RSP as usize] = (frame_rbp + 16) as i64;
                    mctx.gregs[libc::REG_RAX as usize] = 0; // Return 0

                    // Restore guest FS before returning to guest code
                    if guest_fs != 0 {
                        std::arch::asm!(
                            "syscall",
                            in("rax") libc::SYS_arch_prctl,
                            in("rdi") 0x1002, // ARCH_SET_FS
                            in("rsi") guest_fs,
                            out("rcx") _, out("r11") _,
                            options(nostack, preserves_flags)
                        );
                    }

                    return; // Resume execution at found_ret
                }

                tracing::error!(
                    rip = format!("0x{:X}", rip),
                    fault = format!("0x{:X}", fault_addr),
                    "SIGSEGV recovery FAILED: no valid return address found in stack frames"
                );
            }
        }

        // ── Fatal: dump registers and abort ──
        let rax = mctx.gregs[libc::REG_RAX as usize] as u64;
        let rbx = mctx.gregs[libc::REG_RBX as usize] as u64;
        let rcx = mctx.gregs[libc::REG_RCX as usize] as u64;
        let rdx = mctx.gregs[libc::REG_RDX as usize] as u64;
        let rdi = mctx.gregs[libc::REG_RDI as usize] as u64;
        let rsi = mctx.gregs[libc::REG_RSI as usize] as u64;
        let rbp = mctx.gregs[libc::REG_RBP as usize] as u64;
        let r8  = mctx.gregs[libc::REG_R8  as usize] as u64;
        let r9  = mctx.gregs[libc::REG_R9  as usize] as u64;
        let r10 = mctx.gregs[libc::REG_R10 as usize] as u64;
        let r11 = mctx.gregs[libc::REG_R11 as usize] as u64;
        let r12 = mctx.gregs[libc::REG_R12 as usize] as u64;
        let r13 = mctx.gregs[libc::REG_R13 as usize] as u64;
        let r14 = mctx.gregs[libc::REG_R14 as usize] as u64;
        let r15 = mctx.gregs[libc::REG_R15 as usize] as u64;

        eprintln!("╔══════════════════════════════════════════════════╗");
        eprintln!("║  FATAL GUEST SEGMENTATION FAULT (SIGSEGV)       ║");
        eprintln!("╠══════════════════════════════════════════════════╣");
        eprintln!("║ RIP: 0x{:016X}                    ║", rip);
        eprintln!("║ RSP: 0x{:016X}                    ║", rsp);
        eprintln!("║ RBP: 0x{:016X}                    ║", rbp);
        eprintln!("║ Fault Addr: 0x{:016X}              ║", fault_addr);
        eprintln!("╠══════════════════════════════════════════════════╣");
        eprintln!("║ RAX: 0x{:016X}  RBX: 0x{:016X} ║", rax, rbx);
        eprintln!("║ RCX: 0x{:016X}  RDX: 0x{:016X} ║", rcx, rdx);
        eprintln!("║ RDI: 0x{:016X}  RSI: 0x{:016X} ║", rdi, rsi);
        eprintln!("║ R8 : 0x{:016X}  R9 : 0x{:016X} ║", r8, r9);
        eprintln!("║ R10: 0x{:016X}  R11: 0x{:016X} ║", r10, r11);
        eprintln!("║ R12: 0x{:016X}  R13: 0x{:016X} ║", r12, r13);
        eprintln!("║ R14: 0x{:016X}  R15: 0x{:016X} ║", r14, r15);
        eprintln!("╚══════════════════════════════════════════════════╝");

        // Read bytes at and BEFORE RIP to understand the crash context
        let rip_ptr = rip as *const u8;
        if rip != 0 {
            // Bytes BEFORE the crash (the instruction that loaded the bad address)
            let mut pre_bytes = String::new();
            for i in (0..32u64).rev() {
                let ptr = rip_ptr.sub(32 - i as usize);
                pre_bytes.push_str(&format!("{:02X} ", *ptr));
            }
            eprintln!("Bytes BEFORE RIP (-32): {}", pre_bytes);

            let mut bytes_str = String::new();
            for i in 0..16u64 {
                let ptr = rip_ptr.add(i as usize);
                bytes_str.push_str(&format!("{:02X} ", *ptr));
            }
            eprintln!("Bytes at RIP:          {}", bytes_str);

            // Dump stack (return addresses)
            eprintln!("Stack (8 entries from RSP):");
            let stack_ptr = rsp as *const u64;
            for i in 0..8 {
                let val = *stack_ptr.add(i);
                eprintln!("  [RSP+0x{:02X}] = 0x{:016X}", i * 8, val);
            }
        }
        
        std::process::abort();
    }
}

/// Initializes the SIGILL and SIGSEGV signal handlers globally. Safe to call multiple times.
pub fn initialize_syscall_interceptor() {
    INIT_SIGNAL.call_once(|| {
        unsafe {
            // SIGILL handler (for UD2 syscall traps)
            let mut sa: sigaction = std::mem::zeroed();
            sa.sa_sigaction = sigill_handler as usize;
            sa.sa_flags = SA_SIGINFO | libc::SA_NODEFER;
            libc::sigemptyset(&mut sa.sa_mask);

            if libc::sigaction(SIGILL, &sa, std::ptr::null_mut()) != 0 {
                panic!("Failed to register SIGILL signal handler for syscall interception");
            }

            // SIGSEGV handler (for guest crash diagnostics)
            let mut sa2: sigaction = std::mem::zeroed();
            sa2.sa_sigaction = sigsegv_handler as usize;
            sa2.sa_flags = SA_SIGINFO | libc::SA_NODEFER;
            libc::sigemptyset(&mut sa2.sa_mask);

            if libc::sigaction(libc::SIGSEGV, &sa2, std::ptr::null_mut()) != 0 {
                panic!("Failed to register SIGSEGV signal handler");
            }

            tracing::info!("SIGILL Syscall Interceptor initialized");
        }
    });
}

/// Patch all `0x0F 0x05` (SYSCALL) instructions inside the given executable memory region
/// with `0x0F 0x0B` (UD2) to trigger SIGILL.
pub unsafe fn patch_syscalls(memory_base: *mut u8, size: usize) -> u32 {
    let mut patched_count = 0;
    
    // The memory MUST be mapped with PROT_WRITE before calling this,
    // which our ELF loader currently does (READ | WRITE | EXEC).
    let mut offset = 0;
    while offset < size - 1 {
        unsafe {
            let b1 = *memory_base.add(offset);
            let b2 = *memory_base.add(offset + 1);
            
            if b1 == 0x0F && b2 == 0x05 { // SYSCALL
                // Overwrite with UD2
                *memory_base.add(offset + 1) = 0x0B;
                patched_count += 1;
            }
        }
        offset += 1;
    }
    
    patched_count
}
