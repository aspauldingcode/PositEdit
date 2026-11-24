# DLC 3 local Tab — evidence

Date: 2026-11-20. Host: Apple M1. Python: 3.9.6. Git checkpoint: `f98bae45f044fc575dc51801f1befe3168133282` worktree `.dlc-staging/worktree-dlc3-f98bae4`.

Archive: `~/Downloads/universal-ide-local-tab-dlc3.zip` (unzip -t clean; 29 entries; no absolute paths, `..`, symlinks, or duplicate names).

Extracted to: `.dlc-staging/20261121T001551Z-local-tab-dlc3/`. Extraction overwrites of the IDE: 0.

Installer dry-run then `--apply` copied 19 new files. A second apply refuses because destinations exist. System Python 3.9 lacks `Path.stat(follow_symlinks=...)`. The first apply used an in-process `os.lstat` shim. The staged `scripts/install_overlay.py` was then changed to `os.lstat` so its own tests run. That patch is not in the installed tree.

## Verified

- `PYTHONPATH=. python3 -m unittest discover -s tests -v` from the staged DLC: 32 tests, OK.
- `cargo test -p ide-tui --lib`: 39 passed.
- `cargo test -p ide-local-tab-ui`: crate tests included in the UI crate (3).
- Default control state is enabled. Rendered status includes `Tab ON` while the lexical fallback is active.
- Tab accepts one revision-checked insert (`width` from `wid`). A stale ghost is not inserted; Tab then inserts a literal tab.
- Esc clears ghost text before leaving edit mode.
- Ctrl-Alt-T toggles and writes `.posit/local-tab.json` (gitignored). Shift-click on the button opens Settings.
- Lexical Python fallback on this M1: p50 0.0035 ms, p95 0.0049 ms over 200 calls. This is not model inference.
- Secret-looking names (`.env`, `.key`, `credential`, `secret`, `.pem`) are omitted from in-process context.
- `LAWS.bend`, `PROOF.bend`, and `SPEC.bend` were not modified.
- Existing workbench tests (build/run preview, toolbar) still pass.

## Not verified

- Live terminal mouse click (logic is unit-tested via geometry, not a PTY session).
- Metal, llama.cpp, or any GGUF. No weights were downloaded. Qwen2.5-Coder 0.5B and 1.5B were not benchmarked.
- Outbound-traffic capture of a model process. The lexical path does not open a socket. The Python backend rejects non-loopback `/infill` URLs (unit-tested).
- `bend` is not on `PATH`, so `DLC3_LAWS.bend` / `DLC3_PROOF.bend` stay non-executable placeholders.
- `nix flake check` was not run in this pass.
- Language quality is the same identifier fallback for every language. There is no per-language FIM score.

## Not implemented

- On-device FIM (Ready status). Status stays `Tab ON · Basic`.
- Idle unload, GPU utilization, and battery samples.
- Next-edit prediction.
