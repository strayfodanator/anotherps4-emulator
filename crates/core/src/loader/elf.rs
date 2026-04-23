//! ELF64 parser with PS4/SCE extensions.
//!
//! Handles both standard ELF and Sony's SELF (Signed ELF) format.
//! The SELF header wraps a standard ELF with encryption metadata;
//! for emulation we expect decrypted files but still parse the SELF header.

use anotherps4_common::*;
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

// ============================================================================
// SELF Header (Sony's encrypted ELF wrapper)
// ============================================================================

/// Magic number for SELF files.
pub const SELF_MAGIC: u32 = 0x1D3D_154F;

/// SELF file header (32 bytes).
#[derive(Debug, Clone, Default)]
pub struct SelfHeader {
    pub magic: u32,
    pub version: u8,
    pub mode: u8,
    pub endian: u8,
    pub attributes: u8,
    pub category: u8,
    pub program_type: u8,
    pub padding1: u16,
    pub header_size: u16,
    pub meta_size: u16,
    pub file_size: u32,
    pub padding2: u32,
    pub segment_count: u16,
    pub unknown_1a: u16,
    pub padding3: u32,
}

/// SELF segment header (32 bytes).
#[derive(Debug, Clone, Default)]
pub struct SelfSegmentHeader {
    pub flags: u64,
    pub file_offset: u64,
    pub file_size: u64,
    pub memory_size: u64,
}

impl SelfSegmentHeader {
    pub fn is_blocked(&self) -> bool {
        (self.flags & 0x800) != 0
    }

    pub fn get_id(&self) -> u32 {
        ((self.flags >> 20) & 0xFFF) as u32
    }

    pub fn is_ordered(&self) -> bool {
        (self.flags & 1) != 0
    }

    pub fn is_encrypted(&self) -> bool {
        (self.flags & 2) != 0
    }

    pub fn is_signed(&self) -> bool {
        (self.flags & 4) != 0
    }

    pub fn is_compressed(&self) -> bool {
        (self.flags & 8) != 0
    }
}

// ============================================================================
// ELF Header
// ============================================================================

/// ELF magic bytes: 0x7F 'E' 'L' 'F'
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ElfType {
    None = 0x0,
    Relocatable = 0x1,
    Executable = 0x2,
    SharedObject = 0x3,
    Core = 0x4,
    /// SCE executable.
    SceExec = 0xFE00,
    /// SCE stub library.
    SceStubLib = 0xFE0C,
    /// SCE dynamic executable (most common for games).
    SceDynExec = 0xFE10,
    /// SCE dynamic library (.sprx).
    SceDynamic = 0xFE18,
    /// Unknown type.
    Unknown(u16),
}

impl From<u16> for ElfType {
    fn from(v: u16) -> Self {
        match v {
            0x0 => Self::None,
            0x1 => Self::Relocatable,
            0x2 => Self::Executable,
            0x3 => Self::SharedObject,
            0x4 => Self::Core,
            0xFE00 => Self::SceExec,
            0xFE0C => Self::SceStubLib,
            0xFE10 => Self::SceDynExec,
            0xFE18 => Self::SceDynamic,
            other => Self::Unknown(other),
        }
    }
}

/// ELF ident structure (first 16 bytes).
#[derive(Debug, Clone, Default)]
pub struct ElfIdent {
    pub magic: [u8; 4],
    pub class: u8,
    pub data: u8,
    pub version: u8,
    pub os_abi: u8,
    pub abi_version: u8,
    pub padding: [u8; 7],
}

/// ELF64 file header (64 bytes).
#[derive(Debug, Clone)]
pub struct ElfHeader {
    pub ident: ElfIdent,
    pub elf_type: ElfType,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

// ============================================================================
// Program Header
// ============================================================================

/// ELF program header type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ProgramType {
    Null = 0x0,
    Load = 0x1,
    Dynamic = 0x2,
    Interp = 0x3,
    Note = 0x4,
    Phdr = 0x6,
    Tls = 0x7,
    /// SCE relocation data.
    SceRela = 0x6000_0000,
    /// SCE dynamic library data.
    SceDynlibData = 0x6100_0000,
    /// SCE process parameters.
    SceProcParam = 0x6100_0001,
    /// SCE module parameters.
    SceModuleParam = 0x6100_0002,
    /// SCE relocatable read-only.
    SceRelro = 0x6100_0010,
    /// GNU exception frame header.
    GnuEhFrame = 0x6474_E550,
    /// GNU stack permissions.
    GnuStack = 0x6474_E551,
    /// GNU read-only relocations.
    GnuRelro = 0x6474_E552,
    /// SCE comment.
    SceComment = 0x6FFF_FF00,
    /// SCE library version.
    SceLibVersion = 0x6FFF_FF01,
    /// Unknown type.
    Unknown(u32),
}

impl From<u32> for ProgramType {
    fn from(v: u32) -> Self {
        match v {
            0x0 => Self::Null,
            0x1 => Self::Load,
            0x2 => Self::Dynamic,
            0x3 => Self::Interp,
            0x4 => Self::Note,
            0x6 => Self::Phdr,
            0x7 => Self::Tls,
            0x6000_0000 => Self::SceRela,
            0x6100_0000 => Self::SceDynlibData,
            0x6100_0001 => Self::SceProcParam,
            0x6100_0002 => Self::SceModuleParam,
            0x6100_0010 => Self::SceRelro,
            0x6474_E550 => Self::GnuEhFrame,
            0x6474_E551 => Self::GnuStack,
            0x6474_E552 => Self::GnuRelro,
            0x6FFF_FF00 => Self::SceComment,
            0x6FFF_FF01 => Self::SceLibVersion,
            other => Self::Unknown(other),
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ProgramFlags: u32 {
        const EXECUTE = 0x1;
        const WRITE   = 0x2;
        const READ    = 0x4;
    }
}

/// ELF64 program header (56 bytes).
#[derive(Debug, Clone)]
pub struct ProgramHeader {
    pub phdr_type: ProgramType,
    pub flags: ProgramFlags,
    pub offset: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

// ============================================================================
// Dynamic Section Tags
// ============================================================================

/// SCE dynamic tag constants.
pub mod sce_dynamic {
    pub const DT_NULL: i64 = 0;
    pub const DT_NEEDED: i64 = 0x0000_0001;
    pub const DT_RELA: i64 = 0x0000_0007;
    pub const DT_INIT: i64 = 0x0000_000C;
    pub const DT_FINI: i64 = 0x0000_000D;
    pub const DT_DEBUG: i64 = 0x0000_0015;
    pub const DT_TEXTREL: i64 = 0x0000_0016;
    pub const DT_INIT_ARRAY: i64 = 0x0000_0019;
    pub const DT_FINI_ARRAY: i64 = 0x0000_001A;
    pub const DT_INIT_ARRAYSZ: i64 = 0x0000_001B;
    pub const DT_FINI_ARRAYSZ: i64 = 0x0000_001C;
    pub const DT_FLAGS: i64 = 0x0000_001E;
    pub const DT_PREINIT_ARRAY: i64 = 0x0000_0020;
    pub const DT_PREINIT_ARRAYSZ: i64 = 0x0000_0021;

    pub const DT_SCE_FINGERPRINT: i64 = 0x6100_0007;
    pub const DT_SCE_ORIGINAL_FILENAME: i64 = 0x6100_0009;
    pub const DT_SCE_MODULE_INFO: i64 = 0x6100_000D;
    pub const DT_SCE_NEEDED_MODULE: i64 = 0x6100_000F;
    pub const DT_SCE_MODULE_ATTR: i64 = 0x6100_0011;
    pub const DT_SCE_EXPORT_LIB: i64 = 0x6100_0013;
    pub const DT_SCE_IMPORT_LIB: i64 = 0x6100_0015;
    pub const DT_SCE_IMPORT_LIB_ATTR: i64 = 0x6100_0019;
    pub const DT_SCE_HASH: i64 = 0x6100_0025;
    pub const DT_SCE_PLTGOT: i64 = 0x6100_0027;
    pub const DT_SCE_JMPREL: i64 = 0x6100_0029;
    pub const DT_SCE_PLTREL: i64 = 0x6100_002B;
    pub const DT_SCE_PLTRELSZ: i64 = 0x6100_002D;
    pub const DT_SCE_RELA: i64 = 0x6100_002F;
    pub const DT_SCE_RELASZ: i64 = 0x6100_0031;
    pub const DT_SCE_RELAENT: i64 = 0x6100_0033;
    pub const DT_SCE_STRTAB: i64 = 0x6100_0035;
    pub const DT_SCE_STRSZ: i64 = 0x6100_0037;
    pub const DT_SCE_SYMTAB: i64 = 0x6100_0039;
    pub const DT_SCE_SYMENT: i64 = 0x6100_003B;
    pub const DT_SCE_HASHSZ: i64 = 0x6100_003D;
    pub const DT_SCE_SYMTABSZ: i64 = 0x6100_003F;
}

/// A dynamic table entry.
#[derive(Debug, Clone)]
pub struct DynamicEntry {
    pub tag: i64,
    pub value: u64,
}

// ============================================================================
// Symbol Table
// ============================================================================

/// Symbol binding.
pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

/// Symbol type.
pub const STT_NOTYPE: u8 = 0;
pub const STT_OBJECT: u8 = 1;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;
pub const STT_TLS: u8 = 6;
/// SCE-specific: module_start/module_stop.
pub const STT_SCE: u8 = 11;

/// An ELF symbol table entry.
#[derive(Debug, Clone)]
pub struct ElfSymbol {
    pub name_offset: u32,
    pub info: u8,
    pub other: u8,
    pub shndx: u16,
    pub value: u64,
    pub size: u64,
}

impl ElfSymbol {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 24 {
            return None;
        }
        let mut cursor = std::io::Cursor::new(bytes);
        use byteorder::{LittleEndian, ReadBytesExt};
        Some(Self {
            name_offset: cursor.read_u32::<LittleEndian>().unwrap(),
            info: cursor.read_u8().unwrap(),
            other: cursor.read_u8().unwrap(),
            shndx: cursor.read_u16::<LittleEndian>().unwrap(),
            value: cursor.read_u64::<LittleEndian>().unwrap(),
            size: cursor.read_u64::<LittleEndian>().unwrap(),
        })
    }

    pub fn binding(&self) -> u8 {
        self.info >> 4
    }

    pub fn sym_type(&self) -> u8 {
        self.info & 0xF
    }

    pub fn visibility(&self) -> u8 {
        self.other & 3
    }
}

// ============================================================================
// Relocation
// ============================================================================

/// x86-64 relocation types used by PS4.
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;
pub const R_X86_64_RELATIVE: u32 = 8;
pub const R_X86_64_DTPMOD64: u32 = 16;

/// ELF relocation entry with addend (RELA).
#[derive(Debug, Clone)]
pub struct ElfRelocation {
    pub offset: u64,
    pub info: u64,
    pub addend: i64,
}

impl ElfRelocation {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 24 {
            return None;
        }
        let mut cursor = std::io::Cursor::new(bytes);
        use byteorder::{LittleEndian, ReadBytesExt};
        Some(Self {
            offset: cursor.read_u64::<LittleEndian>().unwrap(),
            info: cursor.read_u64::<LittleEndian>().unwrap(),
            addend: cursor.read_i64::<LittleEndian>().unwrap(),
        })
    }

    pub fn symbol(&self) -> u32 {
        (self.info >> 32) as u32
    }

    pub fn rel_type(&self) -> u32 {
        (self.info & 0xFFFF_FFFF) as u32
    }
}

// ============================================================================
// Program Type (SELF)
// ============================================================================

/// SELF program type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfProgramType {
    Fake,
    NpdrmExec,
    NpdrmDynlib,
    SystemExec,
    SystemDynlib,
    HostKernel,
    SecureModule,
    SecureKernel,
    Unknown(u64),
}

impl From<u64> for SelfProgramType {
    fn from(v: u64) -> Self {
        match v {
            0x1 => Self::Fake,
            0x4 => Self::NpdrmExec,
            0x5 => Self::NpdrmDynlib,
            0x8 => Self::SystemExec,
            0x9 => Self::SystemDynlib,
            0xC => Self::HostKernel,
            0xE => Self::SecureModule,
            0xF => Self::SecureKernel,
            v => Self::Unknown(v),
        }
    }
}

// ============================================================================
// ELF Parser
// ============================================================================

/// Errors during ELF parsing.
#[derive(Debug, thiserror::Error)]
pub enum ElfError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("not a valid ELF or SELF file")]
    NotElf,

    #[error("not a 64-bit ELF (class={0})")]
    Not64Bit(u8),

    #[error("not little-endian (data={0})")]
    NotLittleEndian(u8),

    #[error("not x86-64 (machine=0x{0:04X})")]
    NotX86_64(u16),
}

/// A parsed PS4 ELF/SELF file.
#[derive(Debug)]
pub struct Elf {
    /// True if the file has a SELF header.
    pub is_self: bool,
    /// SELF header (if present).
    pub self_header: Option<SelfHeader>,
    /// SELF segment headers (if present).
    pub self_segments: Vec<SelfSegmentHeader>,
    /// ELF header.
    pub header: ElfHeader,
    /// Program headers.
    pub program_headers: Vec<ProgramHeader>,
    /// Dynamic entries (from PT_DYNAMIC / PT_SCE_DYNLIBDATA).
    pub dynamic_entries: Vec<DynamicEntry>,
    /// Raw dynamic data segment.
    pub dynlib_data: Vec<u8>,
    /// The raw file data.
    data: Vec<u8>,
}

impl Elf {
    /// Parse an ELF/SELF file from a path on disk.
    pub fn open(path: &Path) -> Result<Self, ElfError> {
        let data = std::fs::read(path)?;
        Self::parse(data)
    }

    /// Parse an ELF/SELF from raw bytes (takes ownership).
    pub fn parse(data: Vec<u8>) -> Result<Self, ElfError> {
        let mut cursor = Cursor::new(&data);

        // Check if it's a SELF file first
        let first_u32 = cursor.read_u32::<LittleEndian>()?;
        cursor.seek(SeekFrom::Start(0))?;

        let (is_self, self_header, self_segments, elf_offset) = if first_u32 == SELF_MAGIC {
            let sh = Self::read_self_header(&mut cursor)?;
            let segments = Self::read_self_segments(&mut cursor, sh.segment_count)?;
            let elf_off = cursor.position();
            (true, Some(sh), segments, elf_off)
        } else {
            (false, None, vec![], 0)
        };

        // Parse ELF header
        cursor.seek(SeekFrom::Start(elf_offset))?;
        let header = Self::read_elf_header(&mut cursor)?;

        // Parse program headers
        let mut program_headers = Vec::with_capacity(header.phnum as usize);
        cursor.seek(SeekFrom::Start(elf_offset + header.phoff))?;
        for _ in 0..header.phnum {
            program_headers.push(Self::read_program_header(&mut cursor)?);
        }

        // Extract dynamic entries and dynlib data
        let mut dynamic_entries = Vec::new();
        let mut dynlib_data = Vec::new();

        // Find PT_DYNAMIC and PT_SCE_DYNLIBDATA program headers
        let mut dynamic_phdr_idx = None;
        let mut dynlib_phdr_idx = None;

        for (idx, ph) in program_headers.iter().enumerate() {
            match ph.phdr_type {
                ProgramType::Dynamic => dynamic_phdr_idx = Some(idx),
                ProgramType::SceDynlibData => dynlib_phdr_idx = Some(idx),
                _ => {}
            }
        }

        // Log all SELF segments for debugging
        if is_self {
            for (i, seg) in self_segments.iter().enumerate() {
                tracing::info!(
                    idx = i,
                    flags = format!("0x{:X}", seg.flags),
                    id = seg.get_id(),
                    blocked = seg.is_blocked(),
                    file_offset = format!("0x{:X}", seg.file_offset),
                    file_size = seg.file_size,
                    mem_size = seg.memory_size,
                    "SELF segment"
                );
            }
        }

        // Log found dynamic program headers
        tracing::info!(
            dynamic_phdr = ?dynamic_phdr_idx,
            dynlib_phdr = ?dynlib_phdr_idx,
            "Program header indices for dynamic linking"
        );

        // Parse dynamic entries from PT_DYNAMIC
        if let Some(dyn_idx) = dynamic_phdr_idx {
            let dyn_ph = &program_headers[dyn_idx];
            let real_offset = Self::resolve_file_offset_internal(
                is_self,
                &self_segments,
                &program_headers,
                dyn_idx,
                dyn_ph,
                elf_offset,
            );
            tracing::info!(
                dyn_idx,
                p_offset = format!("0x{:X}", dyn_ph.offset),
                real_offset = format!("0x{:X}", real_offset),
                filesz = dyn_ph.filesz,
                data_len = data.len(),
                "PT_DYNAMIC: attempting to parse"
            );
            if (real_offset as usize) < data.len() {
                cursor.seek(SeekFrom::Start(real_offset))?;
                let num_entries = dyn_ph.filesz / 16; // Each entry is 16 bytes
                for _ in 0..num_entries {
                    let tag = match cursor.read_i64::<LittleEndian>() {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    let value = match cursor.read_u64::<LittleEndian>() {
                        Ok(v) => v,
                        Err(_) => break,
                    };
                    if tag == sce_dynamic::DT_NULL {
                        break;
                    }
                    dynamic_entries.push(DynamicEntry { tag, value });
                }
            } else {
                tracing::error!("PT_DYNAMIC real_offset beyond data!");
            }
        }

        // Extract raw dynlib data from PT_SCE_DYNLIBDATA
        if let Some(dl_idx) = dynlib_phdr_idx {
            let dl_ph = &program_headers[dl_idx];
            let real_offset = Self::resolve_file_offset_internal(
                is_self,
                &self_segments,
                &program_headers,
                dl_idx,
                dl_ph,
                elf_offset,
            );
            let start = real_offset as usize;
            let end = start + dl_ph.filesz as usize;
            if end <= data.len() {
                dynlib_data = data[start..end].to_vec();
            } else if start < data.len() {
                // Partial read - get what we can
                dynlib_data = data[start..].to_vec();
                tracing::warn!(
                    expected = dl_ph.filesz,
                    actual = dynlib_data.len(),
                    "PT_SCE_DYNLIBDATA truncated"
                );
            }
        }

        tracing::info!(
            is_self,
            elf_type = ?header.elf_type,
            entry = format!("0x{:X}", header.entry),
            program_headers = program_headers.len(),
            dynamic_entries = dynamic_entries.len(),
            "ELF parsed successfully"
        );

        Ok(Elf {
            is_self,
            self_header,
            self_segments,
            header,
            program_headers,
            dynamic_entries,
            dynlib_data,
            data,
        })
    }

    /// Returns true if this is a shared library (.sprx).
    pub fn is_shared_lib(&self) -> bool {
        self.header.elf_type == ElfType::SceDynamic
    }

    /// Returns the entry point virtual address.
    pub fn entry_point(&self) -> VAddr {
        self.header.entry
    }

    /// Get the raw file data.
    pub fn raw_data(&self) -> &[u8] {
        &self.data
    }

    /// Get the ELF data offset (after SELF header, if any).
    pub fn elf_offset(&self) -> u64 {
        if self.is_self {
            if let Some(ref sh) = self.self_header {
                // SELF header + segment headers
                32 + (sh.segment_count as u64 * 32)
            } else {
                0
            }
        } else {
            0
        }
    }

    /// Get all program headers.
    pub fn program_headers(&self) -> &[ProgramHeader] {
        &self.program_headers
    }

    /// Get load segments (PT_LOAD program headers).
    pub fn load_segments(&self) -> impl Iterator<Item = &ProgramHeader> {
        self.program_headers
            .iter()
            .filter(|ph| ph.phdr_type == ProgramType::Load)
    }

    /// Resolve the real file offset for a given program header's data
    pub fn resolve_file_offset(&self, phdr_idx: usize, phdr: &ProgramHeader) -> u64 {
        Self::resolve_file_offset_internal(
            self.is_self,
            &self.self_segments,
            &self.program_headers,
            phdr_idx,
            phdr,
            self.elf_offset(),
        )
    }

    /// Helper: resolve the actual file offset for a program header's data.
    /// In a SELF file, the ELF p_offset is virtual; the real data offset
    /// comes from the SELF segment header. There are two cases:
    /// 1. The SELF segment ID directly matches this phdr index (e.g., PT_SCE_DYNLIBDATA)
    /// 2. The phdr data lives inside another LOAD segment (e.g., PT_DYNAMIC inside a LOAD)
    ///    In this case, we find which blocked SELF segment's ELF phdr range contains our offset.
    fn resolve_file_offset_internal(
        is_self: bool,
        self_segments: &[SelfSegmentHeader],
        program_headers: &[ProgramHeader],
        phdr_idx: usize,
        phdr: &ProgramHeader,
        elf_offset: u64,
    ) -> u64 {
        if !is_self {
            return phdr.offset;
        }
        
        // Case 1: Direct ID match
        for (si, seg) in self_segments.iter().enumerate() {
            if seg.is_blocked() && seg.get_id() as usize == phdr_idx {
                return seg.file_offset;
            }
        }
        
        // Case 2: The segment data is contained within another LOAD segment.
        // Find which blocked SELF segment's ELF phdr range contains our p_offset.
        let target_offset = phdr.offset;
        for (si, seg) in self_segments.iter().enumerate() {
            if !seg.is_blocked() { continue; }
            let parent_phdr_id = seg.get_id() as usize;
            if parent_phdr_id >= program_headers.len() { continue; }
            let parent_ph = &program_headers[parent_phdr_id];
            
            // Check if our target offset falls within this parent's ELF range
            if target_offset >= parent_ph.offset && target_offset < parent_ph.offset + parent_ph.filesz {
                let offset_within = target_offset - parent_ph.offset;
                let real = seg.file_offset + offset_within;
                return real;
            }
        }
        
        elf_offset + phdr.offset
    }

    /// Get the TLS program header, if any.
    pub fn tls_header(&self) -> Option<&ProgramHeader> {
        self.program_headers
            .iter()
            .find(|ph| ph.phdr_type == ProgramType::Tls)
    }

    /// Get the process parameter program header (SCE_PROCPARAM).
    pub fn proc_param_header(&self) -> Option<&ProgramHeader> {
        self.program_headers
            .iter()
            .find(|ph| ph.phdr_type == ProgramType::SceProcParam)
    }

    /// Get the GNU EH frame header.
    pub fn eh_frame_header(&self) -> Option<&ProgramHeader> {
        self.program_headers
            .iter()
            .find(|ph| ph.phdr_type == ProgramType::GnuEhFrame)
    }

    /// Get a dynamic entry value by tag.
    pub fn dynamic_value(&self, tag: i64) -> Option<u64> {
        self.dynamic_entries
            .iter()
            .find(|e| e.tag == tag)
            .map(|e| e.value)
    }

    /// Read segment data from the raw file.
    pub fn segment_data(&self, ph: &ProgramHeader) -> &[u8] {
        let offset = (self.elf_offset() + ph.offset) as usize;
        let end = offset + ph.filesz as usize;
        if end <= self.data.len() {
            &self.data[offset..end]
        } else {
            &[]
        }
    }

    /// Helper to get a string from the dynamic string table.
    pub fn get_dynamic_string(&self, offset: u64) -> String {
        let strtab_off = match self.dynamic_value(sce_dynamic::DT_SCE_STRTAB) {
            Some(v) => v,
            None => return String::new(),
        };
        let strtab_sz = match self.dynamic_value(sce_dynamic::DT_SCE_STRSZ) {
            Some(v) => v,
            None => return String::new(),
        };

        if offset >= strtab_sz {
            return String::new();
        }

        let start = (strtab_off + offset) as usize;
        if start >= self.dynlib_data.len() {
            return String::new();
        }

        let mut end = start;
        while end < self.dynlib_data.len() && self.dynlib_data[end] != 0 {
            end += 1;
        }

        String::from_utf8_lossy(&self.dynlib_data[start..end]).to_string()
    }

    /// Get all required modules (DT_NEEDED and DT_SCE_IMPORT_LIB).
    pub fn needed_modules(&self) -> Vec<String> {
        let mut modules = Vec::new();
        for entry in &self.dynamic_entries {
            if entry.tag == sce_dynamic::DT_NEEDED || entry.tag == sce_dynamic::DT_SCE_IMPORT_LIB {
                let name = self.get_dynamic_string(entry.value);
                if !name.is_empty() {
                    modules.push(name);
                }
            }
        }
        modules
    }

    // --- Private parsing helpers ---

    fn read_self_header(cursor: &mut Cursor<&Vec<u8>>) -> Result<SelfHeader, ElfError> {
        Ok(SelfHeader {
            magic: cursor.read_u32::<LittleEndian>()?,
            version: cursor.read_u8()?,
            mode: cursor.read_u8()?,
            endian: cursor.read_u8()?,
            attributes: cursor.read_u8()?,
            category: cursor.read_u8()?,
            program_type: cursor.read_u8()?,
            padding1: cursor.read_u16::<LittleEndian>()?,
            header_size: cursor.read_u16::<LittleEndian>()?,
            meta_size: cursor.read_u16::<LittleEndian>()?,
            file_size: cursor.read_u32::<LittleEndian>()?,
            padding2: cursor.read_u32::<LittleEndian>()?,
            segment_count: cursor.read_u16::<LittleEndian>()?,
            unknown_1a: cursor.read_u16::<LittleEndian>()?,
            padding3: cursor.read_u32::<LittleEndian>()?,
        })
    }

    fn read_self_segments(
        cursor: &mut Cursor<&Vec<u8>>,
        count: u16,
    ) -> Result<Vec<SelfSegmentHeader>, ElfError> {
        let mut segments = Vec::with_capacity(count as usize);
        for _ in 0..count {
            segments.push(SelfSegmentHeader {
                flags: cursor.read_u64::<LittleEndian>()?,
                file_offset: cursor.read_u64::<LittleEndian>()?,
                file_size: cursor.read_u64::<LittleEndian>()?,
                memory_size: cursor.read_u64::<LittleEndian>()?,
            });
        }
        Ok(segments)
    }

    fn read_elf_header(cursor: &mut Cursor<&Vec<u8>>) -> Result<ElfHeader, ElfError> {
        let mut ident = ElfIdent::default();
        cursor.read_exact(&mut ident.magic)?;
        if ident.magic != ELF_MAGIC {
            return Err(ElfError::NotElf);
        }

        ident.class = cursor.read_u8()?;
        if ident.class != 2 {
            return Err(ElfError::Not64Bit(ident.class));
        }

        ident.data = cursor.read_u8()?;
        if ident.data != 1 {
            return Err(ElfError::NotLittleEndian(ident.data));
        }

        ident.version = cursor.read_u8()?;
        ident.os_abi = cursor.read_u8()?;
        ident.abi_version = cursor.read_u8()?;
        cursor.read_exact(&mut ident.padding)?;

        let elf_type_raw = cursor.read_u16::<LittleEndian>()?;
        let machine = cursor.read_u16::<LittleEndian>()?;

        // x86-64 is machine type 62
        if machine != 62 {
            return Err(ElfError::NotX86_64(machine));
        }

        Ok(ElfHeader {
            ident,
            elf_type: ElfType::from(elf_type_raw),
            machine,
            version: cursor.read_u32::<LittleEndian>()?,
            entry: cursor.read_u64::<LittleEndian>()?,
            phoff: cursor.read_u64::<LittleEndian>()?,
            shoff: cursor.read_u64::<LittleEndian>()?,
            flags: cursor.read_u32::<LittleEndian>()?,
            ehsize: cursor.read_u16::<LittleEndian>()?,
            phentsize: cursor.read_u16::<LittleEndian>()?,
            phnum: cursor.read_u16::<LittleEndian>()?,
            shentsize: cursor.read_u16::<LittleEndian>()?,
            shnum: cursor.read_u16::<LittleEndian>()?,
            shstrndx: cursor.read_u16::<LittleEndian>()?,
        })
    }

    fn read_program_header(cursor: &mut Cursor<&Vec<u8>>) -> Result<ProgramHeader, ElfError> {
        let type_raw = cursor.read_u32::<LittleEndian>()?;
        let flags_raw = cursor.read_u32::<LittleEndian>()?;

        Ok(ProgramHeader {
            phdr_type: ProgramType::from(type_raw),
            flags: ProgramFlags::from_bits_truncate(flags_raw),
            offset: cursor.read_u64::<LittleEndian>()?,
            vaddr: cursor.read_u64::<LittleEndian>()?,
            paddr: cursor.read_u64::<LittleEndian>()?,
            filesz: cursor.read_u64::<LittleEndian>()?,
            memsz: cursor.read_u64::<LittleEndian>()?,
            align: cursor.read_u64::<LittleEndian>()?,
        })
    }
}

impl std::fmt::Display for Elf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== ELF File ===")?;
        writeln!(f, "  Type: {:?}", self.header.elf_type)?;
        writeln!(f, "  Entry: 0x{:016X}", self.header.entry)?;
        writeln!(f, "  Program Headers: {}", self.program_headers.len())?;
        writeln!(f, "  Is SELF: {}", self.is_self)?;

        for (i, ph) in self.program_headers.iter().enumerate() {
            writeln!(
                f,
                "  PHDR[{i}]: {:?} offset=0x{:X} vaddr=0x{:X} filesz=0x{:X} memsz=0x{:X} flags={:?}",
                ph.phdr_type, ph.offset, ph.vaddr, ph.filesz, ph.memsz, ph.flags
            )?;
        }

        Ok(())
    }
}
