# ADR: Palette compiler refuses Vim quit spellings

- Status: proposed (additive Bend laws already under `bend/palette/`)
- Graph node IDs: C02 / C03 (TUI commands, workspace command engine)
- Date and affected components: 2026-11-20; `crates/ide-tui`, `bend/palette`
- Problem and constraints:
  Users type `:q` / `q` / `:wq` expecting Vim. Posit Edit is not Vim. A vague
  `unknown command: q` misleads; silently quitting would be worse.
- Options and trade-offs:
  1. Treat `:q` as `quit` — too surprising; Posit quit is `q` in view mode or `quit`.
  2. Keep `unknown command` — unclear.
  3. Closed-token palette compiler with Bend laws — chosen.
- Chosen design and why:
  Normalize palette input (trim, one leading `:`), classify into a closed token
  set, interpret to a single effect. Vim quit tokens map to `RefuseVimQuit` /
  `RefuseVimWriteQuit` and never to `RequestQuit`. Only the exact token `quit`
  requests exit from the palette.
- Law and proof impact:
  Additive laws in `bend/palette/LAWS.bend` (do not edit accepted root
  `LAWS.bend` until owner accepts). Proofs in `bend/palette/PROOF.bend`.
  Rust mirror in `crates/ide-tui/src/palette.rs` with conformance tests.
- Migration and rollback:
  Additive. Remove `bend/palette` and revert `palette.rs` wiring to roll back.
- Verification required:
  - `bend bend/palette/PROOF.bend --verdict`
  - `cargo test -p ide-tui --lib palette::`
  - TUI: `:q` shows the refusal notice and does not quit; `quit` still quits.
- Evidence links:
  - `bend/palette/state.bend`
  - `crates/ide-tui/src/palette.rs`
