# Scroll / Canvas resize / Urgent pressure (2026-11-21)

## Why it felt dogshit

Host was already `kern.memorystatus_vm_pressure_level=2` (Urgent). Pressure handling was **not** the main lag — bad hot paths were:

1. **Every wheel tick cloned the whole file** via `scroll_limit()` → `text().split('\n').count()` (and worse for horizontal scroll).
2. **Every paint cloned the full document + all highlight spans** (`buffer.text()` + `highlight_spans()`).
3. **Minimap** called `document.to_text().lines().count()` and rebuilt a fresh cache every frame.
4. **Canvas open**: every editor present `Clear`ed the Canvas pane, then rewrote Kitty placeholders (and often re-uploaded PNG).
5. **Canvas resize drag**: `write_geom` could hit disk every seam move; Kitty already deferred, geom did not.
6. **Urgent notice** rewrote `self.notice` every 2s while elevated (UI noise / extra dirties).

## Fixes

- Rope `len_lines` / `char_to_line` for scroll limits and caret line.
- Paint borrows cached text/spans (no full clones).
- Cached max line width; revision-cached minimap; skip minimap under elevated pressure.
- Canvas chrome Clear only when dirty; placeholders rebound only after Clear/resize.
- Geom + Kitty stay deferred for the whole seam drag (publish on mouse-up).
- Memory-pressure notice only on level **transition**.

## Bench (release, host Urgent)

See `bench-after-scroll-fix.txt`. Gate: `scroll_1mib_urgent` p95 ≤ 25ms.
