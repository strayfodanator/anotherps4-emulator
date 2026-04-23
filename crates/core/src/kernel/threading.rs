//! PS4 threading HLE.
//!
//! Implements PS4 pthread-like threading primitives. The PS4 uses a 1:1
//! kernel threading model inherited from FreeBSD, where each user thread
//! corresponds directly to a kernel thread.

use std::thread;
use parking_lot::Mutex;
use super::OrbisError;

/// Opaque thread handle.
pub type OrbisThread = u64;

/// Thread attributes.
#[derive(Debug, Clone)]
pub struct ThreadAttr {
    /// Stack size in bytes (default: 2MB).
    pub stack_size: usize,
    /// Thread priority (0-767, default: 700).
    pub priority: i32,
    /// CPU affinity mask.
    pub affinity_mask: u64,
    /// Thread name.
    pub name: String,
}

impl Default for ThreadAttr {
    fn default() -> Self {
        ThreadAttr {
            stack_size: 2 * 1024 * 1024, // 2 MB
            priority: 700,
            affinity_mask: 0xFF, // All 8 cores
            name: String::from("OrbisThread"),
        }
    }
}

/// Thread manager tracking all guest threads.
pub struct ThreadManager {
    /// Next thread ID to assign.
    next_id: Mutex<u64>,
    /// Active thread handles.
    threads: Mutex<Vec<ThreadInfo>>,
}

/// Info about a tracked thread.
struct ThreadInfo {
    id: u64,
    name: String,
    handle: Option<thread::JoinHandle<()>>,
}

impl ThreadManager {
    pub fn new() -> Self {
        ThreadManager {
            next_id: Mutex::new(1),
            threads: Mutex::new(Vec::new()),
        }
    }

    /// Create a new guest thread.
    pub fn create_thread(
        &self,
        attr: &ThreadAttr,
        entry: u64,
        _arg: u64,
    ) -> Result<OrbisThread, i32> {
        let mut id_lock = self.next_id.lock();
        let thread_id = *id_lock;
        *id_lock += 1;
        drop(id_lock);

        let name = attr.name.clone();
        tracing::info!(
            id = thread_id,
            name = %name,
            stack_size = attr.stack_size,
            entry = format!("0x{:X}", entry),
            "Creating guest thread"
        );

        // For now, we just track the thread. Actual execution of guest code
        // will require jumping to the entry point with proper TLS setup.
        let mut threads = self.threads.lock();
        threads.push(ThreadInfo {
            id: thread_id,
            name,
            handle: None,
        });

        Ok(thread_id)
    }

    /// Get the number of active threads.
    pub fn thread_count(&self) -> usize {
        self.threads.lock().len()
    }
}

impl Default for ThreadManager {
    fn default() -> Self {
        Self::new()
    }
}

/// `scePthreadCreate` — Create a new thread.
pub fn sce_pthread_create(
    manager: &ThreadManager,
    attr: &ThreadAttr,
    entry: u64,
    arg: u64,
    thread_out: &mut OrbisThread,
) -> i32 {
    match manager.create_thread(attr, entry, arg) {
        Ok(id) => {
            *thread_out = id;
            OrbisError::OK.into()
        }
        Err(e) => e,
    }
}

/// `scePthreadSelf` — Get the current thread ID.
pub fn sce_pthread_self() -> OrbisThread {
    // Map the host thread ID to a PS4 thread handle
    let id = thread::current().id();
    // Use the thread ID hash as a pseudo-handle
    let hash = format!("{:?}", id);
    hash.len() as u64
}

/// `sceKernelGetCurrentCpu` — Get the current CPU core index.
pub fn sce_kernel_get_current_cpu() -> i32 {
    // Use libc sched_getcpu on Linux
    #[cfg(target_os = "linux")]
    unsafe {
        libc::sched_getcpu()
    }
    #[cfg(not(target_os = "linux"))]
    0
}
