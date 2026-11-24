//! F09 evidence-based causal navigation. Temporal correlation is not causation.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CausalEdge {
    pub from: String,
    pub to: String,
    pub evidence: String,
    pub confidence: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CausalGraph {
    pub edges: Vec<CausalEdge>,
}

#[derive(Clone, Debug, Default)]
pub struct CausalNav {
    graph: CausalGraph,
    breadcrumbs: Vec<String>,
}

impl CausalNav {
    pub fn graph(&self) -> &CausalGraph {
        &self.graph
    }

    pub fn breadcrumbs(&self) -> &[String] {
        &self.breadcrumbs
    }

    pub fn link(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        evidence: impl Into<String>,
        confidence: u8,
    ) {
        let evidence = evidence.into();
        if evidence.trim().is_empty() || confidence == 0 {
            return;
        }
        // Refuse pure temporal claims.
        let lower = evidence.to_ascii_lowercase();
        if lower.contains("happened around the same time")
            || lower.contains("temporal correlation")
            || lower == "nearby in time"
        {
            return;
        }
        let from = from.into();
        let to = to.into();
        self.graph.edges.push(CausalEdge {
            from: from.clone(),
            to: to.clone(),
            evidence,
            confidence,
        });
        if self.breadcrumbs.last().map(String::as_str) != Some(from.as_str()) {
            self.breadcrumbs.push(from);
        }
        self.breadcrumbs.push(to);
    }

    pub fn pop(&mut self) -> Option<String> {
        self.breadcrumbs.pop()
    }
}

/// Pure traversal for Bend differential tests.
pub fn reachable(edges: &[CausalEdge], start: &str) -> Vec<String> {
    let mut out = vec![start.to_string()];
    let mut frontier = vec![start.to_string()];
    while let Some(node) = frontier.pop() {
        for edge in edges {
            if edge.from == node && !out.contains(&edge.to) {
                out.push(edge.to.clone());
                frontier.push(edge.to.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_temporal_only_and_keeps_breadcrumbs() {
        let mut nav = CausalNav::default();
        nav.link("fail", "diag", "nearby in time", 90);
        assert!(nav.graph().edges.is_empty());
        nav.link("fail", "diag", "test assertion matched diagnostic code", 80);
        nav.link("diag", "src", "diagnostic points to source anchor", 90);
        assert_eq!(nav.breadcrumbs().len(), 3);
        assert_eq!(reachable(&nav.graph().edges, "fail"), vec!["fail", "diag", "src"]);
        assert_eq!(nav.pop().as_deref(), Some("src"));
    }
}
