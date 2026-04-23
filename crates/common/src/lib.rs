//! Common types and utilities for AnotherPS4 emulator.
//!
//! This crate provides fundamental type definitions, logging infrastructure,
//! and shared utilities used across all emulator subsystems.

pub mod logging;
pub mod types;

// Re-exports for convenience
pub use bitflags;
pub use parking_lot;
pub use rustc_hash;
pub use types::*;
