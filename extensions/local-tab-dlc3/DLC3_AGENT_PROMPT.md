# ONE-SHOT AUTONOMOUS AGENT PROMPT — DLC 3 LOCAL TAB INTELLIGENCE

You are the authorized lead integration engineer for my existing Rust + Bend 2 universal terminal IDE. Integrate the **newly downloaded DLC 3: Local Tab Intelligence** safely into the existing project. This is an additive feature, **not** a new IDE or a rewrite.

**Existing IDE:** `~/one-shot-terminal-ide/`

**DLC 3 ZIP (expected):** `~/Downloads/universal-ide-local-tab-dlc3.zip`

**Previous DLC:** Project/runtime workbench may already be installed. Discover it; preserve it. Never reinstall, overwrite, or duplicate it blindly.

## A. Mission / outcome

Deliver a built-in **local-only, code-only, on-device predictive Tab completion system** with a clickable **bottom-right status-bar control, enabled by default**. It should feel as fast, contextually relevant and unobtrusive as Cursor Tab/Copilot, but parity is an **evaluation target**, not a claim. Implement actual host wiring, model-backed predictions where authorized, robust no-model fallback, ghost text, Tab acceptance, Esc dismissal, user settings, tests, and real Apple Silicon benchmarks.

This DLC includes a working Python reference worker and additive Rust UI crate, **not model weights** and **not completed host wiring**. Never mark these unimplemented requirements complete just because the archive installs.

## B. Absolute preservation rules

1. Do NOT extract directly over the IDE root. Do NOT run `unzip -o` into the repository.
2. Inspect the current repository and Git status. Preserve all existing user modifications, `LAWS.bend`, `PROOF.bend`, `AGENTS.md`, configuration, build/run controls, existing DLC integrations and working Rust code.
3. Create a recoverable checkpoint (prefer an isolated Git worktree; also back up any files you intentionally plan to modify, with checksums). No destructive clean/reset, no forced git checkout, no recursive copying that overwrites existing files.
4. Validate the ZIP with a safe extractor: reject absolute paths, `..`, symlinks, hardlinks/special files, duplicate normalized paths, casefold collisions, path escape, oversized decompressed entries, and nested symlink parents. Use exclusive creation for extracted files.
5. Extract under a brand-new `~/one-shot-terminal-ide/.dlc-staging/local-tab-<unique-id>/`, not over live project files.
6. Read `README.md`, `DLC3_ARCHITECTURE.md`, `DLC3_MIGRATION.md`, `DLC3_VERIFICATION.md`, `model/MODEL_SELECTION.md`, the installer source, Rust crate, and all tests. Treat documentation as claims to verify against code.
7. From the extracted `universal-ide-local-tab-dlc3/` directory run the non-mutating preview:
   `python3 scripts/install_overlay.py --host ~/one-shot-terminal-ide`
8. Inspect ALL destinations and the host's workspace layout. Apply ONLY if no collisions:
   `python3 scripts/install_overlay.py --host ~/one-shot-terminal-ide --apply`
9. If any collision occurs, stop that installer operation and reconcile with targeted, reversible patches in an isolated worktree. Never bypass its refusal or overwrite existing files.
10. Installer only copies new files. It intentionally does **not** edit host Cargo.toml or wire the editor. You must integrate in code via reviewed patches and tests.

## C. Technical goals

### Built-in bottom-right Tab control

- The control is **ON by default** on first run, with persistence when the user toggles it.
- Exactly on the far right of the IDE's bottom status bar, not over editor text. Use `ide-local-tab-ui` Rust component and existing ratatui renderer.
- Display accurate states: `Tab ON ✓` when model ready; `Tab ON · Basic` for lexical fallback; `Tab ON · Setup` when model unavailable; `Tab Loading`; `Tab OFF` when disabled.
- Left-click toggles; Ctrl-Alt-T toggles; Shift-click or suitable key opens settings.
- Tab accepts a visible, validated suggestion via the existing atomic revision-checked edit transaction. If no suggestion is shown, Tab performs its normal indentation/focus action. Esc dismisses ghost text. Disabling cancels requests and immediately hides ghost text.
- Do not use global shortcut handlers that conflict with existing IDE commands or terminal input. Document and resolve collisions.

### Coding-only local prediction

- Default mode must NEVER call cloud inference or upload source. No remote model URLs, telemetry, implicit credential access or arbitrary tool execution.
- Use a code-specialized **base FIM model**, not a general-purpose chat model. Benchmark Qwen2.5-Coder 0.5B and 1.5B compatible GGUF Q4 models and choose based on real latency/quality/memory results. Check model license, hashes, availability and FIM compatibility.
- Runtime: `llama.cpp` with Metal on Apple Silicon by default, CPU fallback and compatible Linux backend. Compare MLX only if useful; keep backend pluggable.
- Do not download model weights, install system packages, contact external providers, or launch a privileged device process without appropriate authorization. If model not present, enable the fast lexical fallback and show honest status. Never equate fallback with neural completion.
- Model worker communicates through supervised, bounded local IPC or a loopback-only local endpoint; isolate model process and explicitly disable proxies. Verify OS-level network policies for strict privacy. Model acquisition is separate from inference.
- **Never block the main TUI event loop** on model startup, FIM requests, file search, symbol retrieval, or worker errors.

### Completion quality and speed

- Prioritize LSP/snippet/identifier completions immediately; use the model when likely helpful.
- Use bounded FIM prefix/suffix and relevant project graph, imported definitions, LSP types, Tree-sitter structure, recent edits and nearby tests. Exclude ignored, credential, and outside-workspace files.
- Debounce, cancel stale requests, cache prompt prefixes, bound context, throttle under resource pressure, and unload model after idle when appropriate.
- Always verify document ID, revision, cursor/selection position and text anchors before presenting or accepting suggestions. Never apply an edit merely because a model returned text.
- Support Python, Rust, C/C++, Zig, Nix, Java, Kotlin, Swift, JavaScript, TypeScript, and unknown language fallback. Provide language-matrix evidence instead of claiming uniform quality.
- Add next-edit prediction only after ordinary FIM completion is stable and measured. Do not run shell commands/agents as part of automatic Tab behavior.
- Performance **targets** (not promises): keyboard-to-render p95 <=16ms, useful model suggestion p50 <=250ms and p95 <=500ms on M1/16GB benchmark target, no extra RAM when optional worker unloaded, bounded output tokens; report real numbers and exceptions.

## D. Rust + Bend 2 progressive migration

- **Rust** owns input/rendering, ghost text, editor transaction commits, async process supervision, filesystem boundaries and native Metal runtime integration initially.
- **Bend 2** is a candidate for pure context ranking, routing, policy, acceptance state and graph rules. Preserve `LAWS.bend` and `PROOF.bend` in the original IDE unchanged unless a carefully reviewed additive patch is needed.
- Read DLC's `DLC3_LAWS.bend` and `DLC3_PROOF.bend`: they are **requirements-only placeholders**, not compilable proofs. Convert to real Bend syntax only after verifying installed toolchain; do not claim verified proofs from comments.
- Define versioned language-neutral completion contracts; construct Rust/Bend differential fixtures; migrate components one at a time only after functional, security and performance parity; preserve Rust backend/rollback.
- No premature pure Bend rewrite of the GPU backend or editor core.

## E. Graph engineering and execution

Use the existing project's persistent engineering graph; import DLC3 nodes and relationships without duplicating the authoritative graph. Each task must track requirements, dependencies, ownership, artifacts, verification and blockers.

Autonomous loop: discover repository and environment → safe staging and installer preview → additive install → integrate TUI toolbar/worker → implement local model interface → add acceptance transactions and settings → unit tests → actual TUI tests → run on host hardware → benchmark latency/energy/memory → repair failures with bounded retries → record evidence → release check.

Parallelize only independent tasks through isolated worktrees; merge via one integration agent. Every completed node needs real source/test/measurement evidence. Stop for missing authorization rather than bypassing security.

## F. Tests and acceptance gates

Run DLC tests from extracted root:

```
PYTHONPATH=. python3 -m unittest discover -s tests -v
python3 -m compileall -q local_tab scripts
```

Then host checks (where available):

```
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
nix flake check
```

Test in the **actual IDE**, not just in the standalone worker:

1. Existing editor and DLC project buttons still work.
2. Bottom-right Tab toggle renders and defaults ON.
3. Left click / shortcut toggles; settings open; state persists.
4. Model missing → Basic or Setup status, no crash, local lexical completion functions.
5. Model installed → genuine code-only FIM suggestions run on-device with Metal verified where supported.
6. Ghost text does not alter buffer until Tab; Tab applies one atomic edit; Esc dismisses.
7. Stale and cancelled suggestions are never accepted, even under rapid edits and worker crashes.
8. No outbound completion traffic. Check both client and model process; document sandbox mechanism.
9. All target languages have at least smoke fixtures; record quality gaps.
10. Test low-memory behavior while browser, mobile emulator or LSP processes run.
11. Collect p50/p95 latency, RSS, memory pressure, GPU utilization, idle CPU, and battery/energy evidence on real Apple Silicon.
12. Track measured quality vs comparable Cursor Tab/Copilot tasks only if benchmark data are actually obtainable. Never claim quality parity from mockups or one example.
13. Current Rust/Bend contracts and laws are not weakened; run actual proofs where supported.

## G. Final report

Provide: exact archive and extracted path; Git checkpoint/worktree; files added; intentional files modified; **0 extraction overwrites**; any conflicts; commands and results; Rust tests; DLC tests; UI test evidence; model name/license/hash/quantization; accelerator; strictly local/privacy verification; language matrix; model p50/p95 and memory; Bend model/proof status; known limitations; next tasks; reproducible start/rollback instructions. Separate verified, implemented-but-unverified and not-implemented features.

**Start immediately by inspecting the existing IDE and the ZIP; do not ask for routine confirmations. Continue through eligible local work, but do not bypass permissions or falsely claim that a model or GPU benchmark ran. The objective is a genuinely integrated, slim, on-device, coding-only Tab engine, not a separate app that merely runs alongside the IDE.**
