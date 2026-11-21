# Verification, test matrix, release gate and audit policy

## Required layers

- L0: Static instructions/graph/schema validation with Python standard-library tests.
- L1: cargo fmt, clippy and cargo test for all workspace crates.
- L2: actual Bend 2 `bend PROOF.bend --verdict` on immutable accepted requirements. Missing Bend = BLOCKED, not PASS.
- L3: Unicode/text property tests, revision-stale handling, failure injection.
- L4: protocol fixtures and mock servers for LSP JSON-RPC and DAP framing. Validate malformed/multiple/partial frames and cancellation.
- L5: real language toolchains per platform, with matrix of supported versions and capabilities.
- L6: tmux/PTY interaction scripts, terminal state recovery, screenshots/text captures, resize and color modes.
- L7: cross-platform reproducible Nix build/check and build-from-clean checkout.
- L8: security tests around permissions, adversarial repo contents, escaping root, MCP credentials, remote agent and subprocess timeout.
- L9: benchmarks with actual baseline and hardware metadata. Optimizations must preserve L0-L8.

## Required release artifacts

`engineering/state/release-report.json`, Rust test JUnit/summary, Bend verdict with hash/version, TUI interaction captures, protocol test reports, language matrix, macOS and Linux build evidence, Nix store derivation/closure size, binary size and RSS/startup data, license report, SBOM (if tool available), changelog and artifact checksums.

## Fail closed

A proof file existing is not proof. Running a command without checking its exit code is not verification. A unit test passing is not a demonstrated interactive workflow. A feature advertised by docs but not exercised on target runtime is unverified. Flag every known gap. Block final-release task until all mandatory artifacts exist with reproducible commands.

## CI

Host environments may not have Bend or Nix; CI jobs need explicit tool setup. Missing prerequisites must be reported as skipped/blocking and must not allow release gate to pass. GitHub CI template deliberately avoids a fictitious Bend installer without checksum/pinning. Add a pinned, audited Bend package to the Nix flake when available and verified on both targets.

Runtime evidence manifests are validated by `scripts/verify_language_matrix.py`; report paths and SHA-256 hashes for each actual language capability capture. This only checks evidence integrity, not whether a hostile agent forged the initial observation. Release checks additionally require all `IMPLEMENTED` declarations, Cargo.lock, flake.lock and performance report.
