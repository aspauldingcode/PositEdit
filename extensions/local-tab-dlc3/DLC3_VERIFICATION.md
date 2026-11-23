# DLC3 verification ledger

The ZIP's Python stdlib engine and non-overwrite installer are unit-testable on this build host. Rust compilation, real Metal model latency, Cursor/Copilot quality parity, local model integration, native IDE TUI wiring, and Bend proofs require the receiving host and its toolchains.

Run:

```
PYTHONPATH=. python3 -m unittest discover -s tests -v
python3 -m compileall -q local_tab scripts
python3 -m local_tab   # interactive JSONL; pipe health then completion
```

Success criteria for real IDE integration:

- [ ] Rust crate compiles and existing workspace tests pass.
- [ ] Bottom-right button appears inside actual editor, defaults ON.
- [ ] Click/Ctrl-Alt-T toggles correctly; settings opens; state persists.
- [ ] Ghost text renders without mutating buffer until Tab accepted.
- [ ] Tab commits only revision-matched suggestion; Esc dismisses.
- [ ] Model weights acquired with explicit approval and verified checksums.
- [ ] Actual FIM generation locally with Metal GPU acceleration on M1.
- [ ] No external completion requests; network isolation audited.
- [ ] Language matrix validated with held-out code fixtures.
- [ ] Cursor/Copilot parity evaluated on comparable tasks; results reported, no assumption of parity.
- [ ] Memory, latency, energy, cancellation, and crash isolation measured.
- [ ] Bend 2 verified proof and differential evidence produced if Bend implementation migrated.
