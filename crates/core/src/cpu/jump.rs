//! Native CPU Execution Jumper.
//!
//! Because our emulator runs natively on x86_64, "emulating" the CPU is just
//! pointing the Instruction Pointer (RIP) to the mapped PS4 executable.
//! The PS4 expects a specific stack layout (System V AMD64 ABI adapted).

use std::arch::global_asm;

global_asm!(
    r#"
    .global jump_to_guest
    jump_to_guest:
        // function signature: 
        // extern "C" fn jump_to_guest(entry_point: u64 (rdi), stack_pointer: u64 (rsi))

        // Save host callee-saved registers
        push rbp
        push rbx
        push r12
        push r13
        push r14
        push r15

        // Save the host stack pointer safely somewhere so we can restore on panic/exit
        // We'll store it on the caller's stack frame context, but since we swap RSP,
        // we must save it in a global or return it somehow.
        // Easiest is to save the host RSP in a thread-local or pass an address.
        // But for now, since we never expect the main game loop to "return", 
        // we just swap and launch!

        mov rbp, rsp       // Keep a frame pointer just in case
        mov rsp, rsi       // SET THE GUEST STACK

        // Note: The PS4 entry point does NOT return. If it exits, it will call sceKernelExitProcess (a syscall).
        // Since we intercept syscalls via binary patches (CALL instructions),
        // we handle the exit in the syscall dispatcher and never return here natively.

        // JUMP TO GUEST ENTRY POINT
        jmp rdi
    "#
);

global_asm!(
    r#"
    .global jump_to_guest_thread
    jump_to_guest_thread:
        // extern "C" fn jump_to_guest_thread(entry_point: u64 (rdi), stack_pointer: u64 (rsi), arg: u64 (rdx))

        push rbp
        push rbx
        push r12
        push r13
        push r14
        push r15

        mov rbp, rsp
        mov rsp, rsi       // SET THE GUEST STACK

        // Move arg from RDX to RDI so the guest gets it as the first argument
        mov rax, rdi       // temp save entry_point because we need to overwrite rdi
        mov rdi, rdx       // RDI = arg
        
        // Zero out standard registers to prevent guest from seeing host garbage pointers
        xor rbx, rbx
        xor rcx, rcx
        xor rdx, rdx
        xor rbp, rbp
        xor r8, r8
        xor r9, r9
        xor r10, r10
        xor r11, r11
        xor r12, r12
        xor r13, r13
        xor r14, r14
        xor r15, r15
        
        // JUMP TO GUEST ENTRY POINT
        jmp rax
    "#
);

unsafe extern "C" {
    /// Jumps into the guest executable memory, replacing the active stack.
    /// This function NEVER returns. Native thread exits will be handled via syscalls.
    pub fn jump_to_guest(entry_point: u64, stack_pointer: u64) -> !;

    /// Jumps into the guest thread start routine.
    pub fn jump_to_guest_thread(entry_point: u64, stack_pointer: u64, arg: u64) -> !;
}
