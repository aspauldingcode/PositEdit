# Model acquisition and selection

No model weights are included or automatically fetched.

## Default benchmark candidates

- Qwen/Qwen2.5-Coder-0.5B (base): tiny coding model; evaluate latency on 8 GB machines.
- Qwen/Qwen2.5-Coder-1.5B (base): initial quality target for Apple Silicon M1 16 GB; use a *compatible* GGUF quantization with FIM token support.
- Avoid chat-only instruct models for default FIM, unless benchmarked and a compatible template proven.

Official sources:
- https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B
- https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B
- https://github.com/ggml-org/llama.cpp/tree/master/tools/server
- https://github.com/ggml-org/llama.cpp/blob/master/docs/build.md

Before download, check model license, redistribution terms, file provenance, checksums and available disk. The host coding agent may install a model after the operator permits network acquisition. Use a dedicated local model directory (e.g. `${XDG_DATA_HOME:-~/.local/share}/terminal-ide/models`) and do not write to arbitrary locations.

Latency and quality targets are unverified. A completed DLC should generate and publish benchmark logs on actual devices.
