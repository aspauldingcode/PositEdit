# Canvas / editor performance fix (2026-11-21)

## Symptoms
- Browser/Phone Canvas felt laggy
- Editor keystroke/mouse response degraded while Canvas was open

## Causes
1. Capture worker re-ran expensive work every poll (`agent-browser open`/viewport, `terminal-phone up`+`describe`, screenshot) at 0.5–0.75s.
2. UI loop presented every wake (often 16ms via hover-hint “animating”), rewriting Canvas placeholders and sometimes re-uploading PNGs.
3. Post-click refresh spawned extra screenshot processes that fought the capture worker.

## Fixes
- Capture: navigate / phone-up / viewport / describe once; skip identical PNG bytes; idle 2.0s browser / 1.5s phone; nudge file for post-interaction refresh.
- UI: present only when dirty (input, animation, PTY output, new canvas frame, resize).
- Hover hint no longer forces 60fps; waits until the 1s deadline instead.
- `write_geom` and click mapping avoid per-frame disk thrash.
- Clicks nudge the worker instead of spawning oneshot captures.

## Verification
- `cargo test -p ide-tui --lib canvas::` → 6 passed
- `cargo build -p ide` → ok
