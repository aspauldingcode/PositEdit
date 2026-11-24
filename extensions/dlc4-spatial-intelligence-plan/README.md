# DLC 4 — Spatial Intelligence & Interactive Development

**Architecture and one-click agent integration kit** for the existing `~/one-shot-terminal-ide` Rust + Bend 2 IDE. Additive only. This ZIP is a detailed engineering blueprint, contracts, executable validation, and guarded bootstrap; **not** a claim that the twelve complex features have already been implemented. Agent must implement and verify against the actual host repository.

## Non-negotiable
- Preserve existing niri-style horizontal scrollview, transition animations, splits, keyboard focus and original features.
- Zero-configuration sane defaults; no compulsory sidebars, setup wizard or independent infinite canvas.
- One host renderer, one layout authority, one command/event system, one project graph, one DAP, one graphics surface owner.
- Preserve DLC 1/2/3 including local Tab On control bottom-right.
- Lazy workloads, bounded memory, offline by default, graceful text fallback.
- Rust initially for terminal/render/process/native; Bend 2 for policy, pure transformations, formal models where toolchain actually supports them.
- No existing files overwritten by extraction or automatic installer.

## Start
1. Download this archive into `~/Downloads/universal-ide-spatial-intelligence-dlc4.zip`.
2. Give your agent `DLC4_ONE_SHOT_AGENT_PROMPT.md` (or paste the launcher prompt provided in chat).
3. The agent validates and extracts to a unique `.dlc-staging` directory, runs `python3 scripts/verify_package.py`, previews `python3 scripts/install_overlay.py --host ~/one-shot-terminal-ide`, then applies only after reviewing. The installer adds a planning directory and does not modify Cargo manifests or host code.
4. Agent implements, integrates, benchmarks and proves each feature against actual host code; a successful overlay install is NOT feature completion.

## Contents
- `architecture/`: system design, UX, feature-by-feature behavior, renderer, protocols, milestones, security/performance.
- `contracts/`: JSON schemas and API contract sketches.
- `engineering/`: machine-readable task DAG and acceptance matrix.
- `scripts/`: safe additive installer, package validator.
- `tests/`: contract and package unit tests.
- `DLC4_ONE_SHOT_AGENT_PROMPT.md`: complete autonomous implementation directive.
