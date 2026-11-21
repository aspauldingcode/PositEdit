---
name: rust-tui
description: Ropey + Ratatui + Crossterm implementation
---

# Ropey + Ratatui + Crossterm implementation

Author editor state/transactions first, Unicode conversion tests, then renderer/input event loops, terminal cleanup, keyboard-only navigation, split views and PTY integration. Avoid blocking terminal redraws on synchronous network or tool subprocess. Benchmark on actual hardware.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
