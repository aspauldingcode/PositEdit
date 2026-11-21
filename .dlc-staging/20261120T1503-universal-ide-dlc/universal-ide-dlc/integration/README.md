# Additive integration contract (no original source included)

This DLC is a standalone optional service. Do not copy or replace existing editor files. Use the host's existing workspace/process/event APIs to register it.

1. Copy `integration/rust/ide-dlc-ui` into the host workspace as a new crate. The host's Cargo workspace glob should discover it. Add `ide-dlc-ui = { path = "../ide-dlc-ui" }` to the host TUI crate's dependencies.
2. In the host renderer, place `WorkbenchToolbar::render(frame,rect)` at the top of a Project Workbench dock/panel. Route `KeyCode` and `MouseEvent` through `key_action` and `mouse_action`.
3. Wire Build/Run/Test to native action planning (`DlcBridge::request("plan", &[action,"--system",system])`) *off the render thread*, then to the host's **existing supervised process runner** after explicit workspace trust. The bridge deliberately does not execute commands itself.
4. Configure opens the native project/settings inspector plus IDE-owned run configuration editor. Refuse unsupported native edits. The standalone Python curses TUI implements configuration editing and a separate executable demonstration.
5. Browser/Phone/Android buttons should call runtime registry, use built-in graphics compositor only after an actual embed transport is implemented. Until then, launch upstream tools as managed terminal panes (not raw concurrent writes into Ratatui stdout).
6. Register Rust events `ProjectDiscovered`, `BuildPlanReady`, `BuildStarted`, `BuildFinished`, `RuntimeStarted`, `RuntimeFailed`. Use revision identifiers; don't allow stale projects to mutate state.
7. Keep `project_dlc/` outside the Rust executable as an optional, lazily-launched Python service; eventually port the native adapters to Rust without changing the IPC JSON contract.
8. `dlc.py --root PATH scan/inspect/plan ...` emits one JSON object per invocation, without network access and without executing project code. Never parse stdout as a command to run without validation.

## UI keyboard and mouse

Toolbar order Build, Run, Configure, Test, Refresh, Browser, Phone, Android. Shortcut keys 1–8. The independent curses workbench supports both keyboard and mouse-click selection and can execute confirmed native build commands.

## Integration tests required in host

- Host TUI maintains key latency when the process runner is occupied.
- Clicking controls dispatches exactly one action, with no overlap during resize.
- Build/test operations are cancelled cleanly on workspace closure.
- Untrusted project read-only scan must not execute project scripts.
- Runtime graphics must own a distinct pane; no concurrent uncoordinated TUI escape sequences.
- Windows, macOS, Linux path/ABI quirks validated before release claims.

## Optional overlay installer

From DLC directory, `python3 scripts/install_overlay.py --host /path/to/host` previews only. `--apply` copies the new Rust crate and Python service into the host plus adds only the new crate dependency to `ide-tui/Cargo.toml` after backing it up. It does **not** alter existing Rust TUI implementation files, since wiring is host-version-specific and must be tested by the coding agent. The operation deliberately refuses to overwrite existing DLC installation.
