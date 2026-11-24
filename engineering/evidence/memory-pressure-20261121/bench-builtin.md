# Built-in first-party benches (2026-11-21)

## Runner
```bash
cargo run --release -- --bench
```

Suites: `memory`, `edit`, `render`, `open` (also `--bench-memory` alias).

## Release results (host Urgent pressure, 16 GiB)

See `bench-all-release.txt` for the latest raw run. Expected shape after viewport paint + typing coalesce:

- memory: undo capped 64 → 8 under critical trim — PASS
- edit: coalesced 1k chars ~few ms; scatter inserts <80ms — PASS  
- render: draw 120×40 p95 under 40ms — PASS
- open: ~1 MiB plain load+paint near ≤100ms (pressure slack ×1.25 when Urgent)

## Optimizations paired with benches
1. Typing undo coalescing (one snapshot per word)
2. Single-edit fast path in `Document::apply`
3. Text + highlight caches on `EditBuffer`
4. Viewport-limited paint (no full-buffer `Text` materialization)
5. OS memory-pressure trim (existing)
