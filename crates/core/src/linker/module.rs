//! PS4 module loading into memory.
//!
//! Handles loading ELF segments into the virtual address space,
//! parsing dynamic information, and setting up the module structure.

use crate::loader::elf::{Elf, ElfRelocation, ElfSymbol};
use crate::loader::elf::sce_dynamic::*;
use crate::memory::{MemoryManager, MemoryMapFlags, MemoryProt, VMAType};
use anotherps4_common::*;
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;
use std::path::Path;

/// A relocation fully parsed with its target symbol name.
#[derive(Debug, Clone)]
pub struct ParsedRelocation {
    pub offset: u64,
    pub rel_type: u32,
    pub addend: i64,
    pub symbol_name: String,
    pub symbol_binding: u8,
    pub symbol_type: u8,
    pub is_jmp_rel: bool,
}

/// A loaded PS4 module (executable or shared library).
#[derive(Debug)]
pub struct Module {
    /// Module name (filename without path).
    pub name: String,
    /// Full file path.
    pub file: std::path::PathBuf,
    /// Parsed ELF data.
    pub elf: Elf,
    /// Base virtual address where the module is loaded.
    pub base_virtual_addr: VAddr,
    /// Total aligned size in memory.
    pub aligned_base_size: u64,
    /// Address of the process parameters segment.
    pub proc_param_addr: VAddr,
    /// Address of the EH frame header.
    pub eh_frame_hdr_addr: VAddr,
    /// Size of the EH frame header.
    pub eh_frame_hdr_size: u32,
    /// All relocations to apply.
    pub relocations: Vec<ParsedRelocation>,
    /// TLS module index.
    pub tls_index: u32,
    /// TLS image virtual address.
    pub tls_image_addr: VAddr,
    /// TLS image size.
    pub tls_image_size: u32,
    /// TLS full block size (including uninitialized).
    pub tls_size: u32,
    /// TLS alignment.
    pub tls_align: u32,
    /// Minimum address of the LOAD segments in the ELF (used for relative offset calculation).
    pub min_addr: u64,
    /// Address of the `.init_array` table.
    pub init_array_addr: u64,
    /// Size of the `.init_array` table in bytes.
    pub init_array_size: u64,
    /// Address of `DT_INIT` function, if any.
    pub init_addr: u64,
}

impl Module {
    /// Load a PS4 ELF module into memory.
    pub fn load(
        path: &Path,
        memory: &mut MemoryManager,
        max_tls_index: &mut u32,
    ) -> anyhow::Result<Self> {
        let elf = Elf::open(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        tracing::info!(name = %name, "Loading module into memory");

        // Calculate total memory size from LOAD segments
        let mut min_addr = u64::MAX;
        let mut max_addr = 0u64;

        for ph in elf.load_segments() {
            let seg_start = ph.vaddr;
            let seg_end = ph.vaddr + ph.memsz;
            min_addr = min_addr.min(seg_start);
            max_addr = max_addr.max(seg_end);
        }

        if min_addr == u64::MAX {
            anyhow::bail!("No LOAD segments found in {}", name);
        }

        let aligned_base_size = align_up(max_addr - min_addr, PAGE_SIZE);

        // Map the entire module region
        let base_virtual_addr = memory.map_memory(
            0, // Let the memory manager choose an address
            aligned_base_size,
            MemoryProt::CPU_READ | MemoryProt::CPU_WRITE | MemoryProt::CPU_EXEC,
            MemoryMapFlags::ANONYMOUS | MemoryMapFlags::PRIVATE,
            VMAType::Code,
            &name,
        )?;

        tracing::info!(
            name = %name,
            base = format!("0x{:X}", base_virtual_addr),
            size = format_size(aligned_base_size),
            "Module memory allocated"
        );

        // Copy LOAD segment data into memory
        // We need the original program header indices to resolve SELF segment offsets correctly.
        for (ph_idx, ph) in elf.program_headers().iter().enumerate() {
            if ph.phdr_type != crate::loader::elf::ProgramType::Load {
                continue;
            }

            let dest_addr = base_virtual_addr + ph.vaddr - min_addr;
            let src_offset = elf.resolve_file_offset(ph_idx, ph) as usize;
            let copy_size = ph.filesz as usize;

            if copy_size > 0 && src_offset + copy_size <= elf.raw_data().len() {
                let src = &elf.raw_data()[src_offset..src_offset + copy_size];
                unsafe {
                    std::ptr::copy_nonoverlapping(src.as_ptr(), dest_addr as *mut u8, copy_size);
                }
                tracing::debug!(
                    segment_vaddr = format!("0x{:X}", ph.vaddr),
                    dest = format!("0x{:X}", dest_addr),
                    size = format_size(copy_size as u64),
                    "Segment loaded"
                );
            }
        }

        // Process parameters
        let proc_param_addr = elf
            .proc_param_header()
            .map(|ph| base_virtual_addr + ph.vaddr - min_addr)
            .unwrap_or(0);

        // EH frame
        let (eh_frame_hdr_addr, eh_frame_hdr_size) = elf
            .eh_frame_header()
            .map(|ph| (base_virtual_addr + ph.vaddr - min_addr, ph.filesz as u32))
            .unwrap_or((0, 0));

        // Parse relocations from dynamic data
        let relocations = Self::parse_relocations(&elf);

        // TLS setup
        let mut tls_index = 0u32;
        let mut tls_image_addr = 0u64;
        let mut tls_image_size = 0u32;
        let mut tls_size = 0u32;
        let mut tls_align = 0u32;

        if let Some(tls_ph) = elf.tls_header() {
            *max_tls_index += 1;
            tls_index = *max_tls_index;
            tls_image_addr = base_virtual_addr + tls_ph.vaddr - min_addr;
            tls_image_size = tls_ph.filesz as u32;
            tls_size = tls_ph.memsz as u32;
            tls_align = tls_ph.align as u32;

            tracing::debug!(
                tls_index,
                image_size = tls_image_size,
                total_size = tls_size,
                align = tls_align,
                "TLS segment configured"
            );
        }

        // Initialize init array pointers
        let init_array_addr = elf
            .dynamic_value(DT_INIT_ARRAY)
            .map(|addr| base_virtual_addr + addr - min_addr)
            .unwrap_or(0);
        let init_array_size = elf.dynamic_value(DT_INIT_ARRAYSZ).unwrap_or(0);
        let init_addr = elf
            .dynamic_value(DT_INIT)
            .map(|addr| base_virtual_addr + addr - min_addr)
            .unwrap_or(0);

        let needed = elf.needed_modules();
        tracing::info!(name = %name, needed = ?needed, "Module dependencies parsed");

        Ok(Module {
            name,
            file: path.to_path_buf(),
            elf,
            base_virtual_addr,
            aligned_base_size,
            proc_param_addr,
            eh_frame_hdr_addr,
            eh_frame_hdr_size,
            relocations,
            tls_index,
            tls_image_addr,
            tls_image_size,
            tls_size,
            tls_align,
            min_addr,
            init_array_addr,
            init_array_size,
            init_addr,
        })
    }

    /// Parse relocations from the ELF's dynamic section data.
    fn parse_relocations(elf: &Elf) -> Vec<ParsedRelocation> {
        let mut parsed = Vec::new();
        
        let symtab_off = match elf.dynamic_value(DT_SCE_SYMTAB) { Some(v) => v, None => return parsed };
        let symtab_sz = match elf.dynamic_value(DT_SCE_SYMTABSZ) { Some(v) => v, None => return parsed };
        let strtab_off = match elf.dynamic_value(DT_SCE_STRTAB) { Some(v) => v, None => return parsed };
        let strtab_sz = match elf.dynamic_value(DT_SCE_STRSZ) { Some(v) => v, None => return parsed };

        let data = &elf.dynlib_data;
        let get_string = |offset: u32| -> String {
            if (offset as u64) >= strtab_sz { return String::new(); }
            let mut end = (strtab_off + offset as u64) as usize;
            while end < data.len() && data[end] != 0 { end += 1; }
            let bytes = &data[(strtab_off + offset as u64) as usize..end];
            String::from_utf8_lossy(bytes).to_string()
        };

        // Standard Relocations
        if let (Some(rela_off), Some(rela_sz)) = (elf.dynamic_value(DT_SCE_RELA), elf.dynamic_value(DT_SCE_RELASZ)) {
            let count = rela_sz / 24;
            for i in 0..count {
                let bytes = &data[(rela_off + i * 24) as usize ..];
                if let Some(rel) = ElfRelocation::from_bytes(bytes) {
                    let mut name = String::new();
                    let mut bind = 0;
                    let mut typ = 0;
                    
                    let sym_idx = rel.symbol() as u64;
                    if sym_idx != 0 && (sym_idx * 24) < symtab_sz {
                        let sym_bytes = &data[(symtab_off + sym_idx * 24) as usize ..];
                        if let Some(sym) = ElfSymbol::from_bytes(sym_bytes) {
                            name = get_string(sym.name_offset);
                            bind = sym.binding();
                            typ = sym.sym_type();
                        }
                    }

                    parsed.push(ParsedRelocation {
                        offset: rel.offset,
                        rel_type: rel.rel_type(),
                        addend: rel.addend,
                        symbol_name: name,
                        symbol_binding: bind,
                        symbol_type: typ,
                        is_jmp_rel: false,
                    });
                }
            }
        }

        // JMPREL Relocations (PLT/GOT)
        if let (Some(jmp_off), Some(jmp_sz)) = (elf.dynamic_value(DT_SCE_JMPREL), elf.dynamic_value(DT_SCE_PLTRELSZ)) {
            let count = jmp_sz / 24;
            for i in 0..count {
                let bytes = &data[(jmp_off + i * 24) as usize ..];
                if let Some(rel) = ElfRelocation::from_bytes(bytes) {
                    let mut name = String::new();
                    let mut bind = 0;
                    let mut typ = 0;
                    
                    let sym_idx = rel.symbol() as u64;
                    if sym_idx != 0 && (sym_idx * 24) < symtab_sz {
                        let sym_bytes = &data[(symtab_off + sym_idx * 24) as usize ..];
                        if let Some(sym) = ElfSymbol::from_bytes(sym_bytes) {
                            name = get_string(sym.name_offset);
                            bind = sym.binding();
                            typ = sym.sym_type();
                        }
                    }

                    parsed.push(ParsedRelocation {
                        offset: rel.offset,
                        rel_type: rel.rel_type(),
                        addend: rel.addend,
                        symbol_name: name,
                        symbol_binding: bind,
                        symbol_type: typ,
                        is_jmp_rel: true,
                    });
                }
            }
        }

        tracing::info!("Parsed {} relocations", parsed.len());
        parsed
    }
}
