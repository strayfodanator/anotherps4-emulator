//! PS4 kernel HLE (High-Level Emulation).
//!
//! Reimplements the Orbis OS kernel syscalls and services. The PS4 kernel
//! is based on FreeBSD 9.0 with Sony-specific extensions.
//!
//! Instead of emulating the kernel, we intercept system calls and handle
//! them with native host OS operations.

pub mod equeue;
pub mod filesystem;
pub mod memory_syscalls;
pub mod threading;
pub mod time;



/// PS4/Orbis error codes.
///
/// These are the standard error codes returned by PS4 kernel functions.
/// They are negative 32-bit values with the format 0x8002XXXX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrbisError(pub i32);

impl OrbisError {
    pub const OK: Self = Self(0);
    pub const EPERM: Self = Self(-0x7FFE_0001_i32);      // 0x80020001
    pub const ENOENT: Self = Self(-0x7FFE_0002_i32);     // 0x80020002
    pub const ESRCH: Self = Self(-0x7FFE_0003_i32);      // 0x80020003
    pub const EINTR: Self = Self(-0x7FFE_0004_i32);      // 0x80020004
    pub const EBADF: Self = Self(-0x7FFE_0009_i32);      // 0x80020009
    pub const EDEADLK: Self = Self(-0x7FFE_000B_i32);    // 0x8002000B
    pub const ENOMEM: Self = Self(-0x7FFE_000C_i32);     // 0x8002000C
    pub const EACCES: Self = Self(-0x7FFE_000D_i32);     // 0x8002000D
    pub const EBUSY: Self = Self(-0x7FFE_0010_i32);      // 0x80020010
    pub const EINVAL: Self = Self(-0x7FFE_0016_i32);     // 0x80020016
    pub const ETIMEDOUT: Self = Self(-0x7FFE_003C_i32);  // 0x8002003C
    pub const ENOSYS: Self = Self(-0x7FFE_004E_i32);     // 0x8002004E
}

impl From<OrbisError> for i32 {
    fn from(e: OrbisError) -> i32 {
        e.0
    }
}

/// POSIX error codes (positive, used in some syscalls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosixError(pub i32);

impl PosixError {
    pub const OK: Self = Self(0);
    pub const EPERM: Self = Self(1);
    pub const ENOENT: Self = Self(2);
    pub const ESRCH: Self = Self(3);
    pub const EINTR: Self = Self(4);
    pub const EBADF: Self = Self(9);
    pub const ENOMEM: Self = Self(12);
    pub const EACCES: Self = Self(13);
    pub const EBUSY: Self = Self(16);
    pub const EINVAL: Self = Self(22);
    pub const ETIMEDOUT: Self = Self(60);
    pub const ENOSYS: Self = Self(78);
}
