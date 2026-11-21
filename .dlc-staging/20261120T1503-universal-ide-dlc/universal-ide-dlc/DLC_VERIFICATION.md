# Verification ledger (2026-11-20)

## Verified here

- Python standard library syntax import and module compile.
- 41 Python unit/integration tests: project recognition, mixed workspaces, Maven safe XML, static metadata and plans, explicit scope/trust rejection, terminal click mapping, Apple/Android/browser plan validations.
- **Actual native CMake configure → build of a C executable → run and assert process output** in an isolated temporary workspace on this Linux container.
- Supervised process output, exit code, workspace metadata and no shell string concatenation.
- Graph DAG/checker validation, script syntax compilation, zip integrity, SHA-256 manifests (when package built).

## Not verified here

- Rust Cargo crate compiles (Rust toolchain absent). Ratatui host wiring or mouse focus inside actual editor.
- Nix flake builds (Nix absent).
- Bend proof verdict (Bend absent). `.bend` text is a candidate additive model only.
- Gradle Tooling API bridge compile/sync; Maven effective POM; Bazel, Zig, SwiftPM authoritative builds.
- Xcode JSON native roundtrip, Swift helper compile, xcodebuild and Apple signing.
- GPU acceleration, terminal graphics embedding and iOS/Android simulator/device operations (host is not Apple Silicon macOS and toolchains absent).
- omarchy-apple-dev or xtool SDK provisioning and Apple SDK legal compliance in user's environment.
- Native 1:1 IDE behavior (requires differential fixtures and vendor toolchains).
- Exhaustive security/stability testing, real long-run interactive TUI smoke in user's terminal.

## Release policy

A complete DLC release MUST pass per-adapter native-model, build/run/debug/test and config-roundtrip checks on supported target hosts; prove core invariants; pass full regression suite; capture screenshots/video/latency for graphical runtimes; demonstrate no unauthorized device action; pin upstream versions and compare behavior with vendor IDEs. If any matrix cell is incomplete, mark it partial or unsupported rather than claiming the entire universal IDE is stable.

## How to repeat the checks

```
python3 -m compileall -q project_dlc
python3 -m unittest discover -s tests -v
python3 engineering/graph/validate.py
cargo test --manifest-path integration/rust/ide-dlc-ui/Cargo.toml  # requires Rust and network/cached crates
nix build .                                                   # requires Nix/network
bend DLC_PROOF.bend --verdict                                 # requires verified Bend 2
```

The shipped reference uses only Python 3.11+ standard library at runtime. GPU/SDK/IDE vendor tools are optional *external* processes.
