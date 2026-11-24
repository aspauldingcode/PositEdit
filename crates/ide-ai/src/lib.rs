//! Provider-neutral agent router and tool policy.
//!
//! Network providers are selected by availability (OpenCode-style priority).
//! A FreeLocal backend runs allowlisted tools in-process with no cloud call.
//! Secrets are redacted before display. Edits stay revision-checked.
use ide_core::{BufferError, Document, Edit};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(&'static str),
}

/// Tools an agent may request. Shell is never on this list.
pub const ALLOWED_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "apply_edit",
    "run_task",
    "format",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// `IDE_AGENT` override when it resolves on PATH.
    EnvOverride,
    /// Cursor `agent` CLI.
    CursorAgent,
    /// OpenAI Codex CLI.
    Codex,
    /// Anthropic Claude Code CLI.
    Claude,
    /// OpenCode-style open router CLI when installed.
    OpenCode,
    /// In-process tool loop; no paid API required.
    FreeLocal,
}

impl BackendKind {
    pub fn program_name(self) -> Option<&'static str> {
        match self {
            BackendKind::EnvOverride => None,
            BackendKind::CursorAgent => Some("agent"),
            BackendKind::Codex => Some("codex"),
            BackendKind::Claude => Some("claude"),
            BackendKind::OpenCode => Some("opencode"),
            BackendKind::FreeLocal => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BackendKind::EnvOverride => "env",
            BackendKind::CursorAgent => "cursor-agent",
            BackendKind::Codex => "codex",
            BackendKind::Claude => "claude",
            BackendKind::OpenCode => "opencode",
            BackendKind::FreeLocal => "free-local",
        }
    }
}

/// Default routing order: explicit override, then common coding CLIs, then free tools.
pub const DEFAULT_ROUTE: &[BackendKind] = &[
    BackendKind::EnvOverride,
    BackendKind::CursorAgent,
    BackendKind::Codex,
    BackendKind::Claude,
    BackendKind::OpenCode,
    BackendKind::FreeLocal,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedBackend {
    pub kind: BackendKind,
    pub program: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteReport {
    pub chosen: ResolvedBackend,
    pub probed: Vec<(BackendKind, bool)>,
    pub elapsed_us: u128,
}

pub fn authorize(_root: &Path, tool: &str, target: &Path) -> Decision {
    if !ALLOWED_TOOLS.contains(&tool) {
        return Decision::Deny("tool is not allowlisted");
    }
    if matches!(tool, "list_dir" | "grep" | "run_task" | "format") {
        // These tools take a workspace-relative path or none; empty is ok for grep/format.
        if target.as_os_str().is_empty() {
            return Decision::Allow;
        }
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

/// Probe PATH (and `IDE_AGENT`) and pick the first usable backend.
pub fn resolve_route(which: impl Fn(&str) -> bool) -> RouteReport {
    let started = Instant::now();
    let mut probed = Vec::new();
    let env_name = std::env::var("IDE_AGENT").ok().filter(|name| !name.is_empty());

    for kind in DEFAULT_ROUTE {
        match kind {
            BackendKind::EnvOverride => {
                let ok = env_name.as_ref().is_some_and(|name| which(name));
                probed.push((*kind, ok));
                if ok {
                    let program = env_name.clone();
                    return RouteReport {
                        chosen: ResolvedBackend {
                            kind: *kind,
                            program,
                        },
                        probed,
                        elapsed_us: started.elapsed().as_micros(),
                    };
                }
            }
            BackendKind::FreeLocal => {
                probed.push((*kind, true));
                return RouteReport {
                    chosen: ResolvedBackend {
                        kind: *kind,
                        program: None,
                    },
                    probed,
                    elapsed_us: started.elapsed().as_micros(),
                };
            }
            other => {
                let name = other.program_name().unwrap_or("");
                let ok = !name.is_empty() && which(name);
                probed.push((*kind, ok));
                if ok {
                    return RouteReport {
                        chosen: ResolvedBackend {
                            kind: *other,
                            program: Some(name.to_string()),
                        },
                        probed,
                        elapsed_us: started.elapsed().as_micros(),
                    };
                }
            }
        }
    }

    RouteReport {
        chosen: ResolvedBackend {
            kind: BackendKind::FreeLocal,
            program: None,
        },
        probed,
        elapsed_us: started.elapsed().as_micros(),
    }
}

/// CLI argv for a resolved external coding agent.
pub fn cli_args(kind: BackendKind, program: &str, prompt: &str) -> Vec<String> {
    match kind {
        BackendKind::Codex => vec!["exec".to_string(), prompt.to_string()],
        BackendKind::OpenCode => vec!["run".to_string(), prompt.to_string()],
        BackendKind::EnvOverride if program == "codex" => {
            vec!["exec".to_string(), prompt.to_string()]
        }
        BackendKind::EnvOverride if program == "opencode" => {
            vec!["run".to_string(), prompt.to_string()]
        }
        _ => vec!["-p".to_string(), prompt.to_string()],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRequest {
    pub name: String,
    pub path: PathBuf,
    pub payload: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolOutcome {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub elapsed_us: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreeAgentResult {
    pub lines: Vec<String>,
    pub tools: Vec<ToolOutcome>,
    pub elapsed_us: u128,
}

/// True when the prompt is an explicit FreeLocal tool command (not a heuristic grep).
pub fn is_explicit_tool_prompt(prompt: &str) -> bool {
    let lower = prompt.trim().to_ascii_lowercase();
    lower == "list"
        || lower.starts_with("list ")
        || lower.starts_with("read ")
        || lower.starts_with("grep ")
        || lower == "format"
        || lower.starts_with("format ")
}

/// Parse a FreeLocal agent prompt into tool requests.
///
/// Supported forms (one per line or whole prompt):
/// - `read path`
/// - `list [path]`
/// - `grep needle [path]`
/// - `format`
/// - bare text → heuristic: grep first identifier-like token in workspace listing hint
pub fn parse_free_prompt(prompt: &str) -> Vec<ToolRequest> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = strip_cmd(&lower, trimmed, "read ") {
        return vec![ToolRequest {
            name: "read_file".into(),
            path: PathBuf::from(rest.trim()),
            payload: String::new(),
        }];
    }
    if lower == "list" || lower.starts_with("list ") {
        let rest = trimmed.get(4..).unwrap_or("").trim();
        return vec![ToolRequest {
            name: "list_dir".into(),
            path: PathBuf::from(if rest.is_empty() { "." } else { rest }),
            payload: String::new(),
        }];
    }
    if let Some(rest) = strip_cmd(&lower, trimmed, "grep ") {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let needle = parts.next().unwrap_or("").trim().to_string();
        let path = parts.next().unwrap_or(".").trim();
        return vec![ToolRequest {
            name: "grep".into(),
            path: PathBuf::from(if path.is_empty() { "." } else { path }),
            payload: needle,
        }];
    }
    if lower == "format" || lower.starts_with("format ") {
        return vec![ToolRequest {
            name: "format".into(),
            path: PathBuf::new(),
            payload: String::new(),
        }];
    }
    // Heuristic: treat the prompt as a workspace search.
    let needle = trimmed
        .split_whitespace()
        .find(|word| word.chars().any(|ch| ch.is_ascii_alphanumeric()))
        .unwrap_or(trimmed)
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .to_string();
    if needle.is_empty() {
        return Vec::new();
    }
    vec![ToolRequest {
        name: "grep".into(),
        path: PathBuf::from("."),
        payload: needle,
    }]
}

fn strip_cmd<'a>(lower: &str, original: &'a str, prefix: &str) -> Option<&'a str> {
    if lower.starts_with(prefix) {
        Some(&original[prefix.len()..])
    } else {
        None
    }
}

/// Execute FreeLocal tools against a workspace. Callers supply IO so tests stay hermetic.
pub fn run_free_local<R, L, G>(
    root: &Path,
    prompt: &str,
    mut read_file: R,
    mut list_dir: L,
    mut grep: G,
) -> FreeAgentResult
where
    R: FnMut(&Path) -> Result<String, String>,
    L: FnMut(&Path) -> Result<Vec<String>, String>,
    G: FnMut(&Path, &str) -> Result<Vec<String>, String>,
{
    let started = Instant::now();
    let requests = parse_free_prompt(prompt);
    let mut tools = Vec::new();
    let mut lines = Vec::new();
    if requests.is_empty() {
        lines.push("free-local: nothing to do".into());
        return FreeAgentResult {
            lines,
            tools,
            elapsed_us: started.elapsed().as_micros(),
        };
    }
    for request in requests {
        let tool_started = Instant::now();
        match authorize(root, &request.name, &request.path) {
            Decision::Deny(reason) => {
                tools.push(ToolOutcome {
                    name: request.name.clone(),
                    ok: false,
                    detail: reason.into(),
                    elapsed_us: tool_started.elapsed().as_micros(),
                });
                lines.push(format!("{} denied: {reason}", request.name));
                continue;
            }
            Decision::Allow => {}
        }
        let outcome = match request.name.as_str() {
            "read_file" => match read_file(&request.path) {
                Ok(text) => {
                    let preview = redact(&text);
                    let preview: Vec<_> = preview.lines().take(12).map(str::to_string).collect();
                    lines.extend(preview);
                    ToolOutcome {
                        name: request.name,
                        ok: true,
                        detail: "ok".into(),
                        elapsed_us: tool_started.elapsed().as_micros(),
                    }
                }
                Err(err) => {
                    lines.push(err.clone());
                    ToolOutcome {
                        name: request.name,
                        ok: false,
                        detail: err,
                        elapsed_us: tool_started.elapsed().as_micros(),
                    }
                }
            },
            "list_dir" => match list_dir(&request.path) {
                Ok(entries) => {
                    lines.extend(entries.into_iter().take(40));
                    ToolOutcome {
                        name: request.name,
                        ok: true,
                        detail: "ok".into(),
                        elapsed_us: tool_started.elapsed().as_micros(),
                    }
                }
                Err(err) => {
                    lines.push(err.clone());
                    ToolOutcome {
                        name: request.name,
                        ok: false,
                        detail: err,
                        elapsed_us: tool_started.elapsed().as_micros(),
                    }
                }
            },
            "grep" => match grep(&request.path, &request.payload) {
                Ok(hits) => {
                    if hits.is_empty() {
                        lines.push(format!("no matches for {}", request.payload));
                    } else {
                        lines.extend(hits.into_iter().take(40));
                    }
                    ToolOutcome {
                        name: request.name,
                        ok: true,
                        detail: "ok".into(),
                        elapsed_us: tool_started.elapsed().as_micros(),
                    }
                }
                Err(err) => {
                    lines.push(err.clone());
                    ToolOutcome {
                        name: request.name,
                        ok: false,
                        detail: err,
                        elapsed_us: tool_started.elapsed().as_micros(),
                    }
                }
            },
            "format" => {
                lines.push("format: use ff / Format Document in the editor".into());
                ToolOutcome {
                    name: request.name,
                    ok: true,
                    detail: "deferred".into(),
                    elapsed_us: tool_started.elapsed().as_micros(),
                }
            }
            other => {
                lines.push(format!("{other} not implemented in free-local"));
                ToolOutcome {
                    name: request.name,
                    ok: false,
                    detail: "unsupported".into(),
                    elapsed_us: tool_started.elapsed().as_micros(),
                }
            }
        };
        tools.push(outcome);
    }
    FreeAgentResult {
        lines,
        tools,
        elapsed_us: started.elapsed().as_micros(),
    }
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
    use std::collections::HashMap;
    use std::sync::Mutex;

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
        assert_eq!(
            authorize(root, "grep", Path::new("")),
            Decision::Allow
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

    #[test]
    fn router_prefers_available_cli_then_free_local() {
        let report = resolve_route(|name| name == "claude");
        assert_eq!(report.chosen.kind, BackendKind::Claude);
        assert_eq!(report.chosen.program.as_deref(), Some("claude"));
        assert!(report.probed.iter().any(|(k, ok)| *k == BackendKind::Claude && *ok));
        assert!(report.elapsed_us < 50_000);

        let free = resolve_route(|_| false);
        assert_eq!(free.chosen.kind, BackendKind::FreeLocal);
        assert!(free.chosen.program.is_none());
    }

    #[test]
    fn free_local_read_and_grep_are_fast_and_redacted() {
        let root = Path::new(".");
        let files = Mutex::new(HashMap::from([(
            PathBuf::from("secret.env"),
            "token sk-live-abc password=hunter2\n".to_string(),
        )]));
        let result = run_free_local(
            root,
            "read secret.env",
            |path| {
                files
                    .lock()
                    .unwrap()
                    .get(path)
                    .cloned()
                    .ok_or_else(|| "missing".into())
            },
            |_| Ok(vec!["secret.env".into()]),
            |_, needle| {
                Ok(files
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, text)| text.contains(needle))
                    .map(|(path, _)| path.display().to_string())
                    .collect())
            },
        );
        assert!(result.tools.iter().all(|t| t.ok), "{:?}", result.tools);
        assert!(result.lines.iter().any(|line| line.contains("[REDACTED]")));
        assert!(!result.lines.iter().any(|line| line.contains("sk-live")));
        assert!(result.elapsed_us < 20_000, "elapsed {}", result.elapsed_us);

        let grep = run_free_local(
            root,
            "grep sk-live .",
            |_| Err("no".into()),
            |_| Ok(vec![]),
            |_, _| Ok(vec!["secret.env:1".into()]),
        );
        assert_eq!(grep.tools[0].name, "grep");
        assert!(grep.tools[0].ok);
    }

    #[test]
    fn free_local_blocks_path_escape() {
        let result = run_free_local(
            Path::new("."),
            "read ../etc/passwd",
            |_| Ok("nope".into()),
            |_| Ok(vec![]),
            |_, _| Ok(vec![]),
        );
        assert!(!result.tools[0].ok);
        assert!(result.lines[0].contains("denied"));
    }

    #[test]
    fn cli_args_match_backend_conventions() {
        assert_eq!(
            cli_args(BackendKind::Codex, "codex", "hi"),
            vec!["exec".to_string(), "hi".to_string()]
        );
        assert_eq!(
            cli_args(BackendKind::OpenCode, "opencode", "hi"),
            vec!["run".to_string(), "hi".to_string()]
        );
        assert_eq!(
            cli_args(BackendKind::Claude, "claude", "hi"),
            vec!["-p".to_string(), "hi".to_string()]
        );
    }
}
