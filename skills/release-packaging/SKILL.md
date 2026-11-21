---
name: release-packaging
description: Reproducible release and provenance
---

# Reproducible release and provenance

Prepare Nix core/full packages, aarch64-darwin tests, Linux crossplatform where feasible, artifact hashes, SBOM where installed, license/security, benchmark report, reproducible source tree and changelog. Do not publish externally.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
