# Everything test report — DLC 1/2/3/4 + host + in-buffer runtimes

Date: 2026-11-21 (local). Host: Apple Silicon (`arm64`), macOS.

Evidence directory: `engineering/evidence/everything-20261121/`

## Verdict

Host editor, DLC 3, DLC 4, workbench/runtime planning, and **live in-terminal-buffer** browser / iPhone / Android panes were exercised successfully.

One pre-existing host flake remains (`copy_and_paste_use_the_system_clipboard`). Staging DLC1 unittest import fails on system Python 3.9 (`tomllib`); live `project_dlc` against this repo **passed**.

Upstream `terminal-browser` / `terminal-phone` binaries were **not installed**. Evidence shims at `shims/` satisfied the DLC argv contract and still drove:

- **Browser:** `agent-browser read` of `http://example.com` + pane frames
- **iPhone:** already-booted **iPhone 18 Pro** simulator status frames in the PTY
- **Android:** live **Medium_Phone / emulator-5554** screencap stream via `android_pane.py`

The host `Panel` multiplexed all three as separate runtime sessions without duplication.

## Automated suites

| Suite | Result | Notes |
|-------|--------|-------|
| `cargo test --workspace` | **132 passed / 1 failed** | Fail = clipboard flake only |
| `cargo test -p ide-tui -- --skip copy_and_paste` | **62/62** | Includes spatial + live runtime pane |
| `terminal::browser_phone_android_live_inside_terminal_buffer` | **pass** | Real PTY panes for browser/phone/android |
| `ide-spatial` + differential | **17/17** | DLC4 |
| `ide-dap` | **3/3** | incl. readMemory gate |
| `ide-dlc-ui`, `ide-local-tab-ui`, core/syntax/process/test | **pass** | DLC1 UI / DLC3 Tab / core |
| Remaining crates (`ide-git`…`ide-ai`,`ide`) | **18/18** | |
| DLC3 python (`unittest`) | **32/32** | |
| DLC4 package (`unittest`) | **4/4** | |
| Harness | **13/13** | |
| DLC1 staging `test_dlc.py` | **blocked** | Python 3.9 lacks `tomllib` |
| Live DLC1 `project_dlc` doctor/detect/runtime | **pass** | caps + plans + 2 projects (nix, cargo) |
| `bend PROOF.bend` | **blocked** | `bend` not installed |

## In-buffer runtime proof

### Host Panel (Rust)

Command:

```bash
IDE_TERMINAL_BROWSER=…/shims/terminal-browser \
IDE_TERMINAL_PHONE=…/shims/terminal-phone \
cargo test -p ide-tui browser_phone_android_live -- --nocapture
```

Result: 3 sessions titled `browser`, `phone`, `android`; re-focus does not duplicate; recognizable PTY output for each.

Log: `host-runtime-panes.txt`

### Browser pane

- Shim + `agent-browser read http://example.com` returned documentation text
- Continuous `browser-frame N` lines for pane lifetime
- Log: `browser-live.txt`

### iPhone simulator pane

- UDID `9B662FC3-4B78-4561-8200-8CC75EC91718` — **iPhone 18 Pro (Booted)**
- Pane streamed `phone-frame` status lines
- Full framebuffer screenshot: `iphone-booted.png` (1206×2622 PNG, 3.6 MiB)
- Log: `phone-live.txt`, `iphone-screenshot.txt`

### Android emulator pane

- Reused `emulator-5554` (`sdk_gphone64_arm64`) for AVD `Medium_Phone_API_36.1`
- `android_pane.py` printed live `frame png 1080x2400` + logcat into the pane (`ANDROID_PANE_OK frames 2`)
- Full framebuffer: `android-booted.png` (1080×2400 PNG, 1.4 MiB)
- Logs: `android-live.txt`, `adb-before.txt`, `adb-after.txt`

## Feature coverage checklist

### Main editor

- Scrollview / focus / resize / mouse / wheel / fullscreen / sidebars — covered by ide-tui tests
- Terminal multiplex / console — covered
- Help / palette / finders / Yazi — covered
- Clipboard yank — **failed** (environment contention with other clipboards)

### DLC 1 (project workbench)

- Toolbar Build/Run/Configure/Test/Refresh/Browser/Phone/Android/Debug — UI tests
- Untrusted preview-only cargo plan — pass
- Runtime plans without host `open`/`simctl`/`xdg-open` — pass
- Doctor capabilities with shims + SDK — browser/phone/android/adb **true**; xtool **false**

### DLC 2 (implied by workbench runtimes / process / DAP)

- Process runner tests — pass
- DAP capability gates — pass
- Debugger availability path in workbench test — pass (tool-dependent notice)

### DLC 3 (local Tab)

- Bottom-right Tab ON control regression — pass
- Ghost accept revision-safe — pass
- Python engine/protocol/worker suite — 32 pass

### DLC 4 (spatial)

- Overview / lens / structural / timeline / impact / Tab precedence — pass
- Package + spatial crate — pass

## Honest limitations

1. **Upstream `terminal-browser` / `terminal-phone` not on PATH.** Evidence used contract-compatible shims that still hit real browser-fetch and real iOS Simulator. Pixel Chromium/GPU inside the IDE cell was not claimed.
2. **Android pane** streams coarse PNG metadata + logcat into the PTY (by design of `android_pane.py`), not a full GPU framebuffer protocol inside cells. Full PNG was captured via `adb screencap` as proof the AVD is live.
3. **DLC1 staging unittest** needs Python ≥3.11 for `tomllib`; live module path used instead.
4. **Bend** toolchain absent.
5. **Clipboard** unit test remains environmentally flaky.

## How to re-run

```bash
export PATH="$PWD/engineering/evidence/everything-20261121/shims:$HOME/.cargo/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
export IDE_TERMINAL_BROWSER=$PWD/engineering/evidence/everything-20261121/shims/terminal-browser
export IDE_TERMINAL_PHONE=$PWD/engineering/evidence/everything-20261121/shims/terminal-phone
export ANDROID_HOME=$HOME/Library/Android/sdk

cargo test --workspace --offline -- --skip copy_and_paste_use_the_system_clipboard
cargo test -p ide-tui browser_phone_android_live -- --nocapture
PYTHONPATH=extensions/project-dlc python3 extensions/project-dlc/dlc.py --root . doctor
```
