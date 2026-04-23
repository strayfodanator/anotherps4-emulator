//! Logging infrastructure for AnotherPS4.
//!
//! Uses the `tracing` ecosystem for structured, leveled logging with
//! per-subsystem filtering via environment variable `RUST_LOG`.

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialize the global logging subscriber.
///
/// Log level is controlled by the `RUST_LOG` environment variable.
/// Default level is `info`. Examples:
/// - `RUST_LOG=debug` — all crates at debug
/// - `RUST_LOG=anotherps4_core=trace,anotherps4_gpu=debug` — per-crate
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .with_file(false)
            .with_line_number(false)
            .compact())
        .with(filter)
        .init();
}
