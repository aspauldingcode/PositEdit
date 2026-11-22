//! F03 inline DAP probes. Stale values stay marked; no side-effect watches.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeFreshness {
    Fresh,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeSnapshot {
    pub name: String,
    pub value: String,
    pub path: String,
    pub line: usize,
    pub frame_id: i64,
    pub stop_event: u64,
    pub timestamp_ms: u64,
    pub freshness: ProbeFreshness,
}

#[derive(Clone, Debug, Default)]
pub struct ProbeStore {
    probes: Vec<ProbeSnapshot>,
    last_stop: u64,
}

const MAX_PROBES: usize = 32;

impl ProbeStore {
    pub fn ingest_stopped(
        &mut self,
        stop_event: u64,
        timestamp_ms: u64,
        frame_id: i64,
        variables: &[(String, String, String, usize)],
        read_memory_supported: bool,
        memory: Option<(String, String)>,
    ) {
        for probe in &mut self.probes {
            probe.freshness = ProbeFreshness::Stale;
        }
        self.last_stop = stop_event;
        for (name, value, path, line) in variables.iter().take(MAX_PROBES) {
            self.probes.push(ProbeSnapshot {
                name: name.clone(),
                value: truncate(value, 120),
                path: path.clone(),
                line: *line,
                frame_id,
                stop_event,
                timestamp_ms,
                freshness: ProbeFreshness::Fresh,
            });
        }
        if let Some((address, bytes)) = memory {
            if read_memory_supported {
                self.probes.push(ProbeSnapshot {
                    name: format!("mem:{address}"),
                    value: truncate(&bytes, 120),
                    path: String::new(),
                    line: 0,
                    frame_id,
                    stop_event,
                    timestamp_ms,
                    freshness: ProbeFreshness::Fresh,
                });
            }
        }
        while self.probes.len() > MAX_PROBES {
            self.probes.remove(0);
        }
    }

    pub fn refuse_side_effect_watch(&self, expression: &str) -> bool {
        let lower = expression.to_ascii_lowercase();
        lower.contains("delete")
            || lower.contains("write")
            || lower.contains("remove(")
            || lower.contains("= ")
    }

    pub fn for_path(&self, path: &str) -> Vec<&ProbeSnapshot> {
        self.probes.iter().filter(|p| p.path == path).collect()
    }

    pub fn all(&self) -> &[ProbeSnapshot] {
        &self.probes
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut out: String = value.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_stale_and_gates_memory() {
        let mut store = ProbeStore::default();
        store.ingest_stopped(
            1,
            10,
            7,
            &[("x".into(), "1".into(), "main.rs".into(), 3)],
            false,
            Some(("0x1".into(), "dead".into())),
        );
        assert_eq!(store.all().len(), 1);
        store.ingest_stopped(2, 20, 7, &[("x".into(), "2".into(), "main.rs".into(), 3)], true, None);
        assert_eq!(store.all()[0].freshness, ProbeFreshness::Stale);
        assert_eq!(store.all()[1].freshness, ProbeFreshness::Fresh);
        assert!(store.refuse_side_effect_watch("obj.delete()"));
    }
}
