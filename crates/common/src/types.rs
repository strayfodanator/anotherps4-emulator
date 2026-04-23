//! Fundamental type aliases used throughout the emulator.
//!
//! These mirror the PS4's memory model: 64-bit virtual addresses,
//! 40-bit physical addresses (GPU limit), and standard integer types.

/// Virtual address in the guest (PS4) address space.
pub type VAddr = u64;

/// Physical address (Direct Memory address).
pub type PAddr = u64;

/// Page size used by the PS4 kernel (16 KB).
pub const PAGE_SIZE: u64 = 16 * 1024;

/// Page size mask.
pub const PAGE_MASK: u64 = PAGE_SIZE - 1;

/// Maximum GPU-addressable virtual address (40-bit).
pub const MAX_GPU_ADDRESS: VAddr = 0x10_0000_0000;

/// Default base address for memory mappings.
pub const DEFAULT_MAPPING_BASE: VAddr = 0x2_0000_0000;

/// Size of the system-managed virtual memory region (512 MB).
pub const SYSTEM_MANAGED_SIZE: u64 = 512 * 1024 * 1024;

/// Size of the system-reserved virtual memory region (3232 MB).
pub const SYSTEM_RESERVED_SIZE: u64 = 3232 * 1024 * 1024;

/// Size of the user virtual memory region (128 GB).
pub const USER_SIZE: u64 = 128 * 1024 * 1024 * 1024;

/// Aligns a value up to the given alignment.
#[inline]
pub const fn align_up(value: u64, alignment: u64) -> u64 {
    let mask = alignment - 1;
    (value + mask) & !mask
}

/// Aligns a value down to the given alignment.
#[inline]
pub const fn align_down(value: u64, alignment: u64) -> u64 {
    value & !(alignment - 1)
}

/// Checks if a value is aligned to the given alignment.
#[inline]
pub const fn is_aligned(value: u64, alignment: u64) -> bool {
    (value & (alignment - 1)) == 0
}

/// Human-readable size formatting.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 4096), 0);
        assert_eq!(align_up(1, 4096), 4096);
        assert_eq!(align_up(4096, 4096), 4096);
        assert_eq!(align_up(4097, 4096), 8192);
        assert_eq!(align_up(100, PAGE_SIZE), PAGE_SIZE);
    }

    #[test]
    fn test_align_down() {
        assert_eq!(align_down(0, 4096), 0);
        assert_eq!(align_down(1, 4096), 0);
        assert_eq!(align_down(4096, 4096), 4096);
        assert_eq!(align_down(8000, 4096), 4096);
    }

    #[test]
    fn test_is_aligned() {
        assert!(is_aligned(0, 4096));
        assert!(is_aligned(4096, 4096));
        assert!(!is_aligned(1, 4096));
        assert!(!is_aligned(4097, 4096));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1048576), "1.00 MB");
        assert_eq!(format_size(1073741824), "1.00 GB");
    }
}
