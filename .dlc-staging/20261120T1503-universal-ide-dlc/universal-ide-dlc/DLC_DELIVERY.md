# DLC delivery manifest

## Additive deliverables

- `project_dlc/`: executable standard-library Python control plane, passive model inspection, native action planning, safe IDE settings and CMake native preset writer, explicit runtime action plans, interactive curses toolbar.
- `dlc.py`: CLI entrypoint.
- `integration/rust/ide-dlc-ui/`: additive native Rust Ratatui/Crossterm clickable toolbar and JSON bridge; requires compilation in host.
- `integration/bridges/GradleModelBridge.java`: limited documented Tooling API starting point; not full IntelliJ/Android model.
- `scripts/install_overlay.py`: non-overwriting dry-run-first staging into a compatible Rust Cargo workspace; does not autonomously rewrite host editor implementation.
- `integration/contracts.json`: versioned target API contract including separate planned trust-gated native model/configuration/runtime methods.
- `DLC_ARCHITECTURE.md`: full new subsystem architecture.
- `DLC_PLATFORM_TARGETS.md`: host/target/SDK/ABI/device matrix.
- `DLC_COMPATIBILITY_MATRIX.md`: explicit current support status.
- `DLC_ONE_SHOT_AGENT_PROMPT.md`: autonomous agent instructions from initial overlay through native parity and platform acceptance.
- `engineering/skills/`: specialist instruction skill catalog.
- `engineering/graph/`: 44 tasks, dependency edges, DAG validation.
- `DLC_LAWS.bend`, `DLC_PROOF.bend`: additive proof candidates (NOT verified by Bend here).
- `REFERENCES.md`: vendor docs and upstream references.
- `flake.nix`: standalone Nix package definition (NOT built here).
- `tests/`: automated unit/integration/PTY/CMake cases.
- `verification_report.json`: actual tool and test checks from this build environment.

## Verified

41 Python tests pass; includes real CMake configure → executable build → run, CMake native UserPreset creation and configure, subprocess timeout handling, PTY TUI startup/refresh/exit, non-overwriting overlay staging, mixed project detection, safe command planning and explicit failure when targets/toolchains are unknown. Graph has no cycles. Bytecode compilation passes. Archive integrity and checksums are verified by creation script.

## Not completed

Full IntelliJ/Gradle/Maven/Xcode native one-to-one project model/editing; the host Rust UI actually wired; Apple Silicon Android GPU embedded graphics and iOS simulator/physical device runs; Linux Apple SDK and signing; cross-platform Rust/Nix/Bend builds; independent stability and hardware certification. These are not possible to assert or fully complete in this sandbox and remain explicit graph milestones. Any final vendor-IDE parity claim requires fixture-by-fixture real-tool/device evidence.

No original editor ZIP files are included in this DLC; it contains only new modules, docs and scripts.
