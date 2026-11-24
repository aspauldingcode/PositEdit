//! F11 live change impact. Never auto-runs builds or tests.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImpactItem {
    pub label: String,
    pub confidence: u8,
    pub evidence: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeImpact {
    pub revision: u64,
    pub items: Vec<ImpactItem>,
}

#[derive(Clone, Debug, Default)]
pub struct ImpactRanker {
    last: ChangeImpact,
    last_query_ms: u64,
}

impl ImpactRanker {
    pub fn current(&self) -> ChangeImpact {
        self.last.clone()
    }

    pub fn predict(
        &mut self,
        revision: u64,
        now_ms: u64,
        symbols: &[String],
        tests: &[String],
        targets: &[String],
    ) -> ChangeImpact {
        // Debounce: ignore predictions closer than 120ms unless revision advanced.
        if revision == self.last.revision && now_ms.saturating_sub(self.last_query_ms) < 120 {
            return self.last.clone();
        }
        self.last_query_ms = now_ms;
        let mut items = Vec::new();
        for symbol in symbols.iter().take(8) {
            items.push(ImpactItem {
                label: format!("symbol:{symbol}"),
                confidence: 70,
                evidence: "lsp/symbol index".into(),
            });
        }
        for test in tests.iter().take(8) {
            items.push(ImpactItem {
                label: format!("test:{test}"),
                confidence: 55,
                evidence: "project test graph".into(),
            });
        }
        for target in targets.iter().take(4) {
            items.push(ImpactItem {
                label: format!("target:{target}"),
                confidence: 40,
                evidence: "project DLC scan".into(),
            });
        }
        items.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(a.label.cmp(&b.label)));
        self.last = ChangeImpact { revision, items };
        self.last.clone()
    }
}

/// Pure ranking for Bend differential tests.
pub fn rank_score(confidence: u8, evidence_weight: u8) -> u16 {
    u16::from(confidence) * u16::from(evidence_weight.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_and_no_autorun() {
        let mut ranker = ImpactRanker::default();
        let first = ranker.predict(1, 0, &["foo".into()], &["foo_test".into()], &["build".into()]);
        assert_eq!(first.items.len(), 3);
        let second = ranker.predict(1, 50, &["bar".into()], &[], &[]);
        assert_eq!(second.items[0].label, "symbol:foo");
        let third = ranker.predict(2, 200, &["bar".into()], &[], &[]);
        assert_eq!(third.items[0].label, "symbol:bar");
        assert_eq!(rank_score(70, 2), 140);
    }
}
