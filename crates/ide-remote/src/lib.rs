//! Remote session state. The transport is authenticated SSH, not an open TCP port.
//! Sequence numbers drop stale packets after a reconnect.
use std::path::{Component, Path};

pub const COMPONENT: &str = "ide-remote";
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
pub enum RemoteError {
    Unauthenticated,
    StaleSequence,
    PathEscape,
}

#[derive(Clone, Debug)]
pub struct RemoteSession {
    authenticated: bool,
    remote_root: String,
    last_seq: u64,
    buffer: String,
}

impl RemoteSession {
    pub fn new(remote_root: impl Into<String>) -> Self {
        Self {
            authenticated: false,
            remote_root: remote_root.into(),
            last_seq: 0,
            buffer: String::new(),
        }
    }

    pub fn authenticate(&mut self) {
        self.authenticated = true;
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn map_path(&self, relative: &Path) -> Result<String, RemoteError> {
        if !self.authenticated {
            return Err(RemoteError::Unauthenticated);
        }
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(RemoteError::PathEscape);
        }
        let root = self.remote_root.trim_end_matches('/');
        if relative.as_os_str().is_empty() {
            return Ok(root.to_string());
        }
        Ok(format!("{root}/{}", relative.display()))
    }

    pub fn apply(&mut self, seq: u64, text: &str) -> Result<(), RemoteError> {
        if !self.authenticated {
            return Err(RemoteError::Unauthenticated);
        }
        if seq <= self.last_seq {
            return Err(RemoteError::StaleSequence);
        }
        self.last_seq = seq;
        self.buffer = text.to_string();
        Ok(())
    }

    pub fn reconnect(&mut self, last_remote_seq: u64) {
        self.last_seq = last_remote_seq;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unauthenticated_and_stale_packets() {
        let mut session = RemoteSession::new("/work");
        assert_eq!(session.apply(1, "hi"), Err(RemoteError::Unauthenticated));
        session.authenticate();
        session.apply(1, "hello").unwrap();
        assert_eq!(session.apply(1, "older"), Err(RemoteError::StaleSequence));
        assert_eq!(session.buffer(), "hello");
        session.reconnect(4);
        assert_eq!(session.apply(4, "replay"), Err(RemoteError::StaleSequence));
        session.apply(5, "next").unwrap();
        assert_eq!(session.buffer(), "next");
    }

    #[test]
    fn path_mapping_stays_under_the_remote_root() {
        let mut session = RemoteSession::new("/work");
        session.authenticate();
        assert_eq!(
            session.map_path(Path::new("src/main.rs")).unwrap(),
            "/work/src/main.rs"
        );
        assert_eq!(
            session.map_path(Path::new("../etc/passwd")),
            Err(RemoteError::PathEscape)
        );
        assert_eq!(
            session.map_path(Path::new("/etc/passwd")),
            Err(RemoteError::PathEscape)
        );
    }
}
