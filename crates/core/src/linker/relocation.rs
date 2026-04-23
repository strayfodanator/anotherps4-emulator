//! x86-64 relocation processing for PS4 ELF modules.
//!
//! Applies relocations to loaded modules, resolving symbols against
//! the HLE symbol table and other loaded modules.

use crate::loader::elf;
use crate::loader::symbols::SymbolResolver;
use crate::linker::module::{Module, ParsedRelocation};
use crate::hle::stubs;
use anotherps4_common::VAddr;

/// Apply a single relocation to a loaded module.
pub fn apply_relocation(
    reloc: &ParsedRelocation,
    base: VAddr,
    hle_symbols: &SymbolResolver,
    _modules: &[Module],
) {
    let target_addr = base + reloc.offset;
    let rel_type = reloc.rel_type;

    match rel_type {
        elf::R_X86_64_RELATIVE => {
            // Base + Addend
            let value = base.wrapping_add(reloc.addend as u64);
            unsafe {
                *(target_addr as *mut u64) = value;
            }
        }
        elf::R_X86_64_64 | elf::R_X86_64_GLOB_DAT | elf::R_X86_64_JUMP_SLOT => {
            // Resolve Symbol
            let mut resolved_addr: u64 = 0;
            
            if !reloc.symbol_name.is_empty() {
                // Try NID lookup (first part before #)
                let base_name = reloc.symbol_name.split('#').next().unwrap_or(&reloc.symbol_name);
                
                if let Some(addr) = hle_symbols.resolve(base_name) {
                    resolved_addr = addr;
                } else if let Some(addr) = hle_symbols.resolve(&reloc.symbol_name) {
                    resolved_addr = addr;
                } else {
                    // Symbol not found in HLE — allocate a generic stub
                    // so the game doesn't crash when calling this import
                    let stub_name = if let Some(known) = crate::loader::symbols::lookup_known_nid(base_name) {
                        known.to_string()
                    } else {
                        // Let's print the bytes of base_name to see why it fails the hardcoded match
                        tracing::debug!(
                            base_name,
                            bytes = ?base_name.as_bytes(),
                            "lookup_known_nid returned None!"
                        );
                        reloc.symbol_name.clone()
                    };
                    resolved_addr = stubs::allocate_stub(stub_name.clone());
                    tracing::debug!(
                        name = %stub_name,
                        stub = format!("0x{:X}", resolved_addr),
                        "Unresolved import → stub"
                    );
                }
            }

            let value = resolved_addr.wrapping_add(reloc.addend as u64);
            unsafe {
                *(target_addr as *mut u64) = value;
            }
        }
        elf::R_X86_64_DTPMOD64 => {
            // TLS module index
            // FIXME: Hardcode index to 1 for now
            let value = 1u64;
            unsafe {
                *(target_addr as *mut u64) = value;
            }
        }
        _ => {
            tracing::warn!(
                rel_type,
                offset = format!("0x{:X}", reloc.offset),
                "Unknown relocation type"
            );
        }
    }
}
