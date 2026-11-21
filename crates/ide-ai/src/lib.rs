//! Provider-neutral edit and tool policy. Network calls are not made here.
//! A tool path must stay inside the workspace, and secrets are redacted before display.
use ide_core::{BufferError, Document, Edit};
use std::path::{Component, Path};

pub const COMPONENT: &str = "ide-ai";
pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    NotInstalled,
    Ready,
    Unsupported,
}

pub fn status() -> Availability {
    Availability::Ready
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(&'static str),
}

const ALLOWED: &[&str] = &["read_file", "apply_edit"];

pub fn authorize(_root: &Path, tool: &str, target: &Path) -> Decision {
    if !ALLOWED.contains(&tool) {
        return Decision::Deny("tool is not allowlisted");
    }
    if !path_inside(target) {
        return Decision::Deny("path escapes the workspace");
    }
    Decision::Allow
}

pub fn redact(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = find_secret(rest) {
        output.push_str(&rest[..start]);
        let token_len = secret_len(&rest[start..]);
        output.push_str("[REDACTED]");
        rest = &rest[start + token_len..];
    }
    output.push_str(rest);
    output
}

pub fn apply_agent_edit(
    document: &mut Document,
    expected: u64,
    start: usize,
    end: usize,
    replacement: &str,
) -> Result<u64, BufferError> {
    document
        .apply(
            expected,
            &[Edit {
                start,
                end,
                replacement: replacement.to_string(),
            }],
        )
        .map_err(BufferError::Edit)
}

fn path_inside(target: &Path) -> bool {
    let mut depth = 0_i32;
    for component in target.components() {
        match component {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            Component::Normal(_) => depth += 1,
            Component::RootDir | Component::Prefix(_) => return false,
            Component::CurDir => {}
        }
    }
    !target.is_absolute() && depth >= 0
}

fn find_secret(text: &str) -> Option<usize> {
    let markers = ["sk-", "ghp_", "github_pat_", "Bearer ", "password="];
    markers.iter().filter_map(|marker| text.find(marker)).min()
}

fn secret_len(text: &str) -> usize {
    let rest = if let Some(stripped) = text.strip_prefix("Bearer ") {
        stripped
    } else if let Some(stripped) = text.strip_prefix("password=") {
        stripped
    } else {
        text
    };
    let extra = text.len() - rest.len();
    extra + rest.find(char::is_whitespace).unwrap_or(rest.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_escape_and_shell_and_redacts_tokens() {
        let root = Path::new("/work");
        assert_eq!(
            authorize(root, "read_file", Path::new("src/main.rs")),
            Decision::Allow
        );
        assert_eq!(
            authorize(root, "read_file", Path::new("../secrets")),
            Decision::Deny("path escapes the workspace")
        );
        assert_eq!(
            authorize(root, "shell", Path::new("src/main.rs")),
            Decision::Deny("tool is not allowlisted")
        );
        let hidden = redact("token sk-live-secret and Bearer abcdef");
        assert!(!hidden.contains("sk-live"));
        assert!(!hidden.contains("abcdef"));
        assert!(hidden.contains("[REDACTED]"));
    }

    #[test]
    fn stale_agent_edit_does_not_change_the_buffer() {
        let mut document = Document::new("hello");
        let error = apply_agent_edit(&mut document, 4, 0, 1, "H");
        assert!(error.is_err());
        assert_eq!(document.to_text(), "hello");
        assert_eq!(document.revision(), 0);
        apply_agent_edit(&mut document, 0, 0, 5, "hi").unwrap();
        assert_eq!(document.to_text(), "hi");
    }
}
