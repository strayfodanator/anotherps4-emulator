//! PS4 time-related kernel functions.
//!
//! Provides high-resolution timing using the host's TSC or
//! `clock_gettime` for accurate guest time emulation.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Global start time for process time calculation.
static mut PROCESS_START: Option<Instant> = None;

/// Initialize the time subsystem.
pub fn init() {
    unsafe {
        PROCESS_START = Some(Instant::now());
    }
    tracing::debug!("Time subsystem initialized");
}

/// `sceKernelGetTscFrequency` — Get the timestamp counter frequency.
///
/// The PS4 returns the actual CPU TSC frequency. We return a standard
/// value that matches typical AMD Jaguar behavior.
pub fn sce_kernel_get_tsc_frequency() -> u64 {
    // PS4 Jaguar runs at 1.6 GHz
    1_600_000_000
}

/// `sceKernelGetProcessTime` — Get time since process start in microseconds.
pub fn sce_kernel_get_process_time() -> u64 {
    let start = unsafe { PROCESS_START.unwrap_or_else(Instant::now) };
    start.elapsed().as_micros() as u64
}

/// `sceKernelGetProcessTimeCounter` — High-resolution process time counter.
pub fn sce_kernel_get_process_time_counter() -> u64 {
    let start = unsafe { PROCESS_START.unwrap_or_else(Instant::now) };
    start.elapsed().as_nanos() as u64
}

/// `sceKernelGetProcessTimeCounterFrequency` — Frequency of the process time counter.
pub fn sce_kernel_get_process_time_counter_frequency() -> u64 {
    1_000_000_000 // nanoseconds
}

/// `sceKernelClockGettime` — POSIX clock_gettime.
pub fn sce_kernel_clock_gettime(clock_id: i32, sec_out: &mut i64, nsec_out: &mut i64) -> i32 {
    match clock_id {
        0 => {
            // CLOCK_REALTIME
            let duration = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            *sec_out = duration.as_secs() as i64;
            *nsec_out = duration.subsec_nanos() as i64;
            0
        }
        4 => {
            // CLOCK_MONOTONIC
            let elapsed = unsafe {
                PROCESS_START.unwrap_or_else(Instant::now).elapsed()
            };
            *sec_out = elapsed.as_secs() as i64;
            *nsec_out = elapsed.subsec_nanos() as i64;
            0
        }
        _ => {
            tracing::warn!(clock_id, "Unknown clock ID");
            -1
        }
    }
}

/// `sceKernelUsleep` — Sleep for microseconds.
pub fn sce_kernel_usleep(microseconds: u32) -> i32 {
    std::thread::sleep(std::time::Duration::from_micros(microseconds as u64));
    0
}

/// `sceKernelSleep` — Sleep for seconds.
pub fn sce_kernel_sleep(seconds: u32) -> i32 {
    std::thread::sleep(std::time::Duration::from_secs(seconds as u64));
    0
}

/// `sceKernelGettimeofday` — POSIX gettimeofday.
pub fn sce_kernel_gettimeofday(sec_out: &mut i64, usec_out: &mut i64) -> i32 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    *sec_out = duration.as_secs() as i64;
    *usec_out = duration.subsec_micros() as i64;
    0
}
