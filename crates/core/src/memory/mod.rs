//! PS4 memory management system.
//!
//! The PS4 uses a unified memory model with 8GB GDDR5 accessible by both
//! CPU and GPU. The emulator maps this onto the host's virtual address space
//! using `mmap` on Linux.
//!
//! ## Address Space Layout
//!
//! ```text
//! 0x0000_0000_0000  ┌─────────────────┐
//!                   │ System Managed   │  512 MB — kernel allocations
//!                   ├─────────────────┤
//!                   │ System Reserved  │  ~3.2 GB — system libraries
//!                   ├─────────────────┤
//! 0x0002_0000_0000  │ User Space       │  128 GB — game code & data
//!                   │                  │
//!                   └─────────────────┘
//! ```
//!
//! The GPU can only address the lower 40 bits (1 TB).

pub mod address_space;
pub mod physical_memory;
pub mod virtual_memory;

pub use address_space::AddressSpace;
pub use physical_memory::{PhysicalMemoryArea, PhysicalMemoryManager, PhysicalMemoryType};
pub use virtual_memory::{MemoryManager, MemoryMapFlags, MemoryProt, VMAType, VirtualMemoryArea};
