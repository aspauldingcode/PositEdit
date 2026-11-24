# ADR: Additive Bend laws for document, minimap, and scroll

- Status: proposed (additive Bend modules already under `bend/{doc,minimap,scroll}/`)
- Date: 2026-11-22
- Affected components: `ide-core` Document revision gate, `ide-tui` proportional
  minimap + scrollbar metrics, flake `bend-laws` check

## Problem

Accepted root `LAWS.bend` only covers job cancellation. Editor correctness for
revision monotonicity, stale-apply rejection, VS Code–proportional minimap
culling, and scrollbar clamps lived only in Rust tests. The user asked to push
as much of the codebase as possible into Bend 2 so behavior is mathematically
checked, without silently rewriting owner-controlled root laws.

## Options

1. Edit root `LAWS.bend` / `SPEC.bend` in place — forbidden without owner accept.
2. Keep Rust-only property tests — weaker than Bend `--verdict`.
3. Additive Bend modules + Rust conformance mirrors + promote later — chosen.

## Chosen design

| Module | Model | Laws (summary) | Rust mirror |
|--------|-------|----------------|-------------|
| `bend/doc/` | revision gate `decide` / `apply` | fresh bumps rev+1; stale preserves | `Document::apply` |
| `bend/minimap/` | proportional px/line + sliding window | scale=1→8px; fit vs slide; click in window | `minimap_gfx::layout_proportional` |
| `bend/scroll/` | max_offset + track map | clamp identity/cap; track ends | `ScrollMetrics` |
| existing | `bend/model.bend` job cancel | root `LAWS.bend` | process/supervisor tests |
| existing | `bend/palette/`, `bend/dlc/` | vim refusal / trust | `palette.rs`, DLC authorize |

Root `LAWS.bend` and `SPEC.bend` are **unchanged**. Promotion into root laws
requires an explicit owner accept of this proposal (or a successor).

## Verification

```sh
sh ./scripts/check_bend.sh
cargo test -p ide-core --lib bend_doc_laws
cargo test -p ide-tui --lib bend_minimap_laws
cargo test -p ide-tui --lib bend_scroll_laws
```

Flake `checks.bend-laws` should also run the additive proof files.

## Migration / rollback

Additive only. Delete `bend/{doc,minimap,scroll}` and the conformance tests to
roll back. No runtime feature flags required — models are proof-time artifacts
with differential Rust tests.
