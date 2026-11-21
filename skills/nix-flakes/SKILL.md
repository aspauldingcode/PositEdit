---
name: nix-flakes
description: Nix-first reproducible Mac and Linux tooling
---

# Nix-first reproducible Mac and Linux tooling

Use devShell and flake.lock with pinned resolved inputs, Cargo.lock, Nix buildRustPackage. Prefer existing Nix install, no privileged host installer. Test aarch64-darwin plus supported Linux. Report missing attr/platform availability rather than claiming builds.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
