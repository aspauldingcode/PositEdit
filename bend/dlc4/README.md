# DLC4 Bend policies

Pure spatial policies live here. Rust remains the default runtime path.

Differential fixtures are exercised in `crates/ide-spatial` unit tests
(`clamp_jump`, `pick_priority`, `next_nest`, `rank_score`, `validate_snapshot`,
`timestamps_ordered`, `reachable`).

`bend` was not available on the integration host during DLC4 implementation;
these files are prototypes for later gated switchover. Root `LAWS.bend` and
`PROOF.bend` are untouched.
