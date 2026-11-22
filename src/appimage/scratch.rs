//! Private, unpredictably named staging files.
//!
//! `tempfile` is a dev-dependency only (keeping rustix out of the release GUI),
//! so the production path needs its own tiny primitive: `create_new` with a
//! random suffix and 0600, plus a drop guard that removes the file on any
//! error path. Predictable `.stage-<pid>-<epoch>` names were a planting target.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

pub struct TempFile {
    path: PathBuf,
    file: File,
    armed: bool,
}

impl TempFile {
    /// Create a new 0600 file directly in `dir` (which must already exist).
    pub fn create_in(dir: &Path, prefix: &str) -> Result<Self, String> {
        for _ in 0..16 {
            let path = dir.join(format!(".{prefix}-{}", random_hex()?));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file,
                        armed: true,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!(
                        "cannot create a staging file in {}: {e}",
                        dir.display()
                    ));
                }
            }
        }
        Err("could not create a unique staging file".to_string())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| e.to_string())
    }

    /// Atomically move the file to `dest`. The destination's final component
    /// is replaced (never followed), so a planted symlink is removed rather
    /// than written through.
    pub fn persist(mut self, dest: &Path) -> Result<(), String> {
        std::fs::rename(&self.path, dest)
            .map_err(|e| format!("cannot move staging file to {}: {e}", dest.display()))?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn random_hex() -> Result<String, String> {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes)
        .map_err(|e| format!("no randomness available for a staging name: {e}"))?;
    let mut out = String::with_capacity(24);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::TempFile;

    #[test]
    fn creates_private_files_and_removes_them_on_drop() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path;
        {
            let temp = TempFile::create_in(dir.path(), "stage").unwrap();
            path = temp.path().to_path_buf();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert!(
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".stage-")
            );
        }
        assert!(!path.exists(), "drop must remove an unpersisted file");
    }

    #[test]
    fn persist_moves_the_file() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("final.bin");
        let mut temp = TempFile::create_in(dir.path(), "stage").unwrap();
        temp.file_mut().write_all(b"payload").unwrap();
        temp.persist(&dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"payload");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
