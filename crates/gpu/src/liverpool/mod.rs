//! Liverpool GPU emulation — the PS4's custom AMD GCN 1.1 GPU.
//!
//! Named "Liverpool" by AMD/Sony, this GPU has:
//! - 18 Compute Units (1152 stream processors)
//! - 1.84 TFLOPS peak
//! - 64 Async Compute Engines (ACE) hardware queues
//! - GCN 1.1 ISA for shaders
//! - PM4 packet-based command submission

pub mod command_processor;
pub mod pm4;
pub mod regs;
