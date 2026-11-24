//! F05 execution timeline. Recorded replay unless reverse debug is live.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub id: u64,
    pub timestamp_ms: u64,
    pub kind: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReverseDebugLabel {
    RecordedReplay,
    ReverseDebugLive,
}

#[derive(Clone, Debug)]
pub struct TraceStore {
    events: Vec<TraceEvent>,
    next_id: u64,
    retention: usize,
}

impl Default for TraceStore {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            next_id: 1,
            retention: 2000,
        }
    }
}

impl TraceStore {
    pub fn push(
        &mut self,
        timestamp_ms: u64,
        kind: impl Into<String>,
        path: Option<String>,
        line: Option<usize>,
        detail: impl Into<String>,
    ) {
        let detail = redact_secrets(&detail.into());
        self.events.push(TraceEvent {
            id: self.next_id,
            timestamp_ms,
            kind: kind.into(),
            path,
            line,
            detail,
        });
        self.next_id += 1;
        self.events.sort_by_key(|e| (e.timestamp_ms, e.id));
        while self.events.len() > self.retention {
            self.events.remove(0);
        }
    }

    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn label(step_back_advertised: bool, step_back_accepted: bool) -> ReverseDebugLabel {
        if step_back_advertised && step_back_accepted {
            ReverseDebugLabel::ReverseDebugLive
        } else {
            ReverseDebugLabel::RecordedReplay
        }
    }
}

/// Pure ordering check for Bend differential tests.
pub fn timestamps_ordered(events: &[TraceEvent]) -> bool {
    events.windows(2).all(|pair| pair[0].timestamp_ms <= pair[1].timestamp_ms)
}

fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for key in ["password=", "token=", "secret=", "api_key="] {
        if let Some(idx) = out.to_ascii_lowercase().find(key) {
            let end = out[idx..]
                .find(|c: char| c.is_whitespace() || c == ',' || c == '"')
                .map(|n| idx + n)
                .unwrap_or(out.len());
            out.replace_range(idx..end, &format!("{key}***"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_redacts_and_labels_honestly() {
        let mut store = TraceStore::default();
        store.push(20, "stop", Some("a.rs".into()), Some(1), "password=hunter2");
        store.push(10, "launch", None, None, "ok");
        assert!(timestamps_ordered(store.events()));
        assert!(store.events()[1].detail.contains("***"));
        assert_eq!(
            TraceStore::label(true, false),
            ReverseDebugLabel::RecordedReplay
        );
        assert_eq!(
            TraceStore::label(true, true),
            ReverseDebugLabel::ReverseDebugLive
        );
    }
}
