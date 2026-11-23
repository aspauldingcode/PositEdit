# Twelve fully specified feature workstreams

## F01 Spatial overview
Use existing scrollview layout tree; transient overlay displays viewport, relative pane order, live labels, selection. Pan/jump without changing layout. Escape restores focus/scroll. If workspace fits viewport, overview stays unobtrusive. Animated transition uses existing animator. Test pane counts 0/1/100, resize, rapid focus switching.

## F02 Adaptive micro-minimap
2-column Unicode Braille/quadrant heatmap using document structure, diagnostics, Git, coverage and profiler. Auto-priority: active debugger > failing diagnostics > diff > structure. Hide automatically if viewport narrow or performance degraded. Cache by document revision; no full reparse per frame. Fallback ASCII.

## F03 Inline runtime/memory probes
Reuse DAP sessions. Bounded variable value snapshots with source anchors, pointer/memory adapters only if debugger supports readMemory; avoid arbitrary process reads. Values marked fresh/stale with timestamp, stop event, frame ID. Never execute side-effecting watch expressions without consent. Limit size and rate; inline overlays do not alter source.

## F04 Structural editing
Tree-sitter node selection, fold to signature, move/duplicate nodes only through lossless byte-range edit transactions, preserving comments, whitespace, CRLF and Unicode. Reject ambiguous edits and show preview. Undo/redo atomic, revision-safe. LSP symbol references optional, no silent formatter changes.

## F05 Execution timeline
Trace event ingestion and scrubber with source anchors. Baseline: recorded-event navigation, not reverse execution. True reverse debugging only via compatible recorder/debugger and explicit capability flag. Bound storage, redact secrets, support clear/export with confirmation. Tests for trace gaps and timestamp ordering.

## F06 Structured output blocks
Capture stdout/stderr as raw ordered chunks with process ID, monotonic time, exit status; parse diagnostics/test events into collapsible searchable blocks. Preserve original bytes and ANSI sanitization. Source links require validated paths. Backpressure and spill-to-disk quotas.

## F07 Ephemeral lenses
Call graph, dependency graph, symbol explorer, process tree, syscall monitor (where authorized), hex diff. Modal host overlay; no permanent sidebar. Save and restore focus, selection, scroll, keyboard mode and terminal graphics state. Escape closes; nested lenses bounded.

## F08 Universal rich preview
Extend existing DLC browser/phone/Android graphics pipeline; SVG, images, Markdown, LaTeX, PDF page previews, runtime framebuffers. Terminal capability negotiation Kitty/Sixel/iTerm2; fallback text. External rasterization processes supervised, sandboxed, lazy. One surface owner, clipping, resize, cleanup.

## F09 Causal navigation
Graph connects test failure -> diagnostic -> source anchor -> recent edit -> build target -> trace/probe when evidence exists. Confidence/source provenance visible. Never claim causation from mere temporal correlation. Breadcrumbs are navigable and reversible.

## F10 Workspace snapshots
Store open pane identities, geometry intent, scroll offsets, focus, active targets and lens history in versioned local state. Do not store credentials or volatile process handles. Restore missing files/devices gracefully. Debounced atomic writes, migrations, crash recovery, opt-out for sensitive workspaces.

## F11 Live change impact
Incremental dependency graph from LSP and project DLC; predict affected symbols/tests/targets on edits. Show confidence and evidence. Never auto-run builds/tests on every keystroke. Background low priority, debounce, cap resource use.

## F12 Contextual pane relationships
Attach source to debugger/test/browser/mobile pane via semantic relation without forced rearrangement. Opening related pane uses existing scrollview; auto-close only explicitly ephemeral panes. Preserve DLC3 Tab button and bottom-right status precedence.

## Defaults
No onboarding. Keyboard-first, mouse optional. All expensive features lazy. Active editing view remains clean. No unsolicited AI prompts. Auto-hide overlays in constrained terminals. Sensible defaults, optional simple accessibility toggles, no theming/config burden.
