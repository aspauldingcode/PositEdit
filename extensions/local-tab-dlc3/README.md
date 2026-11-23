# DLC 3 — Local Tab Intelligence (additive extension)

## Exactly what this DLC is

- A **working local-only completion worker** implemented in Python stdlib, using bounded JSON-lines over stdin/stdout. No network by default, model-free identifier fallback immediately usable.
- An **optional local llama.cpp `/infill` adapter** restricted to loopback URLs (and proxies disabled); external model weights/llama.cpp are **not bundled**.
- An **additive Rust `ratatui` status control**: bottom-right `Tab ON · Basic` initially, default enabled; click toggles; Ctrl-Alt-T toggles; Shift-click requests settings. Host wiring required.
- A **non-overwriting overlay installer**, Python unit tests, architecture, migration laws and graph tasks, and complete agent integration prompt.

This is *not* a finished Cursor Tab clone. Cursor/Copilot-quality predictions require model downloads, platform testing, tuning, benchmarks, feedback datasets, next-edit training, and real host IDE wiring. None of those are falsely claimed as finished here.

## Run without any model (offline)

```bash
cd universal-ide-local-tab-dlc3
python3 -m local_tab
```

Send newline-separated JSON:

```json
{"op":"complete","id":"1","document_id":"a.rs","revision":1,"language":"rust","prefix":"let config_value = 1;\nconf","suffix":""}
```

Response resembles:

```json
{"protocol":1,"id":"1","document_id":"a.rs","revision":1,"status":"ok","suggestion":"ig_value","source":"lexical","elapsed_ms":0.1}
```

The example elapsed time is illustrative, not a measured benchmark.

## Run with a compatible local FIM model

Use a verified **code-only, fill-in-the-middle-capable** local GGUF model (for example a suitable *base*, not chat, Qwen2.5-Coder 0.5B/1.5B quantization). Verify model and license. Install llama.cpp with Metal on supported Apple Silicon. Supply the local file path yourself:

```bash
llama-server -m /absolute/path/to/your-fim-code-model.gguf \
  --host 127.0.0.1 --port 8137 -ngl 99 -c 4096
python3 -m local_tab --local-llama
```

The worker communicates only with `http://127.0.0.1:8137/infill`, refusing remote endpoints. **Loopback binding is not a full OS-enforced egress sandbox**; hardened releases must audit/model-sandbox the server process and verify network policy at OS level. Model acquisition is a separate user-authorized step.

## Preview additive installation

```bash
python3 scripts/install_overlay.py --host ~/one-shot-terminal-ide
```

This **does not write anything**. After checking the plan:

```bash
python3 scripts/install_overlay.py --host ~/one-shot-terminal-ide --apply
```

This copies only new files under `crates/ide-local-tab-ui/` and `extensions/local-tab-dlc3/`. It **does not modify Cargo.toml, Rust source, AGENTS.md, LAWS.bend, or PROOF.bend** and refuses any collisions. You or an authorized coding agent must wire the crate, worker, and editor events together. Re-running `--apply` refuses rather than overwrites.

## Test

```bash
PYTHONPATH=. python3 -m unittest discover -s tests -v
python3 -m compileall -q local_tab scripts
```

## Key documentation

- `DLC3_AGENT_PROMPT.md`: copy into an authorized coding agent after downloading ZIP.
- `DLC3_ARCHITECTURE.md`: precise contracts, Rust/Bend boundary, privacy, model path and scheduler.
- `DLC3_VERIFICATION.md`: what is tested / not tested.
- `DLC3_MIGRATION.md`: Rust-to-Bend migration gates.
- `REFERENCES.md`: official source links.
- `model/MODEL_SELECTION.md`: model profiles, resource strategy and licensing.
