//! Palette command compiler.
//!
//! Mirrors `bend/palette/state.bend`. Strings are normalized here; Bend reasons
//! only over the closed [`Command`] token set. Vim quit spellings never become
//! [`Effect::RequestQuit`].

/// Exact notice for Vim `:q` / `q` (and force / all-buffer variants).
pub const VIM_QUIT_NOTICE: &str = "Ha, nice try. This isn't vim. Close by just pressing q, or type 'quit' in the command palette.";

/// Closed token set — same constructors as `bend/palette/state.bend` `Command`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Quit,
    VimQuit,
    VimWriteQuit,
    Split,
    Undo,
    Format,
    Help,
    Files,
    Find,
    Grep,
    New,
    Save,
    Unknown,
}

/// Host effect — same constructors as `bend/palette/state.bend` `Effect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    RequestQuit,
    RefuseVimQuit,
    RefuseVimWriteQuit,
    ToggleSplit,
    UndoEdit,
    FormatBuffer,
    ShowHelp,
    OpenFiles,
    FindFiles,
    GrepWorkspace,
    NewBuffer,
    SaveBuffer,
    UnknownCommand,
}

/// Trim edges and at most one leading `:`. Does not lower-case; tokens stay exact.
pub fn normalize(input: &str) -> &str {
    let trimmed = input.trim();
    trimmed
        .strip_prefix(':')
        .map(str::trim)
        .unwrap_or(trimmed)
}

/// Classify a normalized spelling into the closed token set.
///
/// Vim quit family (`q`, `q!`, `qa`, `qa!`) → [`Command::VimQuit`].
/// Vim write-quit family (`wq`, `wq!`, `x`) → [`Command::VimWriteQuit`].
/// Only the exact spelling `quit` → [`Command::Quit`].
pub fn classify(normalized: &str) -> Command {
    match normalized {
        "quit" => Command::Quit,
        "q" | "q!" | "qa" | "qa!" => Command::VimQuit,
        "wq" | "wq!" | "x" => Command::VimWriteQuit,
        "split" => Command::Split,
        "undo" => Command::Undo,
        "format" => Command::Format,
        "help" => Command::Help,
        "files" | "open" | "yazi" => Command::Files,
        "find" => Command::Find,
        "grep" => Command::Grep,
        "new" => Command::New,
        "save" => Command::Save,
        _ => Command::Unknown,
    }
}

/// Same decision as `bend/palette/state.bend` `interpret`.
pub fn interpret(command: Command) -> Effect {
    match command {
        Command::Quit => Effect::RequestQuit,
        Command::VimQuit => Effect::RefuseVimQuit,
        Command::VimWriteQuit => Effect::RefuseVimWriteQuit,
        Command::Split => Effect::ToggleSplit,
        Command::Undo => Effect::UndoEdit,
        Command::Format => Effect::FormatBuffer,
        Command::Help => Effect::ShowHelp,
        Command::Files => Effect::OpenFiles,
        Command::Find => Effect::FindFiles,
        Command::Grep => Effect::GrepWorkspace,
        Command::New => Effect::NewBuffer,
        Command::Save => Effect::SaveBuffer,
        Command::Unknown => Effect::UnknownCommand,
    }
}

/// Full compiler pipeline: raw palette text → effect.
pub fn compile(input: &str) -> (Command, Effect) {
    let command = classify(normalize(input));
    (command, interpret(command))
}

/// Spellings the palette accepts (Bend tokens + host extras like `minimap`).
/// Used only for "did you mean" on unknown input — never widens `classify`.
pub const KNOWN_SPELLINGS: &[&str] = &[
    "quit",
    "split",
    "undo",
    "format",
    "help",
    "files",
    "open",
    "yazi",
    "find",
    "grep",
    "new",
    "save",
    "rn",
    "relative",
    "relativenumber",
    "hints",
    "hover-hints",
    "freeze",
    "canvas-freeze",
    "cf",
    "thaw",
    "canvas-thaw",
    "unfreeze",
    "minimap",
    "mm",
];

/// Best near-miss for an unknown palette token, if any.
///
/// Prefers unique prefixes, then tiny edit distance (typos like `mimimap` /
/// `frmat`). Returns `None` when nothing is close enough to suggest.
pub fn suggest(raw: &str) -> Option<&'static str> {
    let query = normalize(raw).to_ascii_lowercase();
    if query.is_empty() {
        return None;
    }
    // Unique prefix — `:mini` → `minimap`.
    let prefixes: Vec<&'static str> = KNOWN_SPELLINGS
        .iter()
        .copied()
        .filter(|spelling| spelling.starts_with(query.as_str()) && *spelling != query.as_str())
        .collect();
    if prefixes.len() == 1 {
        return Some(prefixes[0]);
    }
    // One-letter typos are noise; need enough of the word to guess.
    if query.len() < 3 {
        return None;
    }
    let mut best: Option<(usize, usize, &'static str)> = None;
    for &spelling in KNOWN_SPELLINGS {
        if spelling == query.as_str() {
            continue;
        }
        let distance = edit_distance(&query, spelling);
        let max = if spelling.len() <= 4 { 1 } else { 2 };
        if distance == 0 || distance > max {
            continue;
        }
        let len_gap = query.len().abs_diff(spelling.len());
        let candidate = (distance, len_gap, spelling);
        if best.is_none_or(|cur| candidate < cur) {
            best = Some(candidate);
        }
    }
    best.map(|(_, _, spelling)| spelling)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let a: Vec<char> = left.chars().collect();
    let b: Vec<char> = right.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_one_colon_and_spaces() {
        assert_eq!(normalize("  :q  "), "q");
        assert_eq!(normalize(":wq"), "wq");
        assert_eq!(normalize("quit"), "quit");
        assert_eq!(normalize("::q"), ":q");
    }

    #[test]
    fn vim_quit_spellings_never_request_exit() {
        for spelling in ["q", ":q", " q ", ":q!", "qa", ":qa!", "Q"] {
            let (command, effect) = compile(spelling);
            if spelling == "Q" {
                // Case-sensitive: capital Q is unknown, still not quit.
                assert_eq!(command, Command::Unknown);
                assert_eq!(effect, Effect::UnknownCommand);
            } else {
                assert_eq!(command, Command::VimQuit, "{spelling}");
                assert_eq!(effect, Effect::RefuseVimQuit, "{spelling}");
            }
            assert_ne!(effect, Effect::RequestQuit, "{spelling}");
        }
    }

    #[test]
    fn vim_write_quit_spellings_never_request_exit() {
        for spelling in ["wq", ":wq", "wq!", ":x", "x"] {
            let (command, effect) = compile(spelling);
            assert_eq!(command, Command::VimWriteQuit, "{spelling}");
            assert_eq!(effect, Effect::RefuseVimWriteQuit, "{spelling}");
            assert_ne!(effect, Effect::RequestQuit, "{spelling}");
        }
    }

    #[test]
    fn only_exact_quit_requests_exit() {
        assert_eq!(compile("quit"), (Command::Quit, Effect::RequestQuit));
        assert_eq!(compile(":quit").1, Effect::RequestQuit);
        assert_ne!(compile("quit!").1, Effect::RequestQuit);
        assert_ne!(compile("QUIT").1, Effect::RequestQuit);
    }

    #[test]
    fn suggest_catches_typos_and_unique_prefixes() {
        assert_eq!(suggest("mimimap"), Some("minimap"));
        assert_eq!(suggest("frmat"), Some("format"));
        assert_eq!(suggest("mini"), Some("minimap"));
        assert_eq!(suggest("definitely-not-a-command"), None);
        assert_eq!(suggest("mm"), None, "exact known spelling is not a suggestion");
    }

    #[test]
    fn bend_laws_conformance() {
        // quit_requests_exit
        assert_eq!(interpret(Command::Quit), Effect::RequestQuit);
        // vim_quit_refuses_exit
        assert_eq!(interpret(Command::VimQuit), Effect::RefuseVimQuit);
        // vim_write_quit_refuses_exit
        assert_eq!(interpret(Command::VimWriteQuit), Effect::RefuseVimWriteQuit);
        // unknown_does_not_quit
        assert_eq!(interpret(Command::Unknown), Effect::UnknownCommand);
        // interpret_idempotent_*
        assert_eq!(
            interpret(Command::Quit),
            interpret(Command::Quit)
        );
        assert_eq!(
            interpret(Command::VimQuit),
            interpret(Command::VimQuit)
        );
    }
}
