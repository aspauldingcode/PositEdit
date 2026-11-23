# Test matrix

- Baseline: existing scrollview ordering, animations, focus, mouse, keyboard, resize, small terminals, 100 panes.
- DLC 1/2: Build/Run/Debug/Test/Configure, browser, phone, Android as available; no duplicate renderer.
- DLC 3: Tab On bottom-right default, click/toggle, Tab accept, ghost text, no status collision.
- Spatial: overview jump and exit restore; workspace snapshots with missing file and crash recovery.
- Structural: Unicode, CRLF, comments, nested nodes, overlapping edits, undo/redo.
- DAP: paused/running/stale frames, readMemory unsupported, watch side effects denied.
- Trace: event order, missing events, retention, recorded replay vs reverse-debug labels.
- Output: ANSI injection, invalid UTF-8, huge output, cancellation, source mapping.
- Graphics: Kitty/Sixel/iTerm2 negotiation, no support fallback, resize, pane clipping, cleanup.
- Security: workspace trust, malicious SVG, path traversal, redaction, process permissions.
- Performance: idle CPU, memory, p95 typing, animation FPS, image preview cold start.
- Cross-platform: macOS M1 actual; Linux/NixOS where available; Windows capability report.
