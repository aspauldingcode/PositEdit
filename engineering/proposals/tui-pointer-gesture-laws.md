# Proposal: additive TUI pointer-gesture laws

## Summary

Add exclusive pointer-gesture state-machine laws under `bend/tui/` (not root
`LAWS.bend`). A non-Idle gesture stays sticky until mouse-up: scrollbar /
minimap scrolling must never re-arm tile or sidebar resize mid-drag.

## Why additive

Root `LAWS.bend` / `SPEC.bend` are accepted and frozen. Subsystem laws follow
the same pattern as `bend/flip`, `bend/scroll`, `bend/minimap`.

## Laws (see `bend/tui/LAWS.bend`)

- `idle_arms_scrollbar`
- `scroll_gesture_sticky` — Scrollbar + arm(TileResize) = Scrollbar
- `minimap_rejects_sidebar`
- `release_is_idle`
- `scrollbar_is_doc_scroll`

## Rust mirror

`crates/ide-tui/src/gesture.rs` + `EditorApp::pointer_gesture` exclusive arming
in `pointer()`.
