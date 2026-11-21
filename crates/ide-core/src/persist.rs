//! Atomic save, newline preservation, and crash-recovery snapshots.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Newline {
    Lf,
    Crlf,
    /// Mixed or legacy lone CR. The rope keeps the original bytes' text unchanged.
    Raw,
}

#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    Symlink,
    InvalidEncoding,
    CorruptRecovery,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(err) => write!(f, "save failed: {err}"),
            SaveError::Symlink => write!(f, "refusing to replace a symlink"),
            SaveError::InvalidEncoding => write!(f, "file is not valid UTF-8"),
            SaveError::CorruptRecovery => write!(f, "recovery snapshot is corrupt"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<io::Error> for SaveError {
    fn from(err: io::Error) -> Self {
        SaveError::Io(err)
    }
}

pub fn classify_newlines(raw: &str) -> (Newline, String) {
    let has_crlf = raw.contains("\r\n");
    let stripped = raw.replace("\r\n", "");
    let has_other = stripped.contains('\n') || stripped.contains('\r');
    if has_crlf && !has_other {
        (Newline::Crlf, raw.replace("\r\n", "\n"))
    } else if !has_crlf && !stripped.contains('\r') {
        (Newline::Lf, raw.to_string())
    } else {
        (Newline::Raw, raw.to_string())
    }
}

impl Document {
    pub fn newline(&self) -> Newline {
        self.newline
    }

    pub fn from_disk_text(raw: &str) -> Self {
        let (newline, logical) = classify_newlines(raw);
        let mut doc = Self::new(&logical);
        doc.newline = newline;
        doc
    }

    pub fn open(path: &Path) -> Result<Self, SaveError> {
        let bytes = fs::read(path)?;
        let raw = std::str::from_utf8(&bytes).map_err(|_| SaveError::InvalidEncoding)?;
        Ok(Self::from_disk_text(raw))
    }

    pub fn encoded_text(&self) -> String {
        let logical = self.to_text();
        match self.newline {
            Newline::Crlf => logical.replace('\n', "\r\n"),
            Newline::Lf | Newline::Raw => logical,
        }
    }

    /// Write the encoded buffer by rename in the destination directory.
    /// An existing symlink is left untouched.
    pub fn save_atomic(&self, path: &Path) -> Result<(), SaveError> {
        let existing_mode = match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(SaveError::Symlink),
            Ok(meta) => Some(meta.permissions()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => return Err(err.into()),
        };
        let tmp = temp_path(path);
        let write_result = (|| -> Result<(), SaveError> {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            file.write_all(self.encoded_text().as_bytes())?;
            file.sync_all()?;
            #[cfg(unix)]
            if let Some(perms) = existing_mode {
                fs::set_permissions(&tmp, perms)?;
            }
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        write_result
    }

    pub fn write_recovery(&self, path: &Path) -> Result<(), SaveError> {
        let body = self.to_text();
        let newline = match self.newline {
            Newline::Lf => "lf",
            Newline::Crlf => "crlf",
            Newline::Raw => "raw",
        };
        let header = format!(
            "AXIOM-RECOVERY 1\n{}\n{}\n{}\n",
            self.revision(),
            newline,
            body.len()
        );
        let tmp = temp_path(path);
        let write_result = (|| -> Result<(), SaveError> {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            file.write_all(header.as_bytes())?;
            file.write_all(body.as_bytes())?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        write_result
    }

    pub fn load_recovery(path: &Path) -> Result<Self, SaveError> {
        let bytes = fs::read(path)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| SaveError::CorruptRecovery)?;
        let mut lines = text.splitn(5, '\n');
        let magic = lines.next().ok_or(SaveError::CorruptRecovery)?;
        if magic != "AXIOM-RECOVERY 1" {
            return Err(SaveError::CorruptRecovery);
        }
        let revision: u64 = lines
            .next()
            .ok_or(SaveError::CorruptRecovery)?
            .parse()
            .map_err(|_| SaveError::CorruptRecovery)?;
        let newline = match lines.next().ok_or(SaveError::CorruptRecovery)? {
            "lf" => Newline::Lf,
            "crlf" => Newline::Crlf,
            "raw" => Newline::Raw,
            _ => return Err(SaveError::CorruptRecovery),
        };
        let len: usize = lines
            .next()
            .ok_or(SaveError::CorruptRecovery)?
            .parse()
            .map_err(|_| SaveError::CorruptRecovery)?;
        let body = lines.next().ok_or(SaveError::CorruptRecovery)?;
        if body.len() != len {
            return Err(SaveError::CorruptRecovery);
        }
        let mut doc = Self::new(body);
        doc.newline = newline;
        doc.revision = revision;
        Ok(doc)
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("buffer");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = format!(".{name}.{}.{nanos}.axiom-tmp", std::process::id());
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(file_name),
        _ => PathBuf::from(file_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("axiom-core-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn crlf_round_trip_keeps_permissions_and_newlines() {
        let dir = scratch();
        let path = dir.join("notes.txt");
        fs::write(&path, "a\r\nb\r\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.newline(), Newline::Crlf);
        assert_eq!(doc.to_text(), "a\nb\n");
        doc.apply(
            0,
            &[crate::Edit {
                start: 0,
                end: 1,
                replacement: "A".into(),
            }],
        )
        .unwrap();
        doc.save_atomic(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"A\r\nb\r\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_save_does_not_replace_the_link() {
        let dir = scratch();
        let real = dir.join("real.txt");
        let link = dir.join("link.txt");
        fs::write(&real, "keep").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let doc = Document::new("changed");
        assert!(matches!(doc.save_atomic(&link), Err(SaveError::Symlink)));
        assert_eq!(fs::read(&real).unwrap(), b"keep");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_restores_revision_and_rejects_truncation() {
        let dir = scratch();
        let path = dir.join("buffer.recovery");
        let mut doc = Document::from_disk_text("one\r\ntwo");
        doc.apply(
            0,
            &[crate::Edit {
                start: 0,
                end: 0,
                replacement: "X".into(),
            }],
        )
        .unwrap();
        doc.write_recovery(&path).unwrap();
        let restored = Document::load_recovery(&path).unwrap();
        assert_eq!(restored.to_text(), "Xone\ntwo");
        assert_eq!(restored.revision(), 1);
        assert_eq!(restored.newline(), Newline::Crlf);
        fs::write(&path, "AXIOM-RECOVERY 1\n1\ncrlf\n4\nX").unwrap();
        assert!(matches!(
            Document::load_recovery(&path),
            Err(SaveError::CorruptRecovery)
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
