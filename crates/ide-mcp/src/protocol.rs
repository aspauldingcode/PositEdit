//! Line-delimited JSON control protocol between `posit-edit-mcp` and a live IDE.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Tools the MCP server may invoke against a live Posit Edit session.
pub const CONTROL_TOOLS: &[&str] = &[
    "posit_ping",
    "posit_status",
    "posit_buffers",
    "posit_read_buffer",
    "posit_snapshot",
    "posit_open",
    "posit_key",
    "posit_command",
    "posit_runtime",
    "posit_pointer",
    "posit_capture",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControlRequest {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControlResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusInfo {
    pub pid: u32,
    pub workspace: String,
    pub notice: String,
    pub focus: String,
    pub editing: bool,
    pub buffer_name: String,
    pub language: String,
    pub cursor: usize,
    pub revision: u64,
    pub dirty: bool,
    pub buffer_count: usize,
    pub terminal_open: bool,
    pub runtime_dock: Option<String>,
    pub width: u16,
    pub height: u16,
    /// Absolute vs hybrid relative line numbers in the editor gutter.
    #[serde(default)]
    pub relative_numbers: bool,
    /// Focused-editor minimap preference (`:minimap`).
    #[serde(default = "default_true")]
    pub minimap_on: bool,
    /// Canvas hold (`:freeze`) — last frame kept, uploads paused.
    #[serde(default)]
    pub canvas_frozen: bool,
    /// Live Unix control socket path (empty if unknown).
    #[serde(default)]
    pub control_socket: String,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BufferInfo {
    pub index: usize,
    pub name: String,
    pub language: String,
    pub dirty: bool,
    pub cursor: usize,
    pub revision: u64,
    pub focused: bool,
}

/// Socket + lock live under the same runtime directory as `ide-tui` instance lock.
pub fn control_socket_path() -> PathBuf {
    if let Ok(override_path) = std::env::var("POSIT_EDIT_CONTROL_SOCKET") {
        if !override_path.is_empty() {
            return PathBuf::from(override_path);
        }
    }
    // Prefer a live session pointer written next to the lock (cross-TMPDIR safe).
    if let Ok(pointer) = std::env::var("POSIT_EDIT_CONTROL_POINTER") {
        if !pointer.is_empty() {
            if let Ok(path) = std::fs::read_to_string(&pointer) {
                let path = path.trim();
                if !path.is_empty() {
                    return PathBuf::from(path);
                }
            }
        }
    }
    // Home cache pointer written by the live IDE (survives TMPDIR mismatches).
    if let Ok(home) = std::env::var("HOME") {
        let pointer = PathBuf::from(home)
            .join(".cache")
            .join("posit-edit")
            .join("control.path");
        if let Ok(path) = std::fs::read_to_string(&pointer) {
            let path = path.trim();
            if !path.is_empty() && PathBuf::from(path).exists() {
                return PathBuf::from(path);
            }
        }
    }
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return PathBuf::from(runtime).join("posit-edit.control.sock");
        }
    }
    let mut dir = std::env::temp_dir();
    dir.push(format!("posit-edit-{}", user_token()));
    // Prefer an existing session under this TMPDIR.
    let candidate = dir.join("control.sock");
    if candidate.exists() {
        return candidate;
    }
    if let Ok(path) = std::fs::read_to_string(dir.join("control.path")) {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    candidate
}

fn user_token() -> String {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() }.to_string()
    }
    #[cfg(not(unix))]
    {
        std::env::var("USER").unwrap_or_else(|_| "user".into())
    }
}

pub fn method_to_tool(method: &str) -> Option<&'static str> {
    match method {
        "ping" => Some("posit_ping"),
        "status" => Some("posit_status"),
        "buffers" => Some("posit_buffers"),
        "read_buffer" => Some("posit_read_buffer"),
        "snapshot" => Some("posit_snapshot"),
        "open" => Some("posit_open"),
        "key" => Some("posit_key"),
        "command" => Some("posit_command"),
        "runtime" => Some("posit_runtime"),
        "capture" => Some("posit_capture"),
        _ => None,
    }
}
