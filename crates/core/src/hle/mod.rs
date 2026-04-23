//! HLE (High-Level Emulation) library system.
//!
//! Provides reimplementations of PS4 system libraries. When a game calls
//! a system function, the linker redirects the call to our HLE implementation
//! instead of running the original Sony library code.

pub mod dispatcher;
pub mod exports;
pub mod libkernel;
pub mod libraries;
pub mod stubs;
