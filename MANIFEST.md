# Content manifest and production readiness

- `ONE_SHOT_PROMPT.md`: complete one-shot master directive.
- `AGENTS.md`, `CLAUDE.md`, `.cursor/rules/`: persistent agent rules.
- `skills/*/SKILL.md`: 11 specialist playbooks.
- `engineering/`: task/evidence graph seed, laws, verification, security and policy.
- `scripts/`: preflight, project MCP generation, SQLite orchestrator, verification, TUI smoke, governance check.
- `LAWS.bend`, `PROOF.bend`, `SPEC.bend`, `bend/model.bend`: real Bend 2 formal model and starter proofs.
- `crates/*`: 16 Rust crates; **only `ide-core`, `ide-tui`, `ide-protocol` contain substantive starter code**; other crates explicitly remain TODO.
- `languages/`: 10 language adapter configurations (suggested tools; runtime availability unknown).
- `flake.nix`, `nix/`: initial Nix development shell with pinned Bend 2 v2.0.40 source input, packages, Home Manager example.
- `.github/`: initial CI and owner-approval template.
- `REFERENCES.md`: official upstream docs and source links.

## Important generated omissions

This ZIP does not embed `Cargo.lock` or `flake.lock`: no Rust/Nix toolchain is available in the build container to generate and validate those locks. `./bootstrap.sh --setup` generates Cargo.lock when Cargo/Nix is available, and `nix flake lock` generates flake.lock when Nix is available. Release readiness requires checking them in. The bundle itself is a working harness with an incomplete IDE starter, not a prevalidated finished product.

- `engineering/contracts/`: 14 component interface and negative-test contracts.
- `engineering/agents/`: 9 agent-role mandates.
