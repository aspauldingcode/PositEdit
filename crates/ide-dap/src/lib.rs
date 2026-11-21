//! DAP client. This is not JSON-RPC: requests, responses, and events share framing only.
//! Commands the adapter did not advertise are refused and do not change debugger state.
use serde_json::{json, Value};

pub const COMPONENT: &str = "ide-dap";
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
pub enum DapState {
    Idle,
    Initializing,
    Initialized,
    Configured,
    Running,
    Stopped,
    Exited,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DapCapabilities {
    pub supports_configuration_done: bool,
    pub supports_step_back: bool,
}

#[derive(Debug)]
pub struct DapSession {
    state: DapState,
    capabilities: DapCapabilities,
    threads: Vec<i64>,
    next_seq: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DapError {
    WrongState,
    Malformed,
    Unsupported(&'static str),
}

impl Default for DapSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DapSession {
    pub fn new() -> Self {
        Self {
            state: DapState::Idle,
            capabilities: DapCapabilities::default(),
            threads: Vec::new(),
            next_seq: 1,
        }
    }

    pub fn state(&self) -> DapState {
        self.state
    }

    pub fn threads(&self) -> &[i64] {
        &self.threads
    }

    pub fn initialize(&mut self) -> Result<String, DapError> {
        if self.state != DapState::Idle {
            return Err(DapError::WrongState);
        }
        self.state = DapState::Initializing;
        Ok(self.request("initialize", json!({"clientID": "posit-edit"})))
    }

    pub fn launch(&mut self, program: &str) -> Result<String, DapError> {
        if self.state != DapState::Initialized {
            return Err(DapError::WrongState);
        }
        if self.capabilities.supports_configuration_done {
            self.state = DapState::Configured;
        } else {
            self.state = DapState::Running;
        }
        Ok(self.request("launch", json!({"program": program})))
    }

    pub fn configuration_done(&mut self) -> Result<String, DapError> {
        if self.state != DapState::Configured {
            return Err(DapError::WrongState);
        }
        self.state = DapState::Running;
        Ok(self.request("configurationDone", json!({})))
    }

    pub fn step_back(&mut self) -> Result<String, DapError> {
        if self.state != DapState::Stopped {
            return Err(DapError::WrongState);
        }
        if !self.capabilities.supports_step_back {
            return Err(DapError::Unsupported("stepBack"));
        }
        self.state = DapState::Running;
        Ok(self.request("stepBack", json!({})))
    }

    pub fn push_adapter(&mut self, payload: &str) -> Result<(), DapError> {
        let value: Value = serde_json::from_str(payload).map_err(|_| DapError::Malformed)?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or(DapError::Malformed)?;
        match kind {
            "response" if value.get("command").and_then(Value::as_str) == Some("initialize") => {
                if self.state != DapState::Initializing {
                    return Err(DapError::WrongState);
                }
                let body = &value["body"];
                self.capabilities.supports_configuration_done = body
                    ["supportsConfigurationDoneRequest"]
                    .as_bool()
                    .unwrap_or(false);
                self.capabilities.supports_step_back =
                    body["supportsStepBack"].as_bool().unwrap_or(false);
                self.state = DapState::Initialized;
            }
            "event" => self.on_event(
                value.get("event").and_then(Value::as_str).unwrap_or(""),
                &value,
            )?,
            "response" if value.get("command").and_then(Value::as_str) == Some("threads") => {
                self.threads.clear();
                if let Some(items) = value["body"]["threads"].as_array() {
                    for thread in items {
                        if let Some(id) = thread["id"].as_i64() {
                            self.threads.push(id);
                        }
                    }
                }
            }
            "response" => {}
            _ => return Err(DapError::Malformed),
        }
        Ok(())
    }

    fn on_event(&mut self, name: &str, value: &Value) -> Result<(), DapError> {
        match name {
            "initialized" => {
                if self.state == DapState::Initializing {
                    self.state = DapState::Initialized;
                }
            }
            "stopped" => {
                if matches!(self.state, DapState::Running | DapState::Stopped) {
                    self.state = DapState::Stopped;
                }
            }
            "terminated" | "exited" => self.state = DapState::Exited,
            "thread" => {
                if let Some(id) = value["body"]["threadId"].as_i64() {
                    if !self.threads.contains(&id) {
                        self.threads.push(id);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn request(&mut self, command: &str, arguments: Value) -> String {
        let seq = self.next_seq;
        self.next_seq += 1;
        json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_waits_for_configuration_when_advertised() {
        let mut session = DapSession::new();
        assert!(session.launch("app").is_err());
        session.initialize().unwrap();
        session
            .push_adapter(r#"{"seq":1,"type":"response","command":"initialize","success":true,"body":{"supportsConfigurationDoneRequest":true}}"#)
            .unwrap();
        assert_eq!(session.state(), DapState::Initialized);
        session.launch("./app").unwrap();
        assert_eq!(session.state(), DapState::Configured);
        session.configuration_done().unwrap();
        assert_eq!(session.state(), DapState::Running);
        session
            .push_adapter(
                r#"{"seq":2,"type":"event","event":"stopped","body":{"reason":"breakpoint"}}"#,
            )
            .unwrap();
        assert_eq!(session.state(), DapState::Stopped);
        assert_eq!(session.step_back(), Err(DapError::Unsupported("stepBack")));
        assert_eq!(session.state(), DapState::Stopped);
        session
            .push_adapter(r#"{"seq":3,"type":"event","event":"terminated"}"#)
            .unwrap();
        assert_eq!(session.state(), DapState::Exited);
    }

    #[test]
    fn malformed_adapter_message_does_not_move_state() {
        let mut session = DapSession::new();
        session.initialize().unwrap();
        assert!(session.push_adapter("not-json").is_err());
        assert_eq!(session.state(), DapState::Initializing);
        let frame =
            ide_protocol::encode(br#"{"seq":1,"type":"event","event":"initialized"}"#).unwrap();
        let (frames, _) = ide_protocol::drain(&frame).unwrap();
        assert_eq!(frames.len(), 1);
    }
}
