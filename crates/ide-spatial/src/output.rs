//! F06 structured output blocks with ANSI sanitization and path checks.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawChunk {
    pub process_id: u32,
    pub monotonic_ms: u64,
    pub stream: &'static str,
    pub bytes: Vec<u8>,
    pub exit_status: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputBlock {
    pub title: String,
    pub display: String,
    pub source_path: Option<String>,
    pub collapsed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct OutputStore {
    chunks: Vec<RawChunk>,
    blocks: Vec<OutputBlock>,
    spilled: usize,
}

const MAX_CHUNKS: usize = 1000;
const SPILL_BYTES: usize = 256 * 1024;

impl OutputStore {
    pub fn ingest(&mut self, chunk: RawChunk, workspace: &Path) {
        let display = sanitize_ansi(&chunk.bytes);
        let source_path = extract_path(&display).and_then(|p| {
            let candidate = PathBuf::from(&p);
            let full = if candidate.is_absolute() {
                candidate
            } else {
                workspace.join(&candidate)
            };
            let root = workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf());
            let Ok(canon) = full.canonicalize() else {
                return None;
            };
            if canon.starts_with(&root) {
                Some(p)
            } else {
                None
            }
        });
        let title = if display.contains("FAILED") || display.contains("error") {
            "diagnostic".into()
        } else if display.contains("test ") {
            "test".into()
        } else {
            chunk.stream.to_string()
        };
        self.blocks.push(OutputBlock {
            title,
            display: display.chars().take(400).collect(),
            source_path,
            collapsed: false,
        });
        self.chunks.push(chunk);
        while self.chunks.len() > MAX_CHUNKS {
            let removed = self.chunks.remove(0);
            self.spilled = self.spilled.saturating_add(removed.bytes.len());
            if !self.blocks.is_empty() {
                self.blocks.remove(0);
            }
        }
        let total: usize = self.chunks.iter().map(|c| c.bytes.len()).sum();
        if total > SPILL_BYTES {
            self.spilled = self.spilled.saturating_add(total - SPILL_BYTES);
        }
    }

    pub fn blocks(&self) -> &[OutputBlock] {
        &self.blocks
    }

    pub fn spilled(&self) -> usize {
        self.spilled
    }
}

pub fn sanitize_ansi(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if ch == '\0' {
            continue;
        }
        out.push(ch);
    }
    out
}

fn extract_path(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let trimmed = token.trim_matches(|c: char| c == ',' || c == '"' || c == '\'');
        let path = trimmed
            .split(':')
            .next()
            .unwrap_or(trimmed)
            .trim_matches(|c: char| c == '"' || c == '\'');
        if path.contains('/') && path.contains('.') {
            return Some(path.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn strips_ansi_and_rejects_escape_paths() {
        let cleaned = sanitize_ansi(b"\x1b[31merror\x1b[0m at src/main.rs:1");
        assert_eq!(cleaned, "error at src/main.rs:1");
        assert!(!cleaned.contains('\u{1b}'));

        let root = env::temp_dir().join(format!(
            "posit-out-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), b"fn main(){}\n").unwrap();
        let mut store = OutputStore::default();
        store.ingest(
            RawChunk {
                process_id: 1,
                monotonic_ms: 1,
                stream: "stderr",
                bytes: b"\x1b[31merror\x1b[0m src/main.rs:1".to_vec(),
                exit_status: Some(1),
            },
            &root,
        );
        assert_eq!(store.blocks()[0].source_path.as_deref(), Some("src/main.rs"));
        store.ingest(
            RawChunk {
                process_id: 1,
                monotonic_ms: 2,
                stream: "stderr",
                bytes: b"leak /etc/passwd:1".to_vec(),
                exit_status: None,
            },
            &root,
        );
        assert!(store.blocks()[1].source_path.is_none());
        let _ = fs::remove_dir_all(root);
    }
}
