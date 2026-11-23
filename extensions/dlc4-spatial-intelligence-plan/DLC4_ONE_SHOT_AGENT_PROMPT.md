# AUTONOMOUS ONE-SHOT AGENT: DLC4 SPATIAL INTELLIGENCE

You are the principal engineer integrating DLC4 into my **existing** `~/one-shot-terminal-ide/` Rust+Bend2 universal terminal IDE. Archive: `~/Downloads/universal-ide-spatial-intelligence-dlc4.zip`.

## Objective
Implement **all 12 workstreams** described in `architecture/FEATURES.md` as genuine integrated host features. Preserve existing niri-style scrolling window manager, split navigation and animations. Zero-configuration sane defaults. Do not replace current IDE, create another renderer, another window manager, or break DLC 1/2/3. Preserve DLC3 local Tab On control at bottom-right.

## Safe first steps
1. Inspect actual repo, Git status, uncommitted changes and previous DLCs. Capture baseline tests and create reversible checkpoint; do not commit unrelated work.
2. Validate ZIP integrity and path security, extract **only** into a unique `.dlc-staging/dlc4-<unique>` directory using exclusive creation. Never unzip over repo root; never use `unzip -o`, `cp -f`, `rsync --delete`, `git reset --hard` or `git clean`.
3. Read README, every `architecture/*.md`, `contracts/*`, `engineering/*`, scripts and tests. Inspect scripts before execution.
4. Run `python3 scripts/verify_package.py`, `python3 -m unittest discover -s tests -v`, then `python3 scripts/install_overlay.py --host ~/one-shot-terminal-ide` for dry run. The installer adds **only a non-overwriting planning bundle**. Apply only after inspecting the plan: `python3 scripts/install_overlay.py --host ~/one-shot-terminal-ide --apply`.
5. Do not mistake overlay installation for implementation. Build all functionality in existing host crates via minimal reversible patches and actual event/render wiring.

## Implementation sequence
M0: inventory existing host layout, renderer, input dispatcher, command registry, project graph, Tree-sitter, LSP, DAP, task runner, preview transport and DLC3 status control. Establish contracts, feature flags as capability detection (not customization), baselines and dependency graph.
M1: F01 spatial overview, F07 lenses, F10 snapshots, F12 pane relationships. Reuse host niri layout; restore focus and scroll on close.
M2: F02 micro-minimaps, F04 lossless AST structural edits, F11 change impact. Incremental and bounded.
M3: F03 inline DAP probes, F06 structured output, F09 evidence-based causal navigation. No invented variable values or causation.
M4: F08 rich preview via existing browser/phone graphics ownership, terminal protocol negotiation and fallback.
M5: F05 event timeline and recorded trace replay; only label true reverse debugging if compatible recorder genuinely supports it.
M6: refine accessibility, narrow terminal fallback, privacy, performance, Nix, docs, regression tests.

## Architecture constraints
- One host event loop, renderer, terminal writer, scrollview layout, animation authority and graphics surface manager.
- Reuse DLC project graph, debugger, browser/mobile runtimes, and local completion worker; never clone them.
- Services communicate via revisioned, cancellable, bounded, versioned contracts. No background process per keystroke.
- Lazy activation of heavy features; no mandatory setup, plugin management, or customization panels.
- Rust owns native services initially; Bend2 candidates for pure graph/policy logic, migrated via differential tests and performance gates. Preserve existing LAWS.bend and PROOF.bend; never fabricate formal proof.
- No silent destructive changes, unsafe device access, arbitrary process reads or untrusted command execution.

## Verification
Run existing Cargo fmt/check/test/clippy, Nix flake check where available, Bend validation using real installed toolchain, DLC4 package tests and real host interaction tests. Verify original scroll animations, pane focus, all DLC 1-3 behaviors and Tab On bottom-right. Test terminal graphics fallback and resource cleanup, Unicode AST edits, stale DAP data, trace fidelity, log injection, snapshot recovery. Benchmark input p95, idle CPU/memory, frame latency on Apple Silicon and available Linux host. Record exact commands and results. A test blocked by missing hardware must be marked blocked, not passed.

## Autonomy
Work through task DAG, repair failures with bounded retries, checkpoint progress, and continue independent tasks when one blocks. Do not claim a feature complete unless actual host code is integrated, test evidence exists and UX has been inspected. Do not promise work outside active agent execution. Respect approvals for destructive/security-sensitive actions.

## Final report
List files added/modified, staging directory, checkpoint, extraction overwrites (must be 0), baseline vs post-integration tests, each F01-F12 implementation/evidence/limitations, DLC1-3 regression, Bend migration status, CPU/memory/latency, security tests, blocked features and exact rollback steps. **The goal is one working IDE, not a standalone demo or an unpacked ZIP. Start now.**
