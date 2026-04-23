//! Thread Local Storage (TLS) management for PS4 modules.
//!
//! Each thread has its own TLS block containing per-module data.
//! The FS segment register points to the Thread Control Block (TCB)
//! which contains pointers to TLS data via the DTV (Dynamic Thread Vector).

/// TLS image information for a loaded module.
#[derive(Debug, Clone, Default)]
pub struct TlsImage {
    /// Module TLS index.
    pub module_index: u32,
    /// Alignment requirement.
    pub align: u32,
    /// Size of initialized data.
    pub image_size: u32,
    /// Total size including BSS.
    pub total_size: u32,
    /// Virtual address of the TLS initialization image.
    pub image_addr: u64,
}

/// Manages TLS allocation for all threads.
#[derive(Debug, Default)]
pub struct TlsManager {
    /// TLS images for all modules.
    images: Vec<TlsImage>,
    /// Total static TLS size.
    total_static_size: usize,
}

impl TlsManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a module's TLS image.
    pub fn register_image(&mut self, image: TlsImage) {
        tracing::debug!(
            module_index = image.module_index,
            size = image.total_size,
            "TLS image registered"
        );
        self.total_static_size += image.total_size as usize;
        self.images.push(image);
    }

    /// Total static TLS size across all modules.
    pub fn total_static_size(&self) -> usize {
        self.total_static_size
    }

    /// Number of registered TLS images.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
}
