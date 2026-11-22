//! Spatial intelligence services for the host TUI.
//! Returns declarative outcomes. Never writes terminal escape sequences.
pub mod causal;
pub mod impact;
pub mod lenses;
pub mod minimap;
pub mod output;
pub mod overview;
pub mod preview;
pub mod probes;
pub mod relations;
pub mod snapshot;
pub mod structural;
pub mod timeline;

pub use causal::{CausalEdge, CausalGraph, CausalNav};
pub use impact::{ChangeImpact, ImpactRanker};
pub use lenses::{LensKind, LensSession, LensStack};
pub use minimap::{MinimapCache, MinimapCell, MinimapPriority};
pub use output::{OutputBlock, OutputStore, RawChunk};
pub use overview::{OverviewJump, PaneLabel, SpatialOverview};
pub use preview::{PreviewKind, PreviewRequest, SurfaceDescriptor};
pub use probes::{ProbeFreshness, ProbeSnapshot, ProbeStore};
pub use relations::{PaneKind, PaneRelation, RelationGraph};
pub use snapshot::{PaneSnap, SnapshotStore, WorkspaceSnapshot};
pub use structural::{StructuralEdit, StructuralOp, StructuralPreview};
pub use timeline::{ReverseDebugLabel, TraceEvent, TraceStore};

pub const COMPONENT: &str = "ide-spatial";
pub const IMPLEMENTED: bool = true;
pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    NotInstalled,
    Ready,
    Unsupported,
}

pub fn status() -> Availability {
    Availability::Ready
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpatialCapabilities {
    pub kitty_graphics: bool,
    pub sixel: bool,
    pub iterm2_inline: bool,
    pub dap_step_back: bool,
    pub dap_read_memory: bool,
    pub tree_sitter: bool,
    pub browser_runtime: bool,
    pub phone_runtime: bool,
    pub android_runtime: bool,
    pub syscall_monitor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpatialContext {
    pub workspace_id: String,
    pub pane_id: usize,
    pub document_revision: u64,
    pub trusted: bool,
    pub request_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpatialAction {
    OpenOverview,
    JumpOverview { index: usize },
    CloseSpatial,
    OpenLens { kind: LensKind },
    CloseLens,
    SaveSnapshot,
    RestoreSnapshot,
    ShowImpact,
    StructuralPreview { op: StructuralOp },
    ApplyStructural,
    ToggleTimeline,
    ClearTrace,
    AttachRelation { target: PaneKind },
    FocusRelated { target: PaneKind },
    OpenPreview { kind: PreviewKind },
    ClosePreview,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpatialOutcome {
    None,
    Notice(String),
    OpenOverview(SpatialOverview),
    JumpToPane { index: usize },
    RestoreFocus {
        focus: usize,
        view: i32,
        editing: bool,
    },
    OpenLens(LensSession),
    SnapshotSaved(String),
    SnapshotRestored(WorkspaceSnapshot),
    Impact(ChangeImpact),
    Structural(StructuralPreview),
    ApplyEdits {
        revision: u64,
        edits: Vec<ide_core::Edit>,
    },
    Timeline(Vec<TraceEvent>),
    OpenPreview(SurfaceDescriptor),
    ClosePreview,
    FocusRelation { target: PaneKind },
}

#[derive(Clone, Debug)]
pub struct SpatialSession {
    pub capabilities: SpatialCapabilities,
    pub overview: Option<SpatialOverview>,
    pub saved_focus: Option<(usize, i32, bool)>,
    pub lenses: LensStack,
    pub snapshots: SnapshotStore,
    pub relations: RelationGraph,
    pub minimap: MinimapCache,
    pub probes: ProbeStore,
    pub output: OutputStore,
    pub causal: CausalNav,
    pub impact: ImpactRanker,
    pub traces: TraceStore,
    pub preview: Option<SurfaceDescriptor>,
    pub pending_structural: Option<StructuralPreview>,
    pub active_mode: SpatialMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpatialMode {
    #[default]
    Idle,
    Overview,
    Lens,
    Timeline,
    Impact,
    Structural,
    Preview,
}

impl Default for SpatialSession {
    fn default() -> Self {
        Self::new(SpatialCapabilities::default())
    }
}

impl SpatialSession {
    pub fn new(capabilities: SpatialCapabilities) -> Self {
        Self {
            capabilities,
            overview: None,
            saved_focus: None,
            lenses: LensStack::default(),
            snapshots: SnapshotStore::default(),
            relations: RelationGraph::default(),
            minimap: MinimapCache::default(),
            probes: ProbeStore::default(),
            output: OutputStore::default(),
            causal: CausalNav::default(),
            impact: ImpactRanker::default(),
            traces: TraceStore::default(),
            preview: None,
            pending_structural: None,
            active_mode: SpatialMode::Idle,
        }
    }

    pub fn dispatch(&mut self, action: SpatialAction, ctx: &SpatialContext) -> SpatialOutcome {
        match action {
            SpatialAction::OpenOverview => {
                let overview = self
                    .overview
                    .clone()
                    .unwrap_or_else(|| SpatialOverview::empty(ctx.workspace_id.clone()));
                self.active_mode = SpatialMode::Overview;
                SpatialOutcome::OpenOverview(overview)
            }
            SpatialAction::JumpOverview { index } => {
                if let Some(overview) = &self.overview {
                    if index < overview.panes.len() {
                        return SpatialOutcome::JumpToPane { index };
                    }
                }
                SpatialOutcome::Notice("overview: pane out of range".into())
            }
            SpatialAction::CloseSpatial => self.close_spatial(),
            SpatialAction::OpenLens { kind } => {
                if kind == LensKind::Syscall && !self.capabilities.syscall_monitor {
                    return SpatialOutcome::Notice(
                        "syscall lens unavailable: no authorized monitor".into(),
                    );
                }
                let session = self.lenses.push(kind, ctx.pane_id);
                self.active_mode = SpatialMode::Lens;
                SpatialOutcome::OpenLens(session)
            }
            SpatialAction::CloseLens => {
                self.lenses.pop();
                if self.lenses.is_empty() {
                    self.active_mode = SpatialMode::Idle;
                    self.close_spatial()
                } else {
                    SpatialOutcome::OpenLens(self.lenses.current().cloned().unwrap())
                }
            }
            SpatialAction::SaveSnapshot => {
                let path = self.snapshots.last_path(&ctx.workspace_id);
                SpatialOutcome::SnapshotSaved(path)
            }
            SpatialAction::RestoreSnapshot => match self.snapshots.last_loaded() {
                Some(snap) => {
                    self.active_mode = SpatialMode::Idle;
                    SpatialOutcome::SnapshotRestored(snap.clone())
                }
                None => SpatialOutcome::Notice("no workspace snapshot".into()),
            },
            SpatialAction::ShowImpact => {
                self.active_mode = SpatialMode::Impact;
                SpatialOutcome::Impact(self.impact.current())
            }
            SpatialAction::StructuralPreview { op } => {
                if let Some(preview) = &self.pending_structural {
                    if preview.op == op {
                        self.active_mode = SpatialMode::Structural;
                        return SpatialOutcome::Structural(preview.clone());
                    }
                }
                SpatialOutcome::Notice("structural: no preview ready".into())
            }
            SpatialAction::ApplyStructural => {
                if let Some(preview) = self.pending_structural.take() {
                    if preview.ambiguous {
                        return SpatialOutcome::Notice("structural: ambiguous edit rejected".into());
                    }
                    self.active_mode = SpatialMode::Idle;
                    SpatialOutcome::ApplyEdits {
                        revision: preview.revision,
                        edits: preview.edits,
                    }
                } else {
                    SpatialOutcome::Notice("structural: nothing to apply".into())
                }
            }
            SpatialAction::ToggleTimeline => {
                self.active_mode = SpatialMode::Timeline;
                SpatialOutcome::Timeline(self.traces.events().to_vec())
            }
            SpatialAction::ClearTrace => {
                self.traces.clear();
                SpatialOutcome::Notice("trace cleared".into())
            }
            SpatialAction::AttachRelation { target } => {
                self.relations
                    .attach(ctx.pane_id, target, ctx.document_revision);
                SpatialOutcome::Notice(format!("related to {target:?}"))
            }
            SpatialAction::FocusRelated { target } => SpatialOutcome::FocusRelation { target },
            SpatialAction::OpenPreview { kind } => {
                let surface = preview::describe(kind, &self.capabilities);
                self.preview = Some(surface.clone());
                self.active_mode = SpatialMode::Preview;
                SpatialOutcome::OpenPreview(surface)
            }
            SpatialAction::ClosePreview => {
                self.preview = None;
                self.active_mode = SpatialMode::Idle;
                SpatialOutcome::ClosePreview
            }
        }
    }

    fn close_spatial(&mut self) -> SpatialOutcome {
        self.active_mode = SpatialMode::Idle;
        self.overview = None;
        self.lenses.clear();
        self.preview = None;
        if let Some((focus, view, editing)) = self.saved_focus.take() {
            SpatialOutcome::RestoreFocus {
                focus,
                view,
                editing,
            }
        } else {
            SpatialOutcome::None
        }
    }

    pub fn remember_focus(&mut self, focus: usize, view: i32, editing: bool) {
        if self.saved_focus.is_none() {
            self.saved_focus = Some((focus, view, editing));
        }
    }

    pub fn set_overview(&mut self, overview: SpatialOverview) {
        self.overview = Some(overview);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SpatialContext {
        SpatialContext {
            workspace_id: "ws".into(),
            pane_id: 0,
            document_revision: 1,
            trusted: true,
            request_id: 1,
        }
    }

    #[test]
    fn overview_jump_and_escape_restore() {
        let mut session = SpatialSession::default();
        session.remember_focus(2, 40, false);
        session.set_overview(SpatialOverview {
            workspace_id: "ws".into(),
            fits_viewport: false,
            panes: vec![
                PaneLabel {
                    index: 0,
                    name: "a".into(),
                    visible: true,
                },
                PaneLabel {
                    index: 1,
                    name: "b".into(),
                    visible: true,
                },
                PaneLabel {
                    index: 2,
                    name: "c".into(),
                    visible: false,
                },
            ],
        });
        let open = session.dispatch(SpatialAction::OpenOverview, &ctx());
        assert!(matches!(open, SpatialOutcome::OpenOverview(_)));
        let jump = session.dispatch(SpatialAction::JumpOverview { index: 1 }, &ctx());
        assert_eq!(jump, SpatialOutcome::JumpToPane { index: 1 });
        let close = session.dispatch(SpatialAction::CloseSpatial, &ctx());
        assert_eq!(
            close,
            SpatialOutcome::RestoreFocus {
                focus: 2,
                view: 40,
                editing: false
            }
        );
    }

    #[test]
    fn lens_stack_bounds_and_syscall_gate() {
        let mut session = SpatialSession::default();
        let denied = session.dispatch(
            SpatialAction::OpenLens {
                kind: LensKind::Syscall,
            },
            &ctx(),
        );
        assert!(matches!(denied, SpatialOutcome::Notice(_)));
        session.capabilities.syscall_monitor = true;
        for _ in 0..4 {
            let _ = session.dispatch(
                SpatialAction::OpenLens {
                    kind: LensKind::Symbols,
                },
                &ctx(),
            );
        }
        assert!(session.lenses.len() <= 3);
    }

    #[test]
    fn contract_version_is_one() {
        assert_eq!(CONTRACT_VERSION, 1);
    }
}
