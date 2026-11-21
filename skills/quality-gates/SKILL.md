---
name: quality-gates
description: Formal+empirical correctness and performance
---

# Formal+empirical correctness and performance

Execute cargo fmt/clippy/tests, Bend verdict, mock LSP/DAP fixtures, PTY UI, language matrix, security checks and Nix flake builds. Preserve exit codes, logs, metadata and coverage. Never mark missing tooling green.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
