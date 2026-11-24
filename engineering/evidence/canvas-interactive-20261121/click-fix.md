# Canvas browser click fix (2026-11-21)

## Root causes
1. `canvas_capture.py` called `agent-browser open <url>` on every ~0.75s poll, reloading the page and wiping click/form effects.
2. Browser gestures deferred all mouse work until MouseUp; missing Up events meant no click ever fired. Helper processes also raced.

## Fixes
- Capture navigates once per URL, then only screenshots + viewport sync.
- Browser click commits on pointer_down via a serial device queue (`move` → `down` → `up`).
- Click coordinates prefer Canvas `geom.json` pixel box (viewport), not a stale full-window screenshot size.

## Verification
- `cargo test -p ide-tui --lib canvas::` → 6 passed
- `cargo build -p ide` → ok
- Live `agent-browser mouse move/down/up` against example.com succeeded
