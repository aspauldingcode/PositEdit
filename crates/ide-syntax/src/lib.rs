//! Language detection, tree-sitter highlighting, and manual formatting.
//! Formatters run only when `format_manually` is called.
//!
//! Optional nixpkgs `tree-sitter-grammars.withPlugins (_: allGrammars)` farms
//! are loaded from `POSIT_TREE_SITTER_GRAMMARS` (see `dynamic`).
mod dynamic;
mod extra;
mod fold;
mod format;
mod highlight;
mod snippets;

pub use fold::{fold_ranges, FoldRange};
pub use format::{format_manually, format_with, formatter_for, FormatOutcome, Formatter};
pub use highlight::{node_at_byte, NodeRange};
pub use snippets::{expand as expand_snippet, match_trigger, snippets_for, Snippet};
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
    /// Markdown / rich-text headings (`# Title`).
    Heading,
    /// Markdown strong emphasis (`**bold**`).
    Bold,
    /// Markdown emphasis (`*italic*` / `_italic_`).
    Italic,
    /// Markup delimiters (`**`, `#`, `|`, …) — dimmed in the buffer.
    Markup,
    /// Markdown table row cells.
    Table,
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
            // .h is shared by C and ObjC; prefer ObjC when the buffer looks like it.
            if ext == "h"
                && looks_like_objc(text)
                && profiles.iter().any(|profile| profile.id == "objc")
            {
                return "objc".to_string();
            }
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
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            if let Some(language) = dynamic::language_for_extension(&ext.to_ascii_lowercase()) {
                return language.to_string();
            }
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            let lower = name.to_ascii_lowercase();
            if lower == "dockerfile" {
                return "dockerfile".to_string();
            }
            if lower == "makefile" || lower == "gnumakefile" {
                return "make".to_string();
            }
        }
    }
    if let Some(language) = shebang_language(text) {
        return language;
    }
    if text.contains("fn main") && text.contains("let ") {
        return "rust".to_string();
    }
    if looks_like_objc(text) {
        return "objc".to_string();
    }
    "plain".to_string()
}

fn looks_like_objc(text: &str) -> bool {
    text.contains("@interface")
        || text.contains("@implementation")
        || text.contains("@protocol")
        || text.contains("#import")
        || text.contains("@property")
        || text.contains("@end")
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
            (
                "objc",
                "@interface Shown : NSObject\n@property (nonatomic) NSString *name;\n- (void)shown;\n@end\n",
            ),
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
    fn objc_m_and_mm_and_objc_headers_are_detected() {
        let profiles = profiles();
        assert!(profiles.iter().any(|profile| profile.id == "objc"));
        assert_eq!(
            detect(&profiles, Some(Path::new("App.m")), "@implementation App\n@end\n"),
            "objc"
        );
        assert_eq!(
            detect(
                &profiles,
                Some(Path::new("App.mm")),
                "#import <Foundation/Foundation.h>\n@interface App : NSObject\n@end\n"
            ),
            "objc"
        );
        assert_eq!(
            detect(
                &profiles,
                Some(Path::new("App.h")),
                "#import <Foundation/Foundation.h>\n@interface App : NSObject\n@end\n"
            ),
            "objc"
        );
        assert_eq!(
            detect(&profiles, Some(Path::new("plain.h")), "int answer(void);\n"),
            "c"
        );
        let source = "@interface App : NSObject\n- (NSString *)name;\n@end\n";
        let spans = highlight("objc", source);
        assert_eq!(
            highlight::covers(source, &spans, "@interface"),
            Some(TokenKind::Keyword),
            "expected @interface keyword, spans={spans:?}"
        );
        assert!(
            spans.iter().any(|span| span.kind == TokenKind::Type),
            "expected a type span in objc sample: {spans:?}"
        );
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
            Some(TokenKind::Markup)
        );
        assert_eq!(
            highlight::covers(markdown, &spans, "Title"),
            Some(TokenKind::Heading)
        );
        assert_eq!(
            highlight::covers(markdown, &spans, "code"),
            Some(TokenKind::String)
        );
        let rich = "# Welcome\n\n**bold** and *italic*\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";
        let rich_spans = highlight("markdown", rich);
        assert_eq!(
            highlight::covers(rich, &rich_spans, "bold"),
            Some(TokenKind::Bold)
        );
        assert_eq!(
            highlight::covers(rich, &rich_spans, "italic"),
            Some(TokenKind::Italic)
        );
        assert!(
            rich_spans
                .iter()
                .any(|span| span.kind == TokenKind::Table),
            "expected table cell spans: {rich_spans:?}"
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
            let Some(declared) = profile.formatter.as_deref() else {
                continue;
            };
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
    fn java_format_falls_back_when_google_java_format_missing() {
        let source = "class A{int x;}\n";
        match format_manually("java", source) {
            FormatOutcome::Applied(text) => {
                assert!(text.contains("class A"), "{text}");
                assert!(text.contains('\n') || text.contains(' '), "{text}");
                assert_ne!(text, source);
            }
            FormatOutcome::Unchanged => {}
            FormatOutcome::Unavailable { program } => {
                // Neither preferred nor clang-format fallback is installed.
                assert!(
                    program.contains("google-java-format"),
                    "{program}"
                );
            }
            FormatOutcome::Failed { detail, .. } => panic!("java format failed: {detail}"),
        }
    }

    #[test]
    fn nix_format_uses_nixfmt_only() {
        let source = "{ a=1; }\n";
        match format_manually("nix", source) {
            FormatOutcome::Applied(text) => {
                assert!(text.contains('a'), "{text}");
                assert_ne!(text, source);
            }
            FormatOutcome::Unchanged => {}
            FormatOutcome::Unavailable { program } => {
                assert!(program.contains("nixfmt"), "{program}");
                assert!(
                    !program.contains("alejandra") && !program.contains("nixpkgs-fmt"),
                    "nix formatter must not fall back to alejandra/nixpkgs-fmt: {program}"
                );
            }
            FormatOutcome::Failed { detail, .. } => panic!("nix format failed: {detail}"),
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
