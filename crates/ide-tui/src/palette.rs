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
