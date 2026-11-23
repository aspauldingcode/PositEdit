# DLC3 architecture — fast, code-only, local Tab intelligence

## Behavioral contract

- On by default, immediately: status `Tab ON · Basic` before model initialization; status `Tab ON ✓` only with working model worker.
- Control located **at the far right of existing IDE's bottom status bar**, not floating over code.
- Left click toggles; Ctrl-Alt-T toggles; Shift-click opens settings. Preserve native Tab indentation when no prediction is visible.
- Predict only code edits, never prose chat. No unsolicited file writes, builds, shell commands or remote tool calls.
- Tab acceptance must use the host's existing revision-checked, atomic edit transaction and must not accept stale results.
- Optional dismiss: Esc. Suggestions are ghost text, never actual document contents until accepted.
- Disabling Tab stops scheduling and hides suggestions immediately; persist preference locally.

## Flow

```
Keystroke -> host scheduler (debounce, cancellation, priority)
         -> deterministic LSP/snippet/identifier fast path
         -> bounded context selector (current code, suffix, symbols)
         -> separate stdio JSON-Lines worker
         -> optional local llama.cpp /infill (Metal/other local accelerator)
         -> response validation, revision verification
         -> ghost text / Tab atomic accept
```

`local_tab` is a **reference, working control plane**, not a high-quality learned completion model; with no model weights it offers identifier completion only. The native Rust host must own async scheduling and input/render path. The Python stdio worker is transitional; profile overhead and consider replacing it with a Rust worker if measurable.

## IPC v1

Requests:
- `complete`: id, document_id, revision, language, prefix, suffix, context list, max_tokens (1–128).
- `cancel`: document_id + future revision, invalidates old responses.
- `set_enabled`: enabled Boolean.
- `health`: backend and status.

Responses: protocol, id, document_id, revision, status, suggestion, source, elapsed_ms.

The backend **must not** edit files. Revision checking occurs both at service receipt and at final Tab acceptance in the host. Local workers are allowed to return empty suggestions.

## Native Rust host integration requirements

1. Add the new `ide-local-tab-ui` crate to the Cargo workspace using a narrowly reviewed, additive manifest patch (not done by overlay installer).
2. Use the existing ratatui Frame and status layout; render `TabControl` *after* other widgets at far right.
3. Route mouse events to `TabControl.mouse_action` and Ctrl-Alt-T to its keyboard handler. Avoid global Tab interception except when a validated suggestion is active.
4. Persist on/off in an IDE-local setting, default `true` for new workspaces.
5. Create an async, supervised worker with bounded request queue and max one active prediction per document. Avoid blocking TUI event loop.
6. Start lexical fallback immediately; optionally start local model on first accepted idle inference request (lazy loading). No installation/downloading without authorization.
7. Feed language, cursor prefix/suffix, and bounded explicitly allowed context from Tree-sitter/LSP/Project Intelligence. Mask secrets/excluded paths and do not index outside workspace.
8. Check the document ID and revision again before *display* and before *accept*; check cursor position/selection and expected text anchors before applying edits.
9. Degrade to lexical or unavailable if worker crashes; never crash editor. Restart with backoff.
10. Integrate with existing model settings and throttle/unload on memory pressure (mobile/browser runtimes may already be consuming RAM).

## Code-only model architecture

Recommended starting candidates:
- Tiny: Qwen2.5-Coder 0.5B **base** model in compatible Q4 GGUF (quality/latency benchmark required).
- Default: Qwen2.5-Coder 1.5B **base** in compatible Q4 GGUF.
- Future: optional quality model or specialized edit-prediction distillation, **not** always-resident.

The model must support true FIM infilling. Base model selection and compatibility require checking exact tokenization, serving endpoint and licensing. No model weights are shipped in this DLC.

## Performance engineering (targets, not results)

- Editor input-to-render p95 <= 16 ms, even under model load.
- Deterministic identifier completion p95 <= 10 ms after context prepared.
- Model suggestion p50 <= 250 ms and p95 <= 500 ms on designated Apple Silicon target, measured, not assumed.
- Default model weight target 0.9–1.3 GB; total memory depends on KV cache, Metal buffers, server.
- Zero cloud calls; low idle CPU; bounded queue; one prediction at a time; cancel obsolete responses; 32–64 generated tokens typical.
- Context: 1–3K tokens initially; increase only where it improves accepted-edit quality.
- Energy: sample CPU/GPU and battery impact. Stop inference when unfocused, in resource pressure, or while rapid typing.

## Native platform backends

- macOS/Apple Silicon: llama.cpp with Metal acceleration; benchmark MLX variant separately.
- Linux: llama.cpp with CUDA/Vulkan/CPU depending on available hardware.
- No mandatory Ollama daemon, cloud service, Python environment, or model download for baseline editor operation; the Python worker in this ZIP is an intermediate integration reference.

## Model quality evaluation

Benchmark per language on held-out code for Python, Rust, C, Zig, Nix, Java, Kotlin, Swift, TypeScript, JavaScript. Use code completion edit fixtures, not chat benchmarks alone. Measure exact-match where appropriate, edit-similarity, valid syntax, accepts in realistic sessions (opt-in), latency and watts. Compare identical task sets to Cursor/Copilot only where comparisons are legally and technically possible. Do not claim parity based only on model size or subjective impressions.

## Security

- Request model is code-only. No shell execution, network queries, or agentic tools in inline Tab path.
- Refuse non-loopback model endpoint and ignore proxy environment settings.
- Never pass credentials, outside-workspace files, or ignored paths into context.
- Do not save personal edits for training without explicit opt-in.
- Worker process can be restricted with OS sandbox/firewall; the loopback check alone does not prove zero egress by the model server.
