//! HLE implementation of libkernel.sprx.
//!
//! This is the most fundamental PS4 system library. It provides:
//! - Memory management (mmap, direct memory allocation)
//! - Threading (pthread equivalents)
//! - Synchronization (mutex, cond, rwlock, semaphore)
//! - Time functions (TSC, clock_gettime, sleep)
//! - Event queues (kqueue-like)
//! - File I/O (open, read, write, close)
//! - Process management

use crate::loader::symbols::SymbolResolver;
use super::libraries::register_stub;

/// Register all libkernel HLE symbols.
pub fn register(resolver: &mut SymbolResolver) {
    let lib = "libkernel";

    // === Memory Management ===
    register_stub(resolver, "sceKernelAllocateDirectMemory", "sceKernelAllocateDirectMemory", lib);
    register_stub(resolver, "sceKernelMapDirectMemory", "sceKernelMapDirectMemory", lib);
    register_stub(resolver, "sceKernelMapFlexibleMemory", "sceKernelMapFlexibleMemory", lib);
    register_stub(resolver, "sceKernelMunmap", "sceKernelMunmap", lib);
    register_stub(resolver, "sceKernelMprotect", "sceKernelMprotect", lib);
    register_stub(resolver, "sceKernelQueryMemoryProtection", "sceKernelQueryMemoryProtection", lib);
    register_stub(resolver, "sceKernelGetDirectMemorySize", "sceKernelGetDirectMemorySize", lib);
    register_stub(resolver, "sceKernelAvailableDirectMemorySize", "sceKernelAvailableDirectMemorySize", lib);
    register_stub(resolver, "sceKernelVirtualQuery", "sceKernelVirtualQuery", lib);
    register_stub(resolver, "sceKernelReserveVirtualRange", "sceKernelReserveVirtualRange", lib);
    register_stub(resolver, "sceKernelMapNamedFlexibleMemory", "sceKernelMapNamedFlexibleMemory", lib);
    register_stub(resolver, "sceKernelMapNamedDirectMemory", "sceKernelMapNamedDirectMemory", lib);

    // === Threading ===
    register_stub(resolver, "scePthreadCreate", "scePthreadCreate", lib);
    register_stub(resolver, "scePthreadJoin", "scePthreadJoin", lib);
    register_stub(resolver, "scePthreadDetach", "scePthreadDetach", lib);
    register_stub(resolver, "scePthreadSelf", "scePthreadSelf", lib);
    register_stub(resolver, "scePthreadExit", "scePthreadExit", lib);
    register_stub(resolver, "scePthreadSetaffinity", "scePthreadSetaffinity", lib);
    register_stub(resolver, "scePthreadGetaffinity", "scePthreadGetaffinity", lib);
    register_stub(resolver, "scePthreadSetprio", "scePthreadSetprio", lib);
    register_stub(resolver, "scePthreadAttrInit", "scePthreadAttrInit", lib);
    register_stub(resolver, "scePthreadAttrDestroy", "scePthreadAttrDestroy", lib);
    register_stub(resolver, "scePthreadAttrSetstacksize", "scePthreadAttrSetstacksize", lib);
    register_stub(resolver, "scePthreadAttrSetdetachstate", "scePthreadAttrSetdetachstate", lib);
    register_stub(resolver, "scePthreadRename", "scePthreadRename", lib);

    // === Synchronization ===
    register_stub(resolver, "scePthreadMutexInit", "scePthreadMutexInit", lib);
    register_stub(resolver, "scePthreadMutexDestroy", "scePthreadMutexDestroy", lib);
    register_stub(resolver, "scePthreadMutexLock", "scePthreadMutexLock", lib);
    register_stub(resolver, "scePthreadMutexUnlock", "scePthreadMutexUnlock", lib);
    register_stub(resolver, "scePthreadMutexTrylock", "scePthreadMutexTrylock", lib);
    register_stub(resolver, "scePthreadCondInit", "scePthreadCondInit", lib);
    register_stub(resolver, "scePthreadCondDestroy", "scePthreadCondDestroy", lib);
    register_stub(resolver, "scePthreadCondSignal", "scePthreadCondSignal", lib);
    register_stub(resolver, "scePthreadCondBroadcast", "scePthreadCondBroadcast", lib);
    register_stub(resolver, "scePthreadCondWait", "scePthreadCondWait", lib);
    register_stub(resolver, "scePthreadCondTimedwait", "scePthreadCondTimedwait", lib);
    register_stub(resolver, "scePthreadRwlockInit", "scePthreadRwlockInit", lib);
    register_stub(resolver, "scePthreadRwlockDestroy", "scePthreadRwlockDestroy", lib);
    register_stub(resolver, "scePthreadRwlockRdlock", "scePthreadRwlockRdlock", lib);
    register_stub(resolver, "scePthreadRwlockWrlock", "scePthreadRwlockWrlock", lib);
    register_stub(resolver, "scePthreadRwlockUnlock", "scePthreadRwlockUnlock", lib);
    register_stub(resolver, "sceKernelCreateSema", "sceKernelCreateSema", lib);
    register_stub(resolver, "sceKernelDeleteSema", "sceKernelDeleteSema", lib);
    register_stub(resolver, "sceKernelWaitSema", "sceKernelWaitSema", lib);
    register_stub(resolver, "sceKernelSignalSema", "sceKernelSignalSema", lib);

    // === Event Queues ===
    register_stub(resolver, "sceKernelCreateEqueue", "sceKernelCreateEqueue", lib);
    register_stub(resolver, "sceKernelDeleteEqueue", "sceKernelDeleteEqueue", lib);
    register_stub(resolver, "sceKernelAddUserEvent", "sceKernelAddUserEvent", lib);
    register_stub(resolver, "sceKernelAddReadEvent", "sceKernelAddReadEvent", lib);
    register_stub(resolver, "sceKernelWaitEqueue", "sceKernelWaitEqueue", lib);
    register_stub(resolver, "sceKernelTriggerUserEvent", "sceKernelTriggerUserEvent", lib);

    // === Time ===
    register_stub(resolver, "sceKernelGetTscFrequency", "sceKernelGetTscFrequency", lib);
    register_stub(resolver, "sceKernelGetProcessTime", "sceKernelGetProcessTime", lib);
    register_stub(resolver, "sceKernelGetProcessTimeCounter", "sceKernelGetProcessTimeCounter", lib);
    register_stub(resolver, "sceKernelGetProcessTimeCounterFrequency", "sceKernelGetProcessTimeCounterFrequency", lib);
    register_stub(resolver, "sceKernelClockGettime", "sceKernelClockGettime", lib);
    register_stub(resolver, "sceKernelGettimeofday", "sceKernelGettimeofday", lib);
    register_stub(resolver, "sceKernelUsleep", "sceKernelUsleep", lib);
    register_stub(resolver, "sceKernelSleep", "sceKernelSleep", lib);
    register_stub(resolver, "sceKernelNanosleep", "sceKernelNanosleep", lib);

    // === File System ===
    register_stub(resolver, "sceKernelOpen", "sceKernelOpen", lib);
    register_stub(resolver, "sceKernelClose", "sceKernelClose", lib);
    register_stub(resolver, "sceKernelRead", "sceKernelRead", lib);
    register_stub(resolver, "sceKernelWrite", "sceKernelWrite", lib);
    register_stub(resolver, "sceKernelLseek", "sceKernelLseek", lib);
    register_stub(resolver, "sceKernelStat", "sceKernelStat", lib);
    register_stub(resolver, "sceKernelFstat", "sceKernelFstat", lib);
    register_stub(resolver, "sceKernelMkdir", "sceKernelMkdir", lib);
    register_stub(resolver, "sceKernelGetdents", "sceKernelGetdents", lib);
    register_stub(resolver, "sceKernelCheckReachability", "sceKernelCheckReachability", lib);

    // === Process ===
    register_stub(resolver, "sceKernelGetCurrentCpu", "sceKernelGetCurrentCpu", lib);
    register_stub(resolver, "sceKernelGetProcessParam", "sceKernelGetProcessParam", lib);
    register_stub(resolver, "sceKernelGetCompiledSdkVersion", "sceKernelGetCompiledSdkVersion", lib);
    register_stub(resolver, "sceKernelGetCpuFrequency", "sceKernelGetCpuFrequency", lib);
    register_stub(resolver, "sceKernelGetCpuMode", "sceKernelGetCpuMode", lib);
    register_stub(resolver, "sceKernelIsNeoMode", "sceKernelIsNeoMode", lib);

    // === Module ===
    register_stub(resolver, "sceKernelLoadStartModule", "sceKernelLoadStartModule", lib);
    register_stub(resolver, "sceKernelDlsym", "sceKernelDlsym", lib);
    register_stub(resolver, "sceKernelGetModuleList", "sceKernelGetModuleList", lib);
    register_stub(resolver, "sceKernelGetModuleInfo", "sceKernelGetModuleInfo", lib);
    register_stub(resolver, "sceKernelGetModuleInfoByName", "sceKernelGetModuleInfoByName", lib);

    // === Debug ===
    register_stub(resolver, "sceKernelDebugOutText", "sceKernelDebugOutText", lib);
    register_stub(resolver, "sceKernelDebugRaiseException", "sceKernelDebugRaiseException", lib);

    // === POSIX compat (also exported by libkernel) ===
    register_stub(resolver, "pthread_create", "pthread_create", lib);
    register_stub(resolver, "pthread_join", "pthread_join", lib);
    register_stub(resolver, "pthread_mutex_init", "pthread_mutex_init", lib);
    register_stub(resolver, "pthread_mutex_lock", "pthread_mutex_lock", lib);
    register_stub(resolver, "pthread_mutex_unlock", "pthread_mutex_unlock", lib);
    register_stub(resolver, "pthread_mutex_destroy", "pthread_mutex_destroy", lib);
    register_stub(resolver, "pthread_cond_init", "pthread_cond_init", lib);
    register_stub(resolver, "pthread_cond_signal", "pthread_cond_signal", lib);
    register_stub(resolver, "pthread_cond_wait", "pthread_cond_wait", lib);
    register_stub(resolver, "pthread_cond_destroy", "pthread_cond_destroy", lib);
    register_stub(resolver, "pthread_self", "pthread_self", lib);

    // === libc functions (also exported) ===
    register_stub(resolver, "malloc", "malloc", lib);
    register_stub(resolver, "free", "free", lib);
    register_stub(resolver, "calloc", "calloc", lib);
    register_stub(resolver, "realloc", "realloc", lib);
    register_stub(resolver, "memalign", "memalign", lib);
    register_stub(resolver, "memcpy", "memcpy", lib);
    register_stub(resolver, "memset", "memset", lib);
    register_stub(resolver, "memmove", "memmove", lib);
    register_stub(resolver, "strlen", "strlen", lib);

    tracing::info!(symbols = resolver.len(), "libkernel HLE registered");
}
