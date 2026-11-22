//! F12 contextual pane relationships without forced rearrangement.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneKind {
    Terminal,
    Debug,
    Browser,
    Phone,
    Android,
    Test,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneRelation {
    pub source_pane: usize,
    pub target: PaneKind,
    pub revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RelationGraph {
    links: Vec<PaneRelation>,
}

impl RelationGraph {
    pub fn attach(&mut self, source_pane: usize, target: PaneKind, revision: u64) {
        if let Some(existing) = self
            .links
            .iter_mut()
            .find(|link| link.source_pane == source_pane && link.target == target)
        {
            existing.revision = revision;
            return;
        }
        self.links.push(PaneRelation {
            source_pane,
            target,
            revision,
        });
    }

    pub fn related(&self, source_pane: usize) -> Vec<PaneKind> {
        self.links
            .iter()
            .filter(|link| link.source_pane == source_pane)
            .map(|link| link.target)
            .collect()
    }

    pub fn clear_ephemeral(&mut self, source_pane: usize) {
        // Relations stay until the user dismisses; this only drops stale source panes.
        self.links.retain(|link| link.source_pane != source_pane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_is_idempotent_per_target() {
        let mut graph = RelationGraph::default();
        graph.attach(0, PaneKind::Browser, 1);
        graph.attach(0, PaneKind::Browser, 2);
        assert_eq!(graph.related(0), vec![PaneKind::Browser]);
        assert_eq!(graph.links[0].revision, 2);
    }
}
