# Agent + language matrix — 2026-11-22

## Built this pass

| Area | Change | Tests |
|------|--------|-------|
| Agent router | OpenCode-style priority: `IDE_AGENT` → agent → codex → claude → opencode → FreeLocal | `ide-ai` 6/6 |
| FreeLocal tools | `list` / `read` / `grep` / `format`; path sandbox; redaction; timed | free_local_* |
| LSP | completion, foldingRange, definition requests + capability parse | `ide-lsp` 2/2 |
| DAP | setBreakpoints, stackTrace, continue | `ide-dap` 4/4 |
| Fold | brace + indent fold ranges; `za` chord reports counts | fold::* |
| Snippets | per-language triggers; Tab ghost expansion | snippets::* + TUI |
| Projects | Makefile/make, xcworkspace, nix shells, Bazel markers; `ide-packages::project_suggestions` for Cargo/Nix/Gradle/Maven/CMake/Make/Xcode/IntelliJ | packages + TUI |

## Efficiency (unit, M1 host)

- Router probe (mocked which): &lt; 50 ms budget; observed microseconds in test assert
- FreeLocal read+redact: &lt; 20 ms budget; observed microseconds
- Explicit tool prompts skip cloud CLI round-trip

## Still not “every DLC feature flawless”

Honest gaps (do not claim done):

1. **Live LSP/DAP subprocesses** — session builders exist; no pooled rust-analyzer/jdtls/debugpy wire-up in the TUI event loop yet.
2. **DLC3 on-device FIM** — still lexical/`Basic`; no GGUF/Metal path.
3. **DLC4 live probes** — readMemory gated; no live adapter session.
4. **Code folding UI** — ranges computed; no persistent hide/show gutter chrome yet (`za` reports only).
5. **IntelliJ `.iml` / runConfigurations** — `.idea` detected and misc/modules suggested; full iml parse not implemented.
6. **Xcode schemes** — `.xcodeproj` / `project.pbxproj` suggested; xcodebuild scheme picker not in TUI.
7. **Make language id** — task runner accepts `make`; buffer language usually `plain`/`makefile` via extra path TBD.
8. **clippy -D warnings on ide-tui** — pre-existing failures remain.

## Commands

```
cargo test -p ide-ai -p ide-lsp -p ide-dap -p ide-syntax -p ide-packages --lib
cargo test -p ide-tui --lib detect_make_xcode_and_project_suggestions
cargo test -p ide-tui --lib free_agent_and_snippet_ghost_work
```
