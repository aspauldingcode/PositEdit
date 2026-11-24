# Memory-pressure efficiency (2026-11-21)

## Host
- arch: arm64 (Apple Silicon)
- macOS: 27.2.1 (26B131)
- hw.memsize: 17179869184 (16 GiB)
- `kern.memorystatus_vm_pressure_level` during run: **2 (Urgent)**

## Changes
1. **Bounded undo/redo** (`ide-core`): hard cap 128, normal 64, critical 8.
2. **OS pressure probe**: `kern.memorystatus_vm_pressure_level` → trim document history + PTY scrollback every 2s in the editor tick.
3. **PTY scrollback**: `VecDeque` with pressure-scaled limit (500 → 200 → 80); O(1) eviction.
4. **Console trim** under elevated pressure.
5. **`--bench-memory`**: headless edit storm + critical trim + RSS sample.

## Measurements (`cargo run -- --bench-memory`, n=5)
All PASS on Urgent host pressure:

| sample | undo_after_storm | undo_after_critical | rss_before_KiB | rss_after_KiB | delta_KiB |
|--------|------------------|---------------------|----------------|---------------|-----------|
| 1 | 64 | 8 | 2208 | 5056 | 2848 |
| 2 | 64 | 8 | 2208 | 5104 | 2896 |
| 3 | 64 | 8 | 2208 | 5088 | 2880 |
| 4 | 64 | 8 | 2208 | 5088 | 2880 |
| 5 | 64 | 8 | 2208 | 5088 | 2880 |

Median delta ≈ 2880 KiB for a ~160 KiB text buffer + 1500 edits (history capped).

## Unit gates
- `cargo test -p ide-core --lib` → 17 passed (includes edit-storm + pressure compact).
- `terminal::tests::scrollback_stays_bounded_under_line_storm` → passed.
- Full `ide-tui` suite: clipboard pasteboard can still race with *external* macOS apps under Urgent pressure; in-process clipboard access is now mutex-serialized.

## How to re-run
```bash
cargo test -p ide-core --lib
cargo run -- --bench-memory
```
