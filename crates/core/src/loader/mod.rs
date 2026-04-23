//! PS4 ELF/SELF loader.
//!
//! PS4 executables are ELF64 files with Sony-specific extensions:
//! - **SELF** (Signed ELF): encrypted wrapper around the ELF, with magic `0x1D3D154F`
//! - **SCE program headers**: custom types like `PT_SCE_DYNLIBDATA`, `PT_SCE_PROCPARAM`
//! - **SCE dynamic tags**: custom dynamic linking tags (`DT_SCE_*`)
//! - **NID-based symbols**: symbols are identified by encoded Name IDs
//!
//! For emulation purposes, we work with **decrypted** SELF/ELF files (as dumped
//! from a jailbroken PS4). Encryption is not handled here.

pub mod elf;
pub mod symbols;
