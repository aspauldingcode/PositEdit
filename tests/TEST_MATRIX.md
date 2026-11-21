# TUI / language capability test matrix

No blank cell is evidence. Record versions, execution command, stdout/trace, screenshots/text captures and result.

| Target | Parse/fallback | LSP | Build | DAP | Test | TUI verified |
|---|---|---|---|---|---|---|
| Python | pending | pending | pending | pending | pending | pending |
| Rust | pending | pending | pending | pending | pending | pending |
| C | pending | pending | pending | pending | pending | pending |
| Zig | pending | pending | pending | pending | pending | pending |
| Nix | pending | pending | pending | not generally available | pending | pending |
| Java | pending | pending | pending | pending | pending | pending |
| Kotlin | pending | pending | pending | pending | pending | pending |
| Swift | pending | pending | pending | pending | pending | pending |
| TypeScript | pending | pending | pending | pending | pending | pending |
| JavaScript | pending | pending | pending | pending | pending | pending |
| Unknown extension | pending | N/A | custom task | custom adapter | custom task | pending |

Terminal dimensions: 80x24, 120x40, narrow 50x16; color capabilities: truecolor, 256, ANSI16; Unicode: combining marks, emoji, CJK, CRLF; keyboard Ctrl/Alt/Shift, bracketed paste, resize, Ctrl-Q recovery; PTY tests should use real tmux capture-pane, not simulated screenshots.
