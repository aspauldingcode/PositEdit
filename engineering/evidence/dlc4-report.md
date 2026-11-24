# DLC 4 Spatial Intelligence — final report

## Checkpoint and extraction

| Item | Value |
|------|-------|
| Host | `/Users/8amps/PositEdit` (symlink target of `~/one-shot-terminal-ide`) |
| HEAD | `f98bae45f044fc575dc51801f1befe3168133282` |
| `git stash create -u` | `e5c8af2a20f620a5ecbc77ad34ddbd2f034968fd` |
| ZIP SHA-256 | `d7cfdd30c73e180e2f5b588f43ed6bd690780911b939184b9ee675d599689a66` |
| Staging | `.dlc-staging/dlc4-20261121T020331Z/` |
| Extraction overwrites | **0** |
| Planning overlay | `extensions/dlc4-spatial-intelligence-plan/` (additive only) |

Rollback of the pre-DLC4 dirty tree object: `git stash apply e5c8af2a20f620a5ecbc77ad34ddbd2f034968fd` (only if requested; does not reset HEAD).

Package validation: `verify_package.py` passed; `unittest` 4/4 passed.

## Files added

- `crates/ide-spatial/` (F01–F12 services + differential test)
- `crates/ide-tui/src/graphics.rs` (host GraphicsArbiter)
- `bend/dlc4/policy.bend`, `bend/dlc4/README.md`
- `extensions/dlc4-spatial-intelligence-plan/` (installer planning bundle)
- `.dlc-staging/dlc4-20261121T020331Z/`
- `engineering/evidence/dlc4-*.txt` and this report

## Files deliberately modified

- `crates/ide-tui/Cargo.toml`, `crates/ide-tui/src/lib.rs` — spatial session, `z*` chords, overlay draw, relations, output ingest, minimap paint
- `crates/ide-syntax/src/{lib,highlight}.rs` — `NodeRange` / `node_at_byte`
- `crates/ide-dap/src/lib.rs` — `supports_read_memory`, `read_memory`, tests
- `Cargo.lock` — workspace resolution for `ide-spatial`

Untouched: `LAWS.bend`, `PROOF.bend`, `SPEC.bend`.

Pre-existing uncommitted DLC 3 / host work was preserved (not reset).

## Implementation status F01–F12

| ID | Feature | Status | Evidence / limitations |
|----|---------|--------|------------------------|
| F01 | Spatial overview | **integrated** | `zo` / lone `z`; Escape restores focus/view; unit + host UI tests |
| F02 | Micro-minimap | **integrated** | ASCII heatmap on wide focused tiles; hides when narrow; Braille path in crate |
| F03 | Inline probes | **library + DAP gate** | ProbeStore + `readMemory` refused unless advertised; not driven by a live adapter session in TUI yet |
| F04 | Structural edit | **integrated** | `zn` preview via Tree-sitter node; apply through `Document::apply`; moves stay ambiguous/refused |
| F05 | Execution timeline | **integrated** | `zt` recorded replay label; reverse debug only if live `stepBack` accepted (currently always RecordedReplay in UI) |
| F06 | Structured output | **integrated** | `show_output` ingests RawChunks; ANSI sanitize + workspace path gate |
| F07 | Ephemeral lenses | **integrated** | `zl` symbols lens; stack ≤3; syscall gated unavailable by default |
| F08 | Rich preview | **integrated** | `zp` + GraphicsArbiter; TERM=`dumb` → text backend; no Kitty/Sixel round-trip on this host |
| F09 | Causal navigation | **library** | Evidence-only edges; temporal claims refused; breadcrumb pop API |
| F10 | Workspace snapshots | **integrated** | Atomic write under `.posit/spatial/`; opt-out file; `s` in overview saves |
| F11 | Change impact | **integrated** | `zi` debounced ranker; never auto-runs builds/tests |
| F12 | Pane relationships | **integrated** | Workbench Browser/Phone/Android/Debug/Test attach relations; focus uses existing panes |

## Rust / Bend

- Rust owns all runtime paths.
- `bend/dlc4/policy.bend` prototypes exist; `bend` CLI **not installed** → switchover **blocked**.
- Differential fixtures: `crates/ide-spatial/tests/differential.rs` **passed**.

## Tests

| Suite | Result |
|-------|--------|
| Baseline `cargo test --workspace` | Pre-existing fail: `copy_and_paste_use_the_system_clipboard` |
| `ide-spatial` (16 + 1 differential) | **pass** |
| `ide-dap` (incl. readMemory) | **pass** |
| `ide-tui` spatial_* | **4/4 pass** |
| `ide-tui` excluding clipboard | **61 pass** |
| `clippy -p ide-spatial -p ide-dap -D warnings` | **pass** after ANSI loop fix |
| `nix flake check` | **failed** — Nix build missing `ide-local-tab-ui` in filtered source (pre-existing packaging gap for uncommitted DLC3 crate); marked **blocked**, not passed |
| Live tty UI | **blocked** — agent TERM=`dumb`, no interactive tty session |

## Graphics / debugger capabilities

- Probe env: `TERM=dumb`, no `TERM_PROGRAM`, no iTerm session → **text fallback**.
- Kitty/Sixel/iTerm2: **unverified** (no capable terminal in this run).
- DAP `readMemory` / `stepBack`: capability-gated; mock tests prove refusal without advertisement.
- Reverse debugging: **not claimed**.

## Performance (Apple Silicon arm64)

- Synthetic overview 100 panes × 1000 builds: **~11.0 ms** (`overview_1000_iters_ms=10.99`).
- Idle DLC4 services: activated only on `z*` / workbench / output paths (lazy).
- Linux/NixOS/Windows host runs: **blocked** on this Mac.

## Security notes exercised in tests

- Snapshot credential-shaped JSON rejected.
- Output ANSI stripped; `/etc/passwd`-style paths rejected outside workspace.
- Syscall lens unavailable without capability.
- Side-effecting watch expressions refused by ProbeStore heuristic.

## Remaining blockers

1. Live terminal graphics round-trip (need Kitty/WezTerm/iTerm2).
2. Live DAP adapter with `supportsReadMemory` / `supportsStepBack` for F03/F05 advanced labels.
3. `bend` toolchain for formal switchover.
4. Nix packaging must include `ide-local-tab-ui` (and now `ide-spatial`) in flake source.
5. Pre-existing clipboard unit test failure (environment clipboard contention).

## Goal check

One host IDE with additive DLC4 services wired into the existing niri-style scrollview, workbench, and bottom-right Tab control — not a standalone demo.
