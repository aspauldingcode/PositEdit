# Technical and model references (checked at DLC design time)

- llama.cpp: https://github.com/ggml-org/llama.cpp
- llama-server `/infill` and caching documentation: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md
- Metal build support: https://github.com/ggml-org/llama.cpp/blob/master/docs/build.md
- Qwen2.5-Coder upstream code models: https://github.com/QwenLM/Qwen2.5-Coder
- Qwen2.5-Coder 0.5B: https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B
- Qwen2.5-Coder 1.5B: https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B
- Apple MLX LM: https://github.com/ml-explore/mlx-lm
- Hugging Face Candle: https://github.com/huggingface/candle
- Tabby repository-aware completion: https://github.com/TabbyML/tabby
- Continue code autocomplete: https://docs.continue.dev/customize/deep-dives/autocomplete
- Zed edit prediction: https://zed.dev/docs/ai/edit-prediction
- Kitty graphics protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- ratatui: https://ratatui.rs/
- crossterm: https://github.com/crossterm-rs/crossterm
- Bend programming language: https://github.com/HigherOrderCO/Bend

Validate upstream CLI/API versions at integration time. No external source/model code is vendored into this DLC.
