//! LSP client session. Messages are JSON-RPC over Content-Length frames.
//! Missing capabilities stay unsupported. Stale diagnostics are dropped.
use serde_json::{json, Value};

pub const COMPONENT: &str = "ide-lsp";
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
pub enum SessionState {
    New,
    Initializing,
    Ready,
    Shutdown,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub hover: bool,
    pub definition: bool,
    pub rename: bool,
    pub completion: bool,
    pub folding_range: bool,
    pub document_symbol: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub uri: String,
    pub version: i64,
    pub message: String,
}

/// One folding range from `textDocument/foldingRange` (0-based inclusive lines).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldingRange {
    pub start_line: usize,
    pub end_line: usize,
    pub kind: Option<String>,
}

/// Parse a JSON-RPC `result` payload for `textDocument/foldingRange`.
pub fn parse_folding_ranges(result: &Value) -> Vec<FoldingRange> {
    let Some(items) = result.as_array() else {
        return Vec::new();
    };
    let mut ranges = Vec::with_capacity(items.len());
    for item in items {
        let start = item
            .get("startLine")
            .and_then(Value::as_u64)
            .or_else(|| item.get("start_line").and_then(Value::as_u64));
        let end = item
            .get("endLine")
            .and_then(Value::as_u64)
            .or_else(|| item.get("end_line").and_then(Value::as_u64));
        let (Some(start), Some(end)) = (start, end) else {
            continue;
        };
        if end < start {
            continue;
        }
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string);
        ranges.push(FoldingRange {
            start_line: start as usize,
            end_line: end as usize,
            kind,
        });
    }
    ranges.sort_by_key(|r| (r.start_line, r.end_line));
    ranges.dedup_by(|a, b| a.start_line == b.start_line && a.end_line == b.end_line);
    ranges
}

#[derive(Debug)]
pub struct LspSession {
    state: SessionState,
    next_id: i64,
    document_version: i64,
    capabilities: Capabilities,
    diagnostics: Vec<Diagnostic>,
    cancelled: Vec<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LspError {
    WrongState,
    Malformed,
    Unsupported(&'static str),
}

impl Default for LspSession {
    fn default() -> Self {
        Self::new()
    }
}

impl LspSession {
    pub fn new() -> Self {
        Self {
            state: SessionState::New,
            next_id: 1,
            document_version: 0,
            capabilities: Capabilities::default(),
            diagnostics: Vec::new(),
            cancelled: Vec::new(),
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn initialize(&mut self, root: &str) -> Result<String, LspError> {
        if self.state != SessionState::New {
            return Err(LspError::WrongState);
        }
        self.state = SessionState::Initializing;
        let id = self.alloc_id();
        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {"rootUri": root, "capabilities": {}}
        })
        .to_string())
    }

    pub fn push_server(&mut self, payload: &str) -> Result<(), LspError> {
        let value: Value = serde_json::from_str(payload).map_err(|_| LspError::Malformed)?;
        if let Some(id) = value.get("id").and_then(Value::as_i64) {
            if self.cancelled.contains(&id) {
                return Ok(());
            }
        }
        if value.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
            return self.take_diagnostics(&value);
        }
        if value.get("id").is_some()
            && value.get("result").is_some()
            && self.state == SessionState::Initializing
        {
            self.capabilities = read_capabilities(&value["result"]);
            self.state = SessionState::Ready;
            return Ok(());
        }
        if value.get("error").is_some() && self.state == SessionState::Initializing {
            self.state = SessionState::Failed;
        }
        Ok(())
    }

    pub fn did_open(&mut self, uri: &str, text: &str) -> Result<String, LspError> {
        self.require_ready()?;
        self.document_version = 1;
        Ok(json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": uri, "languageId": "plaintext", "version": 1, "text": text}}
        })
        .to_string())
    }

    pub fn did_change(&mut self, uri: &str, text: &str) -> Result<String, LspError> {
        self.require_ready()?;
        self.document_version += 1;
        let version = self.document_version;
        Ok(json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}
        })
        .to_string())
    }

    pub fn hover(&mut self) -> Result<String, LspError> {
        self.require_ready()?;
        if !self.capabilities.hover {
            return Err(LspError::Unsupported("hover"));
        }
        let id = self.alloc_id();
        Ok(
            json!({"jsonrpc": "2.0", "id": id, "method": "textDocument/hover", "params": {}})
                .to_string(),
        )
    }

    pub fn completion(&mut self, uri: &str, line: u32, character: u32) -> Result<String, LspError> {
        self.require_ready()?;
        if !self.capabilities.completion {
            return Err(LspError::Unsupported("completion"));
        }
        let id = self.alloc_id();
        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }
        })
        .to_string())
    }

    pub fn folding_range(&mut self, uri: &str) -> Result<String, LspError> {
        self.require_ready()?;
        if !self.capabilities.folding_range {
            return Err(LspError::Unsupported("foldingRange"));
        }
        let id = self.alloc_id();
        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/foldingRange",
            "params": {"textDocument": {"uri": uri}}
        })
        .to_string())
    }

    pub fn definition(&mut self, uri: &str, line: u32, character: u32) -> Result<String, LspError> {
        self.require_ready()?;
        if !self.capabilities.definition {
            return Err(LspError::Unsupported("definition"));
        }
        let id = self.alloc_id();
        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/definition",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }
        })
        .to_string())
    }

    pub fn cancel(&mut self, id: i64) -> String {
        self.cancelled.push(id);
        json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": id}}).to_string()
    }

    pub fn shutdown(&mut self) -> Result<String, LspError> {
        self.require_ready()?;
        self.state = SessionState::Shutdown;
        let id = self.alloc_id();
        Ok(json!({"jsonrpc": "2.0", "id": id, "method": "shutdown", "params": null}).to_string())
    }

    fn alloc_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn require_ready(&self) -> Result<(), LspError> {
        if self.state == SessionState::Ready {
            Ok(())
        } else {
            Err(LspError::WrongState)
        }
    }

    fn take_diagnostics(&mut self, value: &Value) -> Result<(), LspError> {
        let params = &value["params"];
        let uri = params["uri"]
            .as_str()
            .ok_or(LspError::Malformed)?
            .to_string();
        let version = params["version"].as_i64().unwrap_or(self.document_version);
        if version < self.document_version {
            return Ok(());
        }
        self.diagnostics.clear();
        let items = params["diagnostics"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for item in items {
            let message = item["message"].as_str().unwrap_or("").to_string();
            self.diagnostics.push(Diagnostic {
                uri: uri.clone(),
                version,
                message,
            });
        }
        Ok(())
    }
}

fn read_capabilities(result: &Value) -> Capabilities {
    let caps = &result["capabilities"];
    Capabilities {
        hover: provider_true(caps.get("hoverProvider")),
        definition: provider_true(caps.get("definitionProvider")),
        rename: provider_true(caps.get("renameProvider")),
        completion: caps.get("completionProvider").is_some(),
        folding_range: provider_true(caps.get("foldingRangeProvider")),
        document_symbol: provider_true(caps.get("documentSymbolProvider")),
    }
}

fn provider_true(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_negotiates_capabilities_and_drops_stale_diagnostics() {
        let mut session = LspSession::new();
        assert!(session.did_open("file://a.rs", "fn").is_err());
        let request = session.initialize("file:///work").unwrap();
        assert!(request.contains("initialize"));
        session
            .push_server(
                r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"hoverProvider":true,"completionProvider":{},"foldingRangeProvider":true,"definitionProvider":true}}}"#,
            )
            .unwrap();
        assert_eq!(session.state(), SessionState::Ready);
        assert!(session.capabilities().hover);
        assert!(session.capabilities().completion);
        assert!(session.capabilities().folding_range);
        assert!(session.capabilities().definition);
        assert!(!session.capabilities().rename);
        assert!(session.hover().is_ok());
        assert!(session
            .completion("file://a.rs", 0, 2)
            .unwrap()
            .contains("textDocument/completion"));
        assert!(session
            .folding_range("file://a.rs")
            .unwrap()
            .contains("foldingRange"));
        assert!(session
            .definition("file://a.rs", 0, 0)
            .unwrap()
            .contains("definition"));
        session.did_open("file://a.rs", "fn main(){}").unwrap();
        session
            .did_change("file://a.rs", "fn main(){ let x = 1; }")
            .unwrap();
        session
            .push_server(r#"{"method":"textDocument/publishDiagnostics","params":{"uri":"file://a.rs","version":1,"diagnostics":[{"message":"stale"}]}}"#)
            .unwrap();
        assert!(session.diagnostics().is_empty());
        session
            .push_server(r#"{"method":"textDocument/publishDiagnostics","params":{"uri":"file://a.rs","version":2,"diagnostics":[{"message":"unused"}]}}"#)
            .unwrap();
        assert_eq!(session.diagnostics()[0].message, "unused");
        assert_eq!(
            session.completion("file://a.rs", 0, 0).ok().map(|_| ()),
            Some(())
        );
    }

    #[test]
    fn parse_folding_ranges_reads_lsp_payload() {
        let result = json!([
            {"startLine": 0, "endLine": 4, "kind": "region"},
            {"startLine": 10, "endLine": 12},
            {"startLine": 3, "endLine": 1}
        ]);
        let ranges = parse_folding_ranges(&result);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 4);
        assert_eq!(ranges[0].kind.as_deref(), Some("region"));
        assert_eq!(ranges[1].start_line, 10);
        assert_eq!(ranges[1].end_line, 12);
    }

    #[test]
    fn cancelled_response_is_ignored_and_frames_reassemble() {
        let mut session = LspSession::new();
        session.initialize("file:///work").unwrap();
        let cancel = session.cancel(1);
        assert!(cancel.contains("cancelRequest"));
        session
            .push_server(
                r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"hoverProvider":true}}}"#,
            )
            .unwrap();
        assert_eq!(session.state(), SessionState::Initializing);
        let payload = br#"{"jsonrpc":"2.0","id":2,"result":{"capabilities":{}}}"#;
        let frame = ide_protocol::encode(payload).unwrap();
        let mut partial = frame[..6].to_vec();
        assert!(ide_protocol::decode(&partial).unwrap().is_none());
        partial.extend_from_slice(&frame[6..]);
        let (frames, _) = ide_protocol::drain(&partial).unwrap();
        session
            .push_server(std::str::from_utf8(&frames[0]).unwrap())
            .unwrap();
        assert_eq!(session.state(), SessionState::Ready);
        assert!(session.push_server("{").is_err());
        assert_eq!(session.state(), SessionState::Ready);
    }
}
