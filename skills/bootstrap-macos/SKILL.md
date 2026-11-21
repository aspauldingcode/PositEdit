---
name: bootstrap-macos
description: Local macOS preflight and sandbox setup
---

# Local macOS preflight and sandbox setup

Detect uname/arch, xcode-select -p, xcrun --find clang, git, nix, python3, cargo, agent/codex/claude, bend, tmux, npm, uvx. Use no sudo or nonaudited curl-pipe-shell. Report missing Apple CLI tools, Nix installer, login, Accessibility or Screen Recording as explicit external blockers. Keep setup repo-local. Use Xcode CLT docs and bootstrapping scripts.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
