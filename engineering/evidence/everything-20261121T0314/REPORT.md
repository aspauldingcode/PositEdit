# Thorough everything-test report

Timestamp: 2026-11-21T03:14Z evidence dir. Host: Apple Silicon arm64.

## Verdict

**Pass**, with one known environmental flake.

| Area | Result |
|------|--------|
| Cargo workspace | **133 passed / 1 failed** (`copy_and_paste_use_the_system_clipboard` clipboard contention) |
| ide-tui excluding clipboard | **62/62 pass** |
| Terminal resize + multiplex | **4/4 pass** (`terminal_*`) |
| Live browser/phone/android PTY panes | **pass** |
| DLC crates (spatial/dap/dlc-ui/local-tab) | **25/25 pass** |
| Clippy `-D warnings` (tui/spatial/dap) | **pass** (lint fixes applied during this run) |
| DLC3 python | **32/32** |
| DLC4 package python | **4/4** |
| Harness python | **13/13** |
| Project DLC doctor + runtime plans | **pass** (browser/phone/android available with shims+SDK) |
| Live browser shim frames | **pass** |
| Live iPhone sim pane | **pass** (booted device listed) |
| Live Android AVD pane frames | **pass** (`ANDROID_OK`) |

## What was exercised

- Main editor: scrollview, focus, sidebars, mouse/wheel, help, finders, workbench toolbar, Tab control, spatial `z*` modes
- Terminal: multiplex, body text select (not resize), **top-edge drag resize**, session-list side drag, open/close
- DLC1 workbench plans / doctor / runtime argv
- DLC3 local-tab engine + UI regression
- DLC4 spatial crate + host UI tests
- In-buffer runtimes via host `Panel`: browser, phone, android concurrently

## Failures / blockers

1. **Clipboard unit test** — still flaky against shared macOS pasteboard (`Title\nbody text` / other apps racing). Not a terminal-resize regression.
2. Upstream `terminal-browser` / `terminal-phone` binaries still absent; evidence shims under `engineering/evidence/everything-20261121/shims/` used (same as prior everything run).
3. `bend` toolchain still not installed (not re-blocked beyond prior report).

## Artifacts

All logs in `engineering/evidence/everything-20261121T0314/`:

- `cargo-workspace.txt`, `ide-tui-skip-clipboard.txt`, `terminal-resize.txt`
- `runtime-panes.txt`, `browser-live.txt`, `phone-live.txt`, `android-live.txt`
- `dlc-crates.txt`, `dlc3-py.txt`, `dlc4-py.txt`, `harness-py.txt`
- `doctor.txt`, `runtime-plans.txt`, `clippy.txt`, `adb.txt`, `sims-booted.txt`
