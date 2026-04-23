//! PS4 filesystem HLE.
//!
//! Maps PS4 filesystem paths to the host filesystem:
//! - `/app0/` → game installation directory
//! - `/savedata0/` → save data directory
//! - `/dev/` → virtual device files (stubs)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use super::OrbisError;

/// Manages PS4 virtual filesystem mount points.
pub struct FileSystem {
    /// Mount points: PS4 path prefix → host path.
    mounts: HashMap<String, PathBuf>,
    /// Next available file descriptor.
    next_fd: i32,
    /// Open file descriptors: fd → host path.
    open_files: HashMap<i32, OpenFile>,
}

/// An open file tracked by the filesystem.
struct OpenFile {
    host_path: PathBuf,
    file: Option<fs::File>,
    position: u64,
}

impl FileSystem {
    pub fn new() -> Self {
        let fs = FileSystem {
            mounts: HashMap::new(),
            next_fd: 3, // 0=stdin, 1=stdout, 2=stderr
            open_files: HashMap::new(),
        };

        // Reserve standard file descriptors
        tracing::debug!("Filesystem initialized");
        fs
    }

    /// Mount a PS4 path to a host directory.
    pub fn mount(&mut self, ps4_path: &str, host_path: &Path) {
        tracing::info!(
            ps4 = ps4_path,
            host = %host_path.display(),
            "Filesystem mount"
        );
        self.mounts
            .insert(ps4_path.to_string(), host_path.to_path_buf());
    }

    /// Resolve a PS4 path to a host filesystem path.
    pub fn resolve_path(&self, ps4_path: &str) -> Option<PathBuf> {
        for (prefix, host_base) in &self.mounts {
            if ps4_path.starts_with(prefix) {
                let relative = &ps4_path[prefix.len()..];
                let relative = relative.trim_start_matches('/');
                return Some(host_base.join(relative));
            }
        }
        None
    }

    /// `sceKernelOpen` / `open` — Open a file.
    pub fn open(&mut self, path: &str, flags: i32, mode: u32) -> i32 {
        tracing::debug!(path, flags, mode, "sceKernelOpen");

        let host_path = match self.resolve_path(path) {
            Some(p) => p,
            None => {
                // Try as a device file
                if path.starts_with("/dev/") {
                    tracing::warn!(path, "Device file access (stub)");
                    let fd = self.next_fd;
                    self.next_fd += 1;
                    self.open_files.insert(
                        fd,
                        OpenFile {
                            host_path: PathBuf::from(path),
                            file: None,
                            position: 0,
                        },
                    );
                    return fd;
                }
                tracing::warn!(path, "Path not mounted");
                return OrbisError::ENOENT.into();
            }
        };

        match fs::File::open(&host_path) {
            Ok(file) => {
                let fd = self.next_fd;
                self.next_fd += 1;
                self.open_files.insert(
                    fd,
                    OpenFile {
                        host_path,
                        file: Some(file),
                        position: 0,
                    },
                );
                tracing::debug!(fd, host = %self.open_files[&fd].host_path.display(), "File opened");
                fd
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "Failed to open file");
                OrbisError::ENOENT.into()
            }
        }
    }

    /// `sceKernelClose` / `close` — Close a file descriptor.
    pub fn close(&mut self, fd: i32) -> i32 {
        if self.open_files.remove(&fd).is_some() {
            tracing::debug!(fd, "File closed");
            0
        } else {
            tracing::warn!(fd, "Bad file descriptor");
            OrbisError::EBADF.into()
        }
    }

    /// `sceKernelRead` / `read` — Read from a file descriptor.
    pub fn read(&mut self, fd: i32, buf: &mut [u8]) -> i64 {
        use std::io::Read;

        let open_file = match self.open_files.get_mut(&fd) {
            Some(f) => f,
            None => {
                tracing::warn!(fd, "Bad file descriptor for read");
                return OrbisError::EBADF.0 as i64;
            }
        };

        let file = match open_file.file.as_mut() {
            Some(f) => f,
            None => {
                tracing::warn!(fd, "Device file read (stub)");
                return 0;
            }
        };

        match file.read(buf) {
            Ok(n) => {
                open_file.position += n as u64;
                n as i64
            }
            Err(e) => {
                tracing::error!(fd, error = %e, "Read error");
                -1
            }
        }
    }

    /// `sceKernelWrite` / `write` — Write to a file descriptor.
    pub fn write(&mut self, fd: i32, buf: &[u8]) -> i64 {
        use std::io::Write;

        // Handle stdout/stderr
        if fd == 1 {
            let text = String::from_utf8_lossy(buf);
            print!("{}", text);
            return buf.len() as i64;
        }
        if fd == 2 {
            let text = String::from_utf8_lossy(buf);
            eprint!("{}", text);
            return buf.len() as i64;
        }

        let open_file = match self.open_files.get_mut(&fd) {
            Some(f) => f,
            None => {
                tracing::warn!(fd, "Bad file descriptor for write");
                return OrbisError::EBADF.0 as i64;
            }
        };

        let file = match open_file.file.as_mut() {
            Some(f) => f,
            None => return buf.len() as i64, // stub device
        };

        match file.write(buf) {
            Ok(n) => {
                open_file.position += n as u64;
                n as i64
            }
            Err(e) => {
                tracing::error!(fd, error = %e, "Write error");
                -1
            }
        }
    }

    /// Number of open file descriptors.
    pub fn open_count(&self) -> usize {
        self.open_files.len()
    }
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new()
    }
}
