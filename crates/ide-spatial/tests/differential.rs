//! Language-neutral fixtures shared with bend/dlc4/policy.bend.

#[test]
fn bend_policy_fixtures_match_rust() {
    assert_eq!(ide_spatial::overview::clamp_jump(0, 0), None);
    assert_eq!(ide_spatial::overview::clamp_jump(2, 3), Some(2));
    assert_eq!(
        ide_spatial::minimap::pick_priority(true, true, true),
        ide_spatial::MinimapPriority::Debugger
    );
    assert_eq!(ide_spatial::lenses::next_nest(2), 3);
    assert_eq!(ide_spatial::lenses::next_nest(3), 3);
    assert_eq!(ide_spatial::impact::rank_score(70, 2), 140);
    let snap = ide_spatial::WorkspaceSnapshot {
        version: 1,
        workspace_id: "ws".into(),
        focus: 0,
        view: 0,
        panes: vec![],
        lens_history: vec![],
        saved_at: 0,
    };
    assert!(ide_spatial::snapshot::validate_snapshot(&snap));
}
