# ADR: Additive Bend laws for Flip 3D Tab hold/release

- Status: proposed (additive module under `bend/flip/`)
- Date: 2026-11-23
- Affected: `ide-tui` FlipSwitcher, flake `bend-laws` / `scripts/check_bend.sh`

## Problem

Tab-hold Flip 3D committed focus asynchronously and lost key-up tracking when
sync preview warm ran between Tab Press and Tab Release in the input batch.
Root `LAWS.bend` must stay owner-locked; Flip focus machine needs Bend coverage.

## Chosen design

| Module | Model | Laws | Rust mirror |
|--------|-------|------|-------------|
| `bend/flip/` | open/flip/release/cancel | release commits selected; cancel restores | `FlipSwitcher::{commit,cancel}_focus` |

Root `LAWS.bend` / `SPEC.bend` unchanged.

## Verification

```sh
sh ./scripts/check_bend.sh
cargo test -p ide-tui --lib bend_flip flip3d_tab_press_release
```
