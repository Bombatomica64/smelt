//! The checker's file system: the in-memory program over the installed packages.
//!
//! The program's own files (Smelt's sources, generated declarations, the
//! vendored `@types/node`) are in memory, keyed under [`ROOT`]. A module the
//! program imports from an installed package (`node_modules/x`, with its
//! `package.json` `types` and `.d.ts` files) is not one of them, and `tsc`
//! would read it from disk. So a lookup the memory layer cannot answer falls
//! through to the real file system, but only for a path inside a
//! `node_modules` directory: the program's sources come from memory alone,
//! which is how a typed version laid over an installed file (in memory, at the
//! installed path) takes the place of what is on disk.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the file system is built by the program builder in a sibling module"
)]

use std::sync::Arc;
use std::time::SystemTime;

use tsrs_vfs::{Entries, FS, FileInfo};

use crate::project::ROOT;

/// The in-memory program, with installed packages read from disk.
pub(crate) struct LayeredFs {
    /// The program's files.
    memory: Arc<dyn FS>,
    /// The real file system.
    disk: &'static dyn FS,
}

impl LayeredFs {
    /// `memory` over the real file system.
    pub(crate) fn new(memory: Arc<dyn FS>) -> Self {
        Self {
            memory,
            disk: tsrs_vfs::osvfs::fs(),
        }
    }

    /// The disk path of a program path inside an installed package's tree
    /// (`/smelt/home/me/app/node_modules/geom/index.d.ts` →
    /// `/home/me/app/node_modules/geom/index.d.ts`), or `None`. The vendored
    /// declarations at `/smelt/node_modules` are the program's, not the disk's.
    fn disk_path(path: &str) -> Option<String> {
        let rest = path.strip_prefix(ROOT)?.strip_prefix('/')?;
        if rest.starts_with("node_modules/") || rest == "node_modules" {
            return None;
        }
        rest.split('/')
            .any(|segment| segment == "node_modules")
            .then(|| format!("/{rest}"))
    }
}

impl FS for LayeredFs {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.memory.use_case_sensitive_file_names()
    }

    fn file_exists(&self, path: &str) -> bool {
        self.memory.file_exists(path)
            || Self::disk_path(path).is_some_and(|disk| self.disk.file_exists(&disk))
    }

    fn read_file(&self, path: &str) -> Option<String> {
        self.memory
            .read_file(path)
            .or_else(|| Self::disk_path(path).and_then(|disk| self.disk.read_file(&disk)))
    }

    fn write_file(&self, path: &str, data: &str) -> Result<(), String> {
        self.memory.write_file(path, data)
    }

    fn append_file(&self, path: &str, data: &str) -> Result<(), String> {
        self.memory.append_file(path, data)
    }

    fn remove(&self, path: &str) -> Result<(), String> {
        self.memory.remove(path)
    }

    fn chtimes(&self, path: &str, a_time: SystemTime, m_time: SystemTime) -> Result<(), String> {
        self.memory.chtimes(path, a_time, m_time)
    }

    fn directory_exists(&self, path: &str) -> bool {
        self.memory.directory_exists(path)
            || Self::disk_path(path).is_some_and(|disk| self.disk.directory_exists(&disk))
    }

    fn get_accessible_entries(&self, path: &str) -> Entries {
        let mut entries = self.memory.get_accessible_entries(path);
        if let Some(disk) = Self::disk_path(path) {
            let found = self.disk.get_accessible_entries(&disk);
            for file in found.files {
                if !entries.files.contains(&file) {
                    entries.files.push(file);
                }
            }
            for directory in found.directories {
                if !entries.directories.contains(&directory) {
                    entries.directories.push(directory);
                }
            }
            entries.symlinks = match (entries.symlinks.take(), found.symlinks) {
                (Some(mut in_memory), Some(on_disk)) => {
                    in_memory.extend(on_disk);
                    Some(in_memory)
                }
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            };
        }
        entries
    }

    fn stat(&self, path: &str) -> Option<FileInfo> {
        self.memory
            .stat(path)
            .or_else(|| Self::disk_path(path).and_then(|disk| self.disk.stat(&disk)))
    }

    fn realpath(&self, path: &str) -> String {
        if self.memory.file_exists(path) || self.memory.directory_exists(path) {
            return self.memory.realpath(path);
        }
        match Self::disk_path(path) {
            Some(disk) if self.disk.file_exists(&disk) || self.disk.directory_exists(&disk) => {
                format!("{ROOT}{}", self.disk.realpath(&disk))
            }
            _ => self.memory.realpath(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LayeredFs;

    /// Only an installed package's tree is read from disk.
    #[test]
    fn only_installed_packages_fall_through() {
        assert_eq!(
            LayeredFs::disk_path("/smelt/home/me/app/node_modules/geom/index.d.ts").as_deref(),
            Some("/home/me/app/node_modules/geom/index.d.ts")
        );
        assert_eq!(LayeredFs::disk_path("/smelt/home/me/app/src/main.ts"), None);
        assert_eq!(
            LayeredFs::disk_path("/smelt/node_modules/@types/node/index.d.ts"),
            None
        );
        assert_eq!(LayeredFs::disk_path("/elsewhere/node_modules/x"), None);
    }
}
