# Posit Edit built-in benchmarks

First-party benches ship inside the `posit-edit` binary — no Criterion / external harness required.

```bash
cargo run --release -- --bench          # all suites
cargo run --release -- --bench memory
cargo run --release -- --bench edit
cargo run --release -- --bench render
cargo run --release -- --bench open
cargo run --release -- --bench-memory   # alias for memory
```

Methodology (`BUDGETS.md`): warmup discarded, ≥5 timed samples, report median + p95.

| Suite   | What it measures                         | Gate (p95)   |
|---------|------------------------------------------|--------------|
| memory  | Undo caps under edit storm + pressure trim | structural   |
| edit    | Coalesced typing + scatter inserts       | 50ms / 80ms  |
| render  | Full TUI draw 120×40 with highlight      | 40ms         |
| open    | Load fixture + first paint               | 100ms        |
