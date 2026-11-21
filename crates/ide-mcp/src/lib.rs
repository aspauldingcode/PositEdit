//! Scoped MCP tool gate. Approval is necessary and not sufficient:
//! the path must stay in the workspace and the tool must be allowlisted.
use ide_ai::{authorize, Decision};
use std::path::Path;

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
    authorize(root, &call.name, Path::new(&call.path))
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
    }
}
