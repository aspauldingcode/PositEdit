//! Language detection, tree-sitter highlighting, and manual formatting.
//! Formatters run only when `format_manually` is called.
mod extra;
mod format;
mod highlight;

pub use format::{format_manually, format_with, formatter_for, FormatOutcome, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

pub const COMPONENT: &str = "ide-syntax";
pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    NotInstalled,
    Ready,
    Unsupported,
}

pub fn status() -> Availability {
    Availability::Ready
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageProfile {
    pub id: String,
    pub extensions: Vec<String>,
    pub formatter: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    Comment,
    String,
    Type,
    Function,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

pub fn load_profiles(dir: &Path) -> Vec<LanguageProfile> {
    let mut profiles = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return profiles,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            if let Some(profile) = parse_profile(&text) {
                profiles.push(profile);
            }
        }
    }
    profiles.sort_by(|left, right| left.id.cmp(&right.id));
    profiles
}

fn parse_profile(text: &str) -> Option<LanguageProfile> {
    let mut id = None;
    let mut extensions = Vec::new();
    let mut formatter = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("id =") {
            id = Some(unquote(value.trim()).to_string());
        } else if let Some(value) = line.strip_prefix("extensions =") {
            extensions = value
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|item| unquote(item.trim()).to_string())
                .filter(|item| !item.is_empty())
                .collect();
        } else if let Some(value) = line.strip_prefix("formatter =") {
            let tool = unquote(value.trim()).to_string();
            if !tool.is_empty() {
                formatter = Some(tool);
            }
        }
    }
    Some(LanguageProfile {
        id: id?,
        extensions,
        formatter,
    })
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"')
}

pub fn detect(profiles: &[LanguageProfile], path: Option<&Path>, text: &str) -> String {
    if let Some(path) = path {
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            let ext = ext.to_ascii_lowercase();
            if let Some(profile) = profiles
                .iter()
                .find(|profile| profile.extensions.iter().any(|item| item == &ext))
            {
                return profile.id.clone();
            }
        }
        if let Some(language) = extra::language_for_path(path) {
            return language.to_string();
        }
    }
    if let Some(language) = shebang_language(text) {
        return language;
    }
    if text.contains("fn main") && text.contains("let ") {
        return "rust".to_string();
    }
    "plain".to_string()
}

fn shebang_language(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    if !first.starts_with("#!") {
        return None;
    }
    let token = first.split_whitespace().last().unwrap_or("");
    let name = Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(token);
    match name {
        "python" | "python3" => Some("python".to_string()),
        "node" => Some("javascript".to_string()),
        "nix-shell" | "nix" => Some("nix".to_string()),
        "bash" | "sh" | "zsh" => Some("shell".to_string()),
        _ => None,
    }
}

pub fn highlight(language: &str, text: &str) -> Vec<Span> {
    highlight::highlight_source(language, text)
}

pub fn bundled_language_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../languages")
}

pub fn grammar_status() -> Availability {
    if highlight::grammar_ready() {
        Availability::Ready
    } else {
        Availability::NotInstalled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Vec<LanguageProfile> {
        load_profiles(&bundled_language_dir())
    }

    #[test]
    fn loads_shipped_profiles() {
        let profiles = profiles();
        assert!(profiles.iter().any(|profile| profile.id == "rust"));
        assert!(profiles
            .iter()
            .any(|profile| profile.extensions.iter().any(|ext| ext == "py")));
    }

    #[test]
    fn extension_beats_shebang_and_unknown_stays_plain() {
        let profiles = profiles();
        let path = Path::new("main.rs");
        assert_eq!(
            detect(
                &profiles,
                Some(path),
                "#!/usr/bin/env python3\nfn main(){}\n"
            ),
            "rust"
        );
        assert_eq!(
            detect(&profiles, None, "#!/usr/bin/env python3\nprint(1)\n"),
            "python"
        );
        assert_eq!(
            detect(&profiles, Some(Path::new("notes.unknown")), "fn main() {}"),
            "plain"
        );
    }

    #[test]
    fn plain_text_is_not_labeled_as_code() {
        let spans = highlight("plain", "fn main() {}");
        assert!(spans.iter().all(|span| span.kind == TokenKind::Plain));
        assert_eq!(grammar_status(), Availability::Ready);
    }

    #[test]
    fn tree_sitter_keeps_keywords_inside_strings() {
        let rust = "fn main() {\n    let message = \"fn hidden\";\n}\n";
        let spans = highlight("rust", rust);
        assert_eq!(
            highlight::covers(rust, &spans, "fn"),
            Some(TokenKind::Keyword)
        );
        assert_eq!(
            highlight::covers(rust, &spans, "fn hidden"),
            Some(TokenKind::String)
        );
        let python = "text = '''\ndef hidden():\n    pass\n'''\ndef shown():\n    pass\n";
        let spans = highlight("python", python);
        assert_eq!(
            highlight::covers(python, &spans, "def hidden"),
            Some(TokenKind::String)
        );
        let shown = python.rfind("def shown").unwrap();
        assert!(spans.iter().any(|span| {
            span.kind == TokenKind::Keyword && span.start <= shown && span.end >= shown + 3
        }));
    }

    #[test]
    fn every_shipped_language_parses_a_keyword() {
        let samples = [
            ("rust", "fn main() {}"),
            ("python", "def shown():\n    return 1\n"),
            ("c", "int main(void) { return 0; }\n"),
            ("javascript", "function shown() { return 1; }\n"),
            ("typescript", "function shown(): number { return 1; }\n"),
            ("java", "class Shown { int value; }\n"),
            ("kotlin", "fun shown(): Int { return 1 }\n"),
            ("swift", "func shown() -> Int { return 1 }\n"),
            ("zig", "pub fn shown() void {}\n"),
            ("nix", "let answer = 1; in answer\n"),
        ];
        for (language, source) in samples {
            let spans = highlight(language, source);
            assert!(
                spans.iter().any(|span| span.kind == TokenKind::Keyword),
                "{language} produced no keyword span: {spans:?}"
            );
        }
    }

    #[test]
    fn markdown_shell_toml_json_and_bend_are_colored() {
        let bend = "def Laws.cancel(state):\n  match state:\n    case Job.Queued{}:\n";
        let spans = highlight("bend", bend);
        assert_eq!(
            highlight::covers(bend, &spans, "def"),
            Some(TokenKind::Keyword)
        );
        assert_eq!(
            highlight::covers(bend, &spans, "match"),
            Some(TokenKind::Keyword)
        );
        assert_eq!(
            highlight::covers(bend, &spans, "case"),
            Some(TokenKind::Keyword)
        );
        let markdown = "# Title\n`code`\n";
        let spans = highlight("markdown", markdown);
        assert_eq!(
            highlight::covers(markdown, &spans, "#"),
            Some(TokenKind::Keyword)
        );
        assert_eq!(
            highlight::covers(markdown, &spans, "code"),
            Some(TokenKind::String)
        );
        let shell = "set -eu\n# note\n";
        let spans = highlight("shell", shell);
        assert_eq!(
            highlight::covers(shell, &spans, "set"),
            Some(TokenKind::Keyword)
        );
        assert_eq!(
            highlight::covers(shell, &spans, "note"),
            Some(TokenKind::Comment)
        );
        let toml = "name = \"aho\"\n";
        let spans = highlight("toml", toml);
        assert_eq!(
            highlight::covers(toml, &spans, "name"),
            Some(TokenKind::Function)
        );
        assert_eq!(
            highlight::covers(toml, &spans, "aho"),
            Some(TokenKind::String)
        );
        let json = "{ \"name\": true }\n";
        let spans = highlight("json", json);
        assert_eq!(
            highlight::covers(json, &spans, "name"),
            Some(TokenKind::Function)
        );
        assert_eq!(
            highlight::covers(json, &spans, "true"),
            Some(TokenKind::Keyword)
        );
        let profiles = profiles();
        assert_eq!(
            detect(&profiles, Some(Path::new("PROOF.bend")), bend),
            "bend"
        );
        assert_eq!(
            detect(&profiles, Some(Path::new("README.md")), markdown),
            "markdown"
        );
        assert_eq!(
            detect(&profiles, Some(Path::new("bootstrap.sh")), shell),
            "shell"
        );
        assert_eq!(
            detect(&profiles, Some(Path::new("Cargo.toml")), toml),
            "toml"
        );
        assert_eq!(
            detect(&profiles, Some(Path::new("Cargo.lock")), toml),
            "toml"
        );
    }

    #[test]
    fn huge_buffers_stay_plain() {
        let text = "fn ".repeat(200_000);
        let spans = highlight("rust", &text);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, TokenKind::Plain);
    }

    #[test]
    fn broken_source_does_not_panic() {
        let spans = highlight("rust", "fn ((((");
        assert!(spans.iter().any(|span| span.kind == TokenKind::Keyword));
    }

    #[test]
    fn formatters_match_shipped_profiles_and_do_not_run_themselves() {
        for profile in profiles() {
            let declared = profile.formatter.expect(&profile.id);
            let tool = formatter_for(&profile.id).unwrap_or_else(|| panic!("{}", profile.id));
            assert!(
                declared.split_whitespace().next() == Some(tool.program),
                "{} declares {declared}",
                profile.id
            );
        }
        match format_manually("plain", "fn main(){let x=1;}") {
            FormatOutcome::Unavailable { program } => assert_eq!(program, "none"),
            other => panic!("plain text was formatted: {other:?}"),
        }
    }

    #[test]
    fn manual_formatter_rewrites_only_a_copy() {
        let source = "original\n";
        match format_with(
            "python3",
            &[
                "-c",
                "import pathlib,sys; pathlib.Path(sys.argv[-1]).write_text('FORMATTED\\n')",
            ],
            "py",
            source,
            std::time::Duration::from_secs(5),
        ) {
            FormatOutcome::Applied(text) => assert_eq!(text, "FORMATTED\n"),
            other => panic!("{other:?}"),
        }
        assert_eq!(source, "original\n");
        match format_with(
            "axiom-missing-formatter",
            &[],
            "rs",
            source,
            std::time::Duration::from_secs(2),
        ) {
            FormatOutcome::Unavailable { program } => {
                assert_eq!(program, "axiom-missing-formatter")
            }
            other => panic!("{other:?}"),
        }
        let started = std::time::Instant::now();
        match format_with(
            "python3",
            &["-c", "import time; time.sleep(30)"],
            "py",
            source,
            std::time::Duration::from_millis(200),
        ) {
            FormatOutcome::Failed { detail, .. } => assert!(detail.contains("timed out")),
            other => panic!("{other:?}"),
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
