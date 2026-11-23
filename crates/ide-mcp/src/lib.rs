//! Posit Edit MCP gate + control protocol for agent hosts.
//!
//! The live IDE listens on a Unix control socket. `posit-edit-mcp` speaks MCP
//! stdio to Cursor/Codex/Claude and proxies tool calls to that socket — so
//! agents drive buffers, runtimes, and snapshots without Computer Use / Ghostty.
use ide_ai::{authorize, Decision};
use std::path::Path;

mod protocol;

pub use protocol::*;

pub const COMPONENT: &str = "ide-mcp";
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
pub struct ToolCall {
    pub name: String,
    pub path: String,
    pub approved: bool,
}

pub fn evaluate(root: &Path, call: &ToolCall) -> Decision {
    if !call.approved {
        return Decision::Deny("tool call is not approved");
    }
    if CONTROL_TOOLS.contains(&call.name.as_str()) {
        // Control tools are session-scoped; path is informational only.
        return Decision::Allow;
    }
    authorize(root, &call.name, Path::new(&call.path))
}

/// Human-facing tool catalog for MCP `tools/list`.
pub fn tool_catalog() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "posit_ping",
            description: "Check whether a live Posit Edit session is reachable on the control socket.",
            schema: r#"{"type":"object","properties":{}}"#,
        },
        ToolDescriptor {
            name: "posit_status",
            description: "Session status: workspace, focus pane, notice, buffer, runtime dock, size.",
            schema: r#"{"type":"object","properties":{}}"#,
        },
        ToolDescriptor {
            name: "posit_buffers",
            description: "List open editor buffers with dirty/cursor/revision metadata.",
            schema: r#"{"type":"object","properties":{}}"#,
        },
        ToolDescriptor {
            name: "posit_read_buffer",
            description: "Read the focused buffer text (or buffer by index).",
            schema: r#"{"type":"object","properties":{"index":{"type":"integer","minimum":0}}}"#,
        },
        ToolDescriptor {
            name: "posit_snapshot",
            description: "Render the current IDE UI to a plain-text grid (no Ghostty/Computer Use needed).",
            schema: r#"{"type":"object","properties":{"width":{"type":"integer"},"height":{"type":"integer"}}}"#,
        },
        ToolDescriptor {
            name: "posit_open",
            description: "Open a workspace-relative or absolute file in the editor.",
            schema: r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        },
        ToolDescriptor {
            name: "posit_key",
            description: "Send a key to Posit Edit (esc, tab, enter, letters, ctrl-s, 6/7/8, …).",
            schema: r#"{"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}"#,
        },
        ToolDescriptor {
            name: "posit_command",
            description: "Run a named command: save, quit-request, help, terminal.toggle, fullscreen, …",
            schema: r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#,
        },
        ToolDescriptor {
            name: "posit_runtime",
            description: "Open an in-editor runtime dock panel: browser | phone | android.",
            schema: r#"{"type":"object","properties":{"kind":{"type":"string","enum":["browser","phone","android"]}},"required":["kind"]}"#,
        },
        ToolDescriptor {
            name: "posit_capture",
            description: "Write a text snapshot under engineering/evidence (or an explicit path inside the workspace).",
            schema: r#"{"type":"object","properties":{"path":{"type":"string"},"width":{"type":"integer"},"height":{"type":"integer"}}}"#,
        },
    ]
}

#[derive(Clone, Debug)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: &'static str,
}

pub fn tool_to_method(tool: &str) -> Option<&'static str> {
    match tool {
        "posit_ping" => Some("ping"),
        "posit_status" => Some("status"),
        "posit_buffers" => Some("buffers"),
        "posit_read_buffer" => Some("read_buffer"),
        "posit_snapshot" => Some("snapshot"),
        "posit_open" => Some("open"),
        "posit_key" => Some("key"),
        "posit_command" => Some("command"),
        "posit_runtime" => Some("runtime"),
        "posit_capture" => Some("capture"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_cannot_widen_the_workspace_or_the_tool_list() {
        let root = Path::new("/work");
        let outside = ToolCall {
            name: "read_file".into(),
            path: "../.ssh/id_rsa".into(),
            approved: true,
        };
        assert_eq!(
            evaluate(root, &outside),
            Decision::Deny("path escapes the workspace")
        );
        let shell = ToolCall {
            name: "shell".into(),
            path: "src/main.rs".into(),
            approved: true,
        };
        assert_eq!(
            evaluate(root, &shell),
            Decision::Deny("tool is not allowlisted")
        );
        let pending = ToolCall {
            name: "read_file".into(),
            path: "src/main.rs".into(),
            approved: false,
        };
        assert_eq!(
            evaluate(root, &pending),
            Decision::Deny("tool call is not approved")
        );
        let ok = ToolCall {
            name: "read_file".into(),
            path: "src/main.rs".into(),
            approved: true,
        };
        assert_eq!(evaluate(root, &ok), Decision::Allow);
        let control = ToolCall {
            name: "posit_status".into(),
            path: ".".into(),
            approved: true,
        };
        assert_eq!(evaluate(root, &control), Decision::Allow);
    }

    #[test]
    fn catalog_covers_control_tools() {
        let names: Vec<_> = tool_catalog().into_iter().map(|t| t.name).collect();
        for tool in CONTROL_TOOLS {
            assert!(names.contains(tool), "missing {tool}");
            assert!(tool_to_method(tool).is_some());
        }
    }
}
