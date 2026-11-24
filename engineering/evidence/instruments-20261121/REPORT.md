# Instruments + dogfood under Urgent pressure (2026-11-21)

## Host
- `kern.memorystatus_vm_pressure_level` = **2 (Urgent)** during all runs
- `hw.physicalcpu` / logical: see sysctl at capture time
- Tools: `xctrace` Time Profiler (20s), `/usr/bin/sample` (3s), PTY dogfood driver
- Computer Use MCP: **refused terminal apps** (Ghostty/posit-edit) — dogfood via PTY instead

## Compile warnings
Resolved: removed dead `highlight_spans` / `highlighted`; `cfg_attr(not(test), allow(dead_code))` on test-only public helpers (`set_confirmed`, `geom_path`). `cargo build -p ide` → **no unused warnings**.

## Bottleneck findings

| Signal | Result |
|---|---|
| Time Profiler / sample | **~99% `kevent` wait** on main thread — not a CPU spin |
| RSS (sample) | **~5.4 MiB** footprint / ~7.6 MiB peak on 1 MiB file |
| Scroll (bench, Urgent) | `scroll_1mib_urgent` ~1.5 ms/frame |
| Quit before fix | ~600–650 ms — `q` behind Downs **and** `stdout.flush()` deadlock vs unread PTY |
| Quit after fix | **1.3–3.8 ms** (PTY dogfood, Urgent); unit `quit_key_wins_over_*` PASS |

## Architecture (what is / isn’t threaded)
- **UI thread (sync):** input coalesce, layout, draw, Kitty escapes, quit
- **Worker thread:** Canvas device taps (`posit-canvas-device`)
- **Child process:** screenshot capture
- **Not async frame loop** — Tokio reserved for IO fan-out
- **GPU:** terminal-side after escapes; no app Metal path yet

## Code changes this pass
1. Warning cleanup
2. Quit hoisted above Down-arrow keys (view mode / Ctrl-Q)
3. `shutdown_fast()` kills Canvas capture with ≤50 ms reap (no save on plain quit)
4. Quit path: hoist `q`/`Ctrl-Q`, `shutdown_fast()` (≤50 ms capture kill), no `flush`, `libc::_exit(0)`
5. Scheduler drain/coalesce + ≤60 Hz present (prior turn)

## Canvas click / resize
- Unit: `phone_and_browser_dispatch_through_stubs`, `canvas_resize_is_deferred_*` — Kitty/geom deferred mid-drag
- Live Ghostty click dogfood blocked by Computer Use terminal restriction; prior live `terminal-phone tap` evidence remains valid

## Traces
- `/tmp/posit-traces/cpu.trace` (unsymbolicated release — sample confirms kevent idle)
- `/tmp/posit-traces/sample.txt` (copied here when present)
