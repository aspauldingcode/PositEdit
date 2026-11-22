# Performance budget and measurement methodology

Proposed targets, not measured claims:
- core binary <=10 MiB stripped; idle RSS <=30 MiB excluding child language servers.
- warm-start to interactive <=50ms on designated Apple Silicon Mac.
- p95 input-to-frame <=16ms on 80x24 and 120x40; near-zero idle CPU.
- opening 1 MiB file <=100ms in typical project; command palette <=30ms cached paths.

Benchmark repeat count >=5, preserve raw observations, median, p95, sample spread; capture OS/kernel, CPU/arch, memory, terminal emulator, power conditions, compiler flags and child process list. Flag performance regressions separately from invariant violations. Favor representative fixture projects, no artificial preloaded caches unless clearly labeled warm.

## Built-in runner (first-party)

```bash
cargo run --release -- --bench          # memory + edit + render + open
cargo run --release -- --bench edit
```

Debug builds apply a ×5 gate scale. Prefer `--release` for budget claims. See `benchmarks/README.md`.
