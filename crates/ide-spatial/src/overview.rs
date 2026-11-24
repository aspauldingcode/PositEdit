//! F01 spatial overview built from host pane geometry.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneLabel {
    pub index: usize,
    pub name: String,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpatialOverview {
    pub workspace_id: String,
    pub panes: Vec<PaneLabel>,
    pub fits_viewport: bool,
}

impl SpatialOverview {
    pub fn empty(workspace_id: String) -> Self {
        Self {
            workspace_id,
            panes: Vec::new(),
            fits_viewport: true,
        }
    }

    pub fn from_panes(
        workspace_id: impl Into<String>,
        names: &[String],
        visible_count: usize,
    ) -> Self {
        let panes = names
            .iter()
            .enumerate()
            .map(|(index, name)| PaneLabel {
                index,
                name: name.clone(),
                visible: index < visible_count,
            })
            .collect::<Vec<_>>();
        let fits_viewport = panes.len() <= visible_count.max(1);
        Self {
            workspace_id: workspace_id.into(),
            panes,
            fits_viewport,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        if self.panes.is_empty() {
            return vec!["overview: empty workspace".into()];
        }
        if self.fits_viewport {
            return vec![format!(
                "overview: {} panes fit the viewport",
                self.panes.len()
            )];
        }
        let mut lines = vec![format!("overview {} panes", self.panes.len())];
        for pane in &self.panes {
            let mark = if pane.visible { '>' } else { ' ' };
            lines.push(format!("{mark} {:>3} {}", pane.index, pane.name));
        }
        lines
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverviewJump {
    pub index: usize,
}

/// Pure policy: clamp jump into range. Differential target for Bend.
pub fn clamp_jump(index: usize, count: usize) -> Option<usize> {
    if count == 0 {
        None
    } else if index < count {
        Some(index)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_one_hundred_panes() {
        let empty = SpatialOverview::from_panes("ws", &[], 1);
        assert!(empty.fits_viewport);
        assert_eq!(empty.lines().len(), 1);

        let one = SpatialOverview::from_panes("ws", &["a".into()], 1);
        assert!(one.fits_viewport);

        let names: Vec<String> = (0..100).map(|i| format!("buf{i}")).collect();
        let many = SpatialOverview::from_panes("ws", &names, 3);
        assert!(!many.fits_viewport);
        assert_eq!(many.panes.len(), 100);
        assert_eq!(clamp_jump(99, 100), Some(99));
        assert_eq!(clamp_jump(100, 100), None);
    }
}
