# Scheduling / threading / async (2026-11-21)

## Verdict

**Do not make the editor frame loop `async`.** It is one UI thread by design.
Quit-during-slow-scroll was not missing Tokio — it was **presenting once per
queued scroll event**, so `q` sat behind the storm.

## Model

| Work | Where | Mechanism today |
|---|---|---|
| Input apply, layout, ratatui draw, Kitty escapes | **UI thread (sync)** | `run()` + `scheduler` |
| Canvas screenshots | **Child process** | `canvas_capture.py` |
| Phone/browser taps | **Worker thread** | `posit-canvas-device` mpsc |
| PTY read | **Per-session** | terminal maintain |
| Grep / MCP fan-out | **Async IO (future/ok)** | Tokio already in workspace |
| True GPU zero-copy | **Terminal** | Kitty/Ghostty after `t=s` |

## Root cause (quit stuck)

Old loop:

```text
present → wait 1 event → handle → present → wait 1 event → …
```

A trackpad flood of 40 scroll events meant 40 presents before `q` ran.

## Fix

New loop:

```text
wait → drain ALL pending events → coalesce
  → apply keys first (quit wins)
  → tick / workers poll
  → present at most once per ~16ms frame clock
  → check quit
```

Module: `crates/ide-tui/src/scheduler.rs` (`coalesce`, `FrameClock`, `WorkClass`).

## Async vs sync

- **Sync UI thread**: required for crossterm/ratatui tty ownership.
- **Threads/processes**: already used for capture + device actions.
- **Tokio**: use for multi-file IO / MCP — never to “await” a frame.
- **GPU**: still terminal-side; UI only emits escapes (prefer shm path later).

## Tests

- `scheduler::tests::*` — scroll storm keeps `q`; drag coalesce; work classes.
- Full `ide-tui` lib: 81 passed.
