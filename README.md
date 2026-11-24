# Posit Edit

<p align="center">
  <img src="assets/icons/posit-edit.png" alt="Posit Edit" width="160" />
</p>

Posit Edit is a terminal IDE, sometimes called the Posit IDE. The name comes from the posit number format, which is more accurate and more efficient than floating point.

Run it with the **CLI** (`posit-edit` / `pst`), or on macOS via the **wrapper app** (neovim-style) that opens your terminal and launches the CLI. `pos` is intentionally unused.

```sh
posit-edit [file]
pst [file]
```

**Icons:** macOS ships a Liquid Glass Icon Composer package (`assets/icons/PositEdit.icon`) built from the PE silhouette; Linux uses the brushed-metal mark (`assets/icons/posit-edit-metal-linux.png` / FreeDesktop hicolor). Build the wrapper with `sh ./scripts/build-macos-wrapper.sh`.

**Status: executable starter harness and IDE scaffold, NOT an already-complete IDE.**

This archive contains a one-command bootstrap, a provider-independent persistent task-graph orchestrator, a complete architecture plan for 14 IDE subsystems, reusable AI skills, Bend 2 formal law/proof model, verification gates, macOS/Nix packaging, MCP configuration generator, and a working *starter* Rust terminal editor. Agents must implement and verify the remaining features; stubs must never be called complete.

## First run (Apple Silicon Mac or Intel Mac)

```sh
git clone https://github.com/aspauldingcode/PositEdit
cd PositEdit
sh ./bootstrap.sh --run
```

`--run` starts the available, previously authenticated headless coding agent (Cursor CLI `agent`, Codex `codex`, or Claude Code `claude`) after preflight. The preferred provider is Cursor because the intended development environment uses Cursor; set `IDE_AGENT=codex` or `IDE_AGENT=claude` to override. To examine without starting an agent, run `sh ./bootstrap.sh --check`.

The bootstrap never silently installs Nix, Xcode, a CLI agent, subscriptions, or macOS permissions. Existing installed tooling is detected, and `nix develop` supplies toolchains **including a pinned Bend 2 v2.0.40 flake input** when available. Missing privileged prerequisites are recorded as **BLOCKED** with actionable instructions. Initial login and OS permission grants cannot be responsibly bypassed.

For direct one-shot use *inside Cursor*, copy `ONE_SHOT_PROMPT.md` into the agent conversation. The prompt instructs the agent to run `sh ./bootstrap.sh --check`, repair permitted setup issues, and then run the harness. Do not launch the harness recursively from within an agent task (the harness has a recursion guard).

## Commands

```sh
sh ./bootstrap.sh --check                # inventory tools, no mutation beyond report
sh ./bootstrap.sh --setup                # graph, project MCP, cargo locks if available
sh ./bootstrap.sh --run                  # setup and agent loop
cargo build -p ide                       # CLI: target/debug/posit-edit (+ pst via nix/flake)
sh ./scripts/build-macos-wrapper.sh      # macOS neovim-style PositEdit.app (Liquid Glass icon)
python3 scripts/harness.py status     # graph status
python3 scripts/harness.py next       # next unblocked task
python3 scripts/harness.py run --max-tasks 6
python3 scripts/harness.py report     # evidence and blockers
python3 scripts/verify.py --level scaffold
python3 -m unittest discover -s harness/tests -v
```

By default the harness attempts up to 60 bounded task invocations or 8 hours, whichever comes first, and has a *per-invocation task limit* and *per-task time limit*, with durable SQLite state and a `run --max-tasks ...` option for resuming. You can set `max_tasks_per_run` in `engineering/policy.json` within safe local resource limits. `--run` begins meaningful work immediately but it does not claim a permanently running daemon; unattended progression is bounded and resumable.

## Trust boundary

- Code edits and tests are workspace-scoped. Never exfiltrate credentials or run arbitrary remote install scripts without reviewing source.
- MCP filesystem access is restricted to this repository, and MCP is optional when native CLI tools already provide the capability.
- `LAWS.bend` and `SPEC.bend` are requirement-owner controlled: the agent can **propose**, but not silently change, accepted laws. The bundled initial laws are preaccepted as part of this request.
- A proof of a Bend model does not prove an arbitrary Rust program; conformance tests, model-vs-Rust differential tests, and TUI tests provide the connection.
- Failing or missing tools do not become success. Proof-gated milestones remain blocked until the actual Bend proof verdict is obtained.
- No remote push, publication, paid resource provisioning, entitlement manipulation, password prompts, security bypass, or macOS privacy grants without separately existing authorization.

Read `ONE_SHOT_PROMPT.md`, `ARCHITECTURE.md`, `engineering/GRAPH_ENGINEERING.md`, `engineering/VERIFICATION.md`, and `REFERENCES.md`.

**Release verification:** `python3 scripts/verify.py --level full` requires actual Rust, Bend, tmux, Nix and observed language-matrix evidence; `--level release` also checks all 14 component implementation declarations, lockfiles and performance evidence. Missing evidence always fails.

Graph mutations: `python3 scripts/graphctl.py --help` (add tasks/defects/risks/edges, reopen downstream after changed contracts). A node-specific result is always checked by the outer harness; manually editing graph state to mark a feature complete is prohibited.
