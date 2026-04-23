//! PARAM.SFO (PSF) parser for PS4 game metadata.
//!
//! The PSF format stores key-value pairs describing a game/application:
//! title, title ID (e.g. "CUSA00001"), version, content type, etc.
//!
//! ## Format Structure
//!
//! ```text
//! +------------------+
//! | PSF Header       |  20 bytes — magic, version, offsets, entry count
//! +------------------+
//! | Index Table      |  16 bytes × N entries — key offset, format, data offset
//! +------------------+
//! | Key Table        |  Variable — null-terminated key strings
//! +------------------+
//! | Data Table       |  Variable — binary/string/integer values
//! +------------------+
//! ```

use byteorder::{LittleEndian, ReadBytesExt};
use std::collections::HashMap;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// PSF magic number: "\0PSF" in big-endian.
const PSF_MAGIC: u32 = 0x00505346;

/// PSF version 1.1 (most common).
const PSF_VERSION_1_1: u32 = 0x0000_0101;

/// Data format of a PSF entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsfEntryFormat {
    /// Raw binary data.
    Binary = 0x0004,
    /// UTF-8 null-terminated string.
    Text = 0x0204,
    /// Signed 32-bit integer.
    Integer = 0x0404,
}

impl PsfEntryFormat {
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0004 => Some(Self::Binary),
            0x0204 => Some(Self::Text),
            0x0404 => Some(Self::Integer),
            _ => None,
        }
    }
}

/// A single entry in a PSF file.
#[derive(Debug, Clone)]
pub struct PsfEntry {
    /// The key name (e.g. "TITLE", "TITLE_ID").
    pub key: String,
    /// The entry value.
    pub value: PsfValue,
}

/// Value of a PSF entry.
#[derive(Debug, Clone)]
pub enum PsfValue {
    /// Raw binary data.
    Binary(Vec<u8>),
    /// UTF-8 string (without null terminator).
    Text(String),
    /// 32-bit signed integer.
    Integer(i32),
}

impl PsfValue {
    /// Returns the value as a string, if it is text.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PsfValue::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns the value as an integer, if it is one.
    pub fn as_integer(&self) -> Option<i32> {
        match self {
            PsfValue::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as binary data, if it is binary.
    pub fn as_binary(&self) -> Option<&[u8]> {
        match self {
            PsfValue::Binary(b) => Some(b.as_slice()),
            _ => None,
        }
    }
}

/// Parsed PSF (PARAM.SFO) file.
#[derive(Debug, Clone)]
pub struct Psf {
    /// PSF format version.
    pub version: u32,
    /// All entries, keyed by name.
    pub entries: HashMap<String, PsfValue>,
}

/// Errors that can occur when parsing a PSF file.
#[derive(Debug, thiserror::Error)]
pub enum PsfError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid PSF magic: expected 0x{PSF_MAGIC:08X}, got 0x{0:08X}")]
    InvalidMagic(u32),

    #[error("unknown entry format: 0x{0:04X}")]
    UnknownFormat(u16),

    #[error("invalid UTF-8 in key or value")]
    InvalidUtf8,
}

/// Raw index table entry (16 bytes).
struct RawIndexEntry {
    key_offset: u16,
    param_fmt: u16,
    param_len: u32,
    _param_max_len: u32,
    data_offset: u32,
}

impl Psf {
    /// Parse a PSF file from a byte slice.
    pub fn parse(data: &[u8]) -> Result<Self, PsfError> {
        let mut cursor = Cursor::new(data);

        // Read header
        let magic = cursor.read_u32::<LittleEndian>()?;

        // PSF magic is stored as big-endian 0x00505346, but when read as LE from the
        // file it appears as the bytes [00, 50, 53, 46]. Let's check both interpretations.
        if magic != PSF_MAGIC && magic.swap_bytes() != PSF_MAGIC {
            return Err(PsfError::InvalidMagic(magic));
        }

        let version = cursor.read_u32::<LittleEndian>()?;
        let key_table_offset = cursor.read_u32::<LittleEndian>()?;
        let data_table_offset = cursor.read_u32::<LittleEndian>()?;
        let index_entries = cursor.read_u32::<LittleEndian>()?;

        tracing::debug!(
            version = format!("0x{version:08X}"),
            key_table_offset,
            data_table_offset,
            index_entries,
            "PSF header parsed"
        );

        // Read index table entries
        let mut raw_entries = Vec::with_capacity(index_entries as usize);
        for _ in 0..index_entries {
            raw_entries.push(RawIndexEntry {
                key_offset: cursor.read_u16::<LittleEndian>()?,
                param_fmt: cursor.read_u16::<LittleEndian>()?,
                param_len: cursor.read_u32::<LittleEndian>()?,
                _param_max_len: cursor.read_u32::<LittleEndian>()?,
                data_offset: cursor.read_u32::<LittleEndian>()?,
            });
        }

        // Parse each entry
        let mut entries = HashMap::with_capacity(index_entries as usize);

        for raw in &raw_entries {
            // Read key from key table
            let key_start = (key_table_offset as u64) + (raw.key_offset as u64);
            cursor.seek(SeekFrom::Start(key_start))?;

            let mut key_bytes = Vec::new();
            loop {
                let byte = cursor.read_u8()?;
                if byte == 0 {
                    break;
                }
                key_bytes.push(byte);
            }
            let key = String::from_utf8(key_bytes).map_err(|_| PsfError::InvalidUtf8)?;

            // Read value from data table
            let data_start = (data_table_offset as u64) + (raw.data_offset as u64);
            cursor.seek(SeekFrom::Start(data_start))?;

            let fmt = PsfEntryFormat::from_u16(raw.param_fmt)
                .ok_or(PsfError::UnknownFormat(raw.param_fmt))?;

            let value = match fmt {
                PsfEntryFormat::Binary => {
                    let mut buf = vec![0u8; raw.param_len as usize];
                    cursor.read_exact(&mut buf)?;
                    PsfValue::Binary(buf)
                }
                PsfEntryFormat::Text => {
                    // param_len includes the null terminator
                    let str_len = if raw.param_len > 0 {
                        (raw.param_len - 1) as usize
                    } else {
                        0
                    };
                    let mut buf = vec![0u8; str_len];
                    cursor.read_exact(&mut buf)?;
                    let text = String::from_utf8(buf).map_err(|_| PsfError::InvalidUtf8)?;
                    PsfValue::Text(text)
                }
                PsfEntryFormat::Integer => {
                    let val = cursor.read_i32::<LittleEndian>()?;
                    PsfValue::Integer(val)
                }
            };

            tracing::trace!(key = %key, "PSF entry parsed");
            entries.insert(key, value);
        }

        Ok(Psf { version, entries })
    }

    /// Parse a PSF file from disk.
    pub fn open(path: &Path) -> Result<Self, PsfError> {
        let data = std::fs::read(path)?;
        Self::parse(&data)
    }

    /// Get a string entry by key.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.entries.get(key).and_then(|v| v.as_str())
    }

    /// Get an integer entry by key.
    pub fn get_integer(&self, key: &str) -> Option<i32> {
        self.entries.get(key).and_then(|v| v.as_integer())
    }

    /// Get the game title.
    pub fn title(&self) -> Option<&str> {
        self.get_string("TITLE")
    }

    /// Get the title ID (e.g. "CUSA00001").
    pub fn title_id(&self) -> Option<&str> {
        self.get_string("TITLE_ID")
    }

    /// Get the content ID.
    pub fn content_id(&self) -> Option<&str> {
        self.get_string("CONTENT_ID")
    }

    /// Get the application version string.
    pub fn app_version(&self) -> Option<&str> {
        self.get_string("APP_VER")
    }

    /// Get the category (e.g. "gd" for game data).
    pub fn category(&self) -> Option<&str> {
        self.get_string("CATEGORY")
    }
}

impl std::fmt::Display for Psf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "PSF (version 0x{:08X}):", self.version)?;
        let mut keys: Vec<&String> = self.entries.keys().collect();
        keys.sort();
        for key in keys {
            let value = &self.entries[key];
            match value {
                PsfValue::Text(s) => writeln!(f, "  {key}: \"{s}\"")?,
                PsfValue::Integer(v) => writeln!(f, "  {key}: {v} (0x{v:08X})")?,
                PsfValue::Binary(b) => writeln!(f, "  {key}: <binary, {} bytes>", b.len())?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid PSF binary for testing.
    fn build_test_psf() -> Vec<u8> {
        let mut buf = Vec::new();

        // Header (20 bytes)
        // Magic (big-endian bytes: 00 50 53 46)
        buf.extend_from_slice(&[0x00, 0x50, 0x53, 0x46]);
        // Version 1.1
        buf.extend_from_slice(&PSF_VERSION_1_1.to_le_bytes());

        // We'll have 2 entries: "TITLE" (text) and "APP_VER" (text)
        let index_entries: u32 = 2;

        // Index table starts at offset 20, each entry is 16 bytes
        let index_table_size = index_entries * 16;
        let key_table_offset = 20 + index_table_size;

        // Keys: "APP_VER\0TITLE\0"
        let key_app_ver = b"APP_VER\0";
        let key_title = b"TITLE\0";
        let key_table_size = key_app_ver.len() + key_title.len();

        let data_table_offset = key_table_offset + key_table_size as u32;

        // Data: "01.00\0" (6 bytes, padded to 8) and "Test Game\0" (10 bytes, padded to 12)
        let data_app_ver = b"01.00\0";
        let data_title = b"Test Game\0";

        buf.extend_from_slice(&key_table_offset.to_le_bytes());
        buf.extend_from_slice(&data_table_offset.to_le_bytes());
        buf.extend_from_slice(&index_entries.to_le_bytes());

        // Index entry 0: APP_VER
        buf.extend_from_slice(&0u16.to_le_bytes()); // key_offset = 0
        buf.extend_from_slice(&0x0204u16.to_le_bytes()); // fmt = Text
        buf.extend_from_slice(&(data_app_ver.len() as u32).to_le_bytes()); // param_len
        buf.extend_from_slice(&8u32.to_le_bytes()); // param_max_len
        buf.extend_from_slice(&0u32.to_le_bytes()); // data_offset = 0

        // Index entry 1: TITLE
        buf.extend_from_slice(&(key_app_ver.len() as u16).to_le_bytes()); // key_offset
        buf.extend_from_slice(&0x0204u16.to_le_bytes()); // fmt = Text
        buf.extend_from_slice(&(data_title.len() as u32).to_le_bytes()); // param_len
        buf.extend_from_slice(&12u32.to_le_bytes()); // param_max_len
        buf.extend_from_slice(&8u32.to_le_bytes()); // data_offset (after padded app_ver)

        // Key table
        buf.extend_from_slice(key_app_ver);
        buf.extend_from_slice(key_title);

        // Data table
        buf.extend_from_slice(data_app_ver);
        buf.extend_from_slice(&[0u8; 2]); // padding to 8
        buf.extend_from_slice(data_title);
        buf.extend_from_slice(&[0u8; 2]); // padding to 12

        buf
    }

    #[test]
    fn test_parse_psf() {
        let data = build_test_psf();
        let psf = Psf::parse(&data).expect("failed to parse PSF");

        assert_eq!(psf.title(), Some("Test Game"));
        assert_eq!(psf.app_version(), Some("01.00"));
    }

    #[test]
    fn test_invalid_magic() {
        let data = vec![0xFF; 20];
        let result = Psf::parse(&data);
        assert!(result.is_err());
    }
}
