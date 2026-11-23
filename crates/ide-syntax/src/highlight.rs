//! Tree-sitter highlighting. Token kinds come from the parse tree.
//! A keyword inside a string or comment stays part of that node.
//!
//! Nix: a block comment that is only a language id (`/* bash */`, `/* py */`)
//! immediately before a string re-highlights that string's content as the
//! guest language. Shell completion / `${…}` interpolations stay Nix.
use tree_sitter::{Language, Node, Parser};

use crate::{Span, TokenKind};

const HIGHLIGHT_LIMIT: usize = 256 * 1024;

pub fn grammar_ready() -> bool {
    true
}

pub fn highlight_source(language: &str, text: &str) -> Vec<Span> {
    if language == "plain" || language.is_empty() || text.len() > HIGHLIGHT_LIMIT {
        return vec![Span {
            start: 0,
            end: text.len(),
            kind: TokenKind::Plain,
        }];
    }
    // Prefer tree-sitter (static crates or nixpkgs allGrammars) over hand scanners.
    if let Some(grammar) = grammar(language) {
        return highlight_tree(language, text, grammar);
    }
    if let Some(spans) = crate::extra::highlight(language, text) {
        return spans;
    }
    vec![Span {
        start: 0,
        end: text.len(),
        kind: TokenKind::Plain,
    }]
}

fn highlight_tree(language: &str, text: &str, grammar: Language) -> Vec<Span> {
    let mut parser = Parser::new();
    if parser.set_language(&grammar).is_err() {
        return vec![Span {
            start: 0,
            end: text.len(),
            kind: TokenKind::Plain,
        }];
    }
    let Some(tree) = parser.parse(text, None) else {
        return vec![Span {
            start: 0,
            end: text.len(),
            kind: TokenKind::Plain,
        }];
    };
    let mut spans = Vec::new();
    if language == "nix" {
        collect_nix(tree.root_node(), text, &mut spans);
    } else {
        collect(tree.root_node(), &mut spans);
    }
    spans.retain(|span| span.start < span.end && span.end <= text.len());
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRange {
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: String,
}

/// Smallest named node covering `byte_offset`, for lossless structural edits.
pub fn node_at_byte(language: &str, text: &str, byte_offset: usize) -> Option<NodeRange> {
    if language == "plain" || language.is_empty() || byte_offset > text.len() {
        return None;
    }
    let grammar = grammar(language)?;
    let mut parser = Parser::new();
    parser.set_language(&grammar).ok()?;
    let tree = parser.parse(text, None)?;
    let mut node = tree.root_node();
    let mut best = None;
    loop {
        if node.is_named()
            && node.start_byte() <= byte_offset
            && byte_offset < node.end_byte().max(node.start_byte().saturating_add(1))
        {
            best = Some(NodeRange {
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                kind: node.kind().to_string(),
            });
        }
        let mut next = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.start_byte() <= byte_offset
                && byte_offset < child.end_byte().max(child.start_byte().saturating_add(1))
            {
                next = Some(child);
                break;
            }
        }
        match next {
            Some(child) => node = child,
            None => break,
        }
    }
    best
}

fn grammar(language: &str) -> Option<Language> {
    let bundled = match language {
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "c" => Some(tree_sitter_c::LANGUAGE.into()),
        // tree-sitter-objc covers Objective-C and typical .mm ObjC++ mixtures.
        "objc" | "objective-c" | "objective-cpp" | "objcpp" => {
            Some(tree_sitter_objc::LANGUAGE.into())
        }
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "kotlin" => Some(tree_sitter_kotlin_ng::LANGUAGE.into()),
        "swift" => Some(tree_sitter_swift::LANGUAGE.into()),
        "zig" => Some(tree_sitter_zig::LANGUAGE.into()),
        "nix" => Some(tree_sitter_nix::LANGUAGE.into()),
        _ => None,
    };
    bundled.or_else(|| crate::dynamic::load(language))
}

fn collect(node: Node<'_>, spans: &mut Vec<Span>) {
    if let Some(kind) = classify(node) {
        spans.push(Span {
            start: node.start_byte(),
            end: node.end_byte(),
            kind,
        });
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, spans);
    }
}

/// Nix walk that pairs `/* lang */` comments with the following string.
fn collect_nix(node: Node<'_>, text: &str, spans: &mut Vec<Span>) {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    if children.is_empty() {
        if let Some(kind) = classify(node) {
            spans.push(Span {
                start: node.start_byte(),
                end: node.end_byte(),
                kind,
            });
        }
        return;
    }

    let mut i = 0;
    while i < children.len() {
        let child = children[i];
        if child.kind() == "comment" {
            let start = child.start_byte();
            let end = child.end_byte();
            spans.push(Span {
                start,
                end,
                kind: TokenKind::Comment,
            });
            let hint = language_hint_from_comment(&text[start..end]);
            if let Some(lang) = hint {
                if i + 1 < children.len()
                    && matches!(
                        children[i + 1].kind(),
                        "string_expression" | "indented_string_expression"
                    )
                {
                    paint_embedded_string(children[i + 1], text, lang, spans);
                    i += 2;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        if matches!(
            child.kind(),
            "string_expression" | "indented_string_expression"
        ) {
            spans.push(Span {
                start: child.start_byte(),
                end: child.end_byte(),
                kind: TokenKind::String,
            });
            i += 1;
            continue;
        }

        if let Some(kind) = classify(child) {
            spans.push(Span {
                start: child.start_byte(),
                end: child.end_byte(),
                kind,
            });
        } else if child.child_count() > 0 {
            collect_nix(child, text, spans);
        }
        i += 1;
    }
}

fn paint_embedded_string(node: Node<'_>, text: &str, lang: &str, spans: &mut Vec<Span>) {
    let base = node.start_byte();
    let end = node.end_byte();
    if end < base || end > text.len() {
        return;
    }

    // Copy the string bytes, then blank out delimiters and `${…}` holes so the
    // guest highlighter only sees embed content at the correct offsets.
    let mut guest = text[base..end].as_bytes().to_vec();
    let mut holes: Vec<(usize, usize)> = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let child_start = child.start_byte();
        let child_end = child.end_byte();
        let rel = child_start.saturating_sub(base)..child_end.saturating_sub(base);
        if rel.end > guest.len() {
            continue;
        }
        if child.kind() == "interpolation" {
            for byte in &mut guest[rel.clone()] {
                *byte = b' ';
            }
            holes.push((child_start, child_end));
            collect_nix_interpolation(child, text, spans);
            continue;
        }
        if !child.is_named() {
            // Opening/closing `''` or `"` stay string-colored.
            for byte in &mut guest[rel.clone()] {
                *byte = b' ';
            }
            holes.push((child_start, child_end));
            spans.push(Span {
                start: child_start,
                end: child_end,
                kind: TokenKind::String,
            });
            continue;
        }
        if child.kind() == "escape_sequence" || child.kind() == "dollar_escape" {
            for byte in &mut guest[rel] {
                *byte = b' ';
            }
            holes.push((child_start, child_end));
            spans.push(Span {
                start: child_start,
                end: child_end,
                kind: TokenKind::String,
            });
        }
    }

    let Ok(guest_text) = String::from_utf8(guest) else {
        spans.push(Span {
            start: base,
            end,
            kind: TokenKind::String,
        });
        return;
    };

    let mut embedded = highlight_source(lang, &guest_text);
    for span in &mut embedded {
        span.start += base;
        span.end += base;
    }
    // Keep guest spans out of delimiter/interpolation holes so paint stays ordered.
    embedded.retain(|span| {
        if span.end > text.len() || span.start >= span.end {
            return false;
        }
        if holes
            .iter()
            .any(|&(hole_start, hole_end)| span.start < hole_end && span.end > hole_start)
        {
            return false;
        }
        text[span.start..span.end]
            .bytes()
            .any(|byte| !byte.is_ascii_whitespace())
    });
    spans.extend(embedded);
}

fn collect_nix_interpolation(node: Node<'_>, text: &str, spans: &mut Vec<Span>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            spans.push(Span {
                start: child.start_byte(),
                end: child.end_byte(),
                kind: TokenKind::Keyword,
            });
        } else {
            collect_nix(child, text, spans);
        }
    }
}

/// `/* bash */` / `# python` → guest language id, only when the comment is a hint.
fn language_hint_from_comment(comment: &str) -> Option<&'static str> {
    let trimmed = comment.trim();
    let inner = if let Some(rest) = trimmed.strip_prefix("/*") {
        rest.strip_suffix("*/")?.trim()
    } else if let Some(rest) = trimmed.strip_prefix('#') {
        rest.trim()
    } else {
        return None;
    };
    if inner.is_empty() {
        return None;
    }
    // Hints are a single language token — long prose comments stay comments.
    if inner.split_whitespace().count() != 1 {
        return None;
    }
    let token = inner.split_whitespace().next()?.trim();
    if !token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '#' | '_' | '-' | '.'))
    {
        return None;
    }
    embed_language(token)
}

fn embed_language(hint: &str) -> Option<&'static str> {
    Some(match hint.to_ascii_lowercase().as_str() {
        "bash" | "sh" | "zsh" | "shell" => "shell",
        "py" | "python" => "python",
        "js" | "javascript" => "javascript",
        "ts" | "typescript" => "typescript",
        "rs" | "rust" => "rust",
        "java" => "java",
        "kt" | "kotlin" => "kotlin",
        "swift" => "swift",
        "c" => "c",
        "objc" | "objective-c" | "objectivec" => "objc",
        "zig" => "zig",
        "nix" => "nix",
        "toml" => "toml",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "bend" => "bend",
        "ini" | "conf" | "cfg" => "ini",
        _ => return None,
    })
}

fn classify(node: Node<'_>) -> Option<TokenKind> {
    let kind = node.kind();
    if !node.is_named() {
        // ObjC @keywords (@interface, @property, @end, …) arrive as anonymous '@…' tokens.
        if kind.starts_with('@') {
            return Some(TokenKind::Keyword);
        }
        return kind
            .chars()
            .next()
            .filter(|ch| ch.is_ascii_alphabetic())
            .map(|_| TokenKind::Keyword);
    }
    let lower = kind.to_ascii_lowercase();
    if lower.contains("comment") {
        return Some(TokenKind::Comment);
    }
    if lower.contains("string")
        || lower.contains("char_literal")
        || lower.contains("string_literal")
        || kind == "string_content"
    {
        return Some(TokenKind::String);
    }
    if lower.contains("integer") || lower.contains("float") || lower == "number" {
        return Some(TokenKind::Type);
    }
    if matches!(
        kind,
        "type_identifier"
            | "primitive_type"
            | "builtin_type"
            | "predefined_type"
            | "class_name"
            | "protocol_name"
            | "sized_type_specifier"
    ) {
        return Some(TokenKind::Type);
    }
    if kind == "identifier"
        || kind == "property_identifier"
        || kind == "field_identifier"
        || kind == "keyword_identifier"
    {
        if let Some(parent) = node.parent() {
            let parent_kind = parent.kind();
            let name = parent.child_by_field_name("name");
            if name.is_some_and(|child| child.id() == node.id())
                && (parent_kind.contains("function")
                    || parent_kind.contains("method")
                    || parent_kind == "function_declarator"
                    || parent_kind == "message_expression")
            {
                return Some(TokenKind::Function);
            }
            if parent_kind.contains("selector") || parent_kind == "keyword_selector" {
                return Some(TokenKind::Function);
            }
        }
    }
    None
}

#[cfg(test)]
pub fn covers(text: &str, spans: &[Span], needle: &str) -> Option<TokenKind> {
    let start = text.find(needle)?;
    let end = start + needle.len();
    spans
        .iter()
        .find(|span| span.start <= start && span.end >= end)
        .map(|span| span.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nix_embed_bash_highlights_shell_keywords() {
        let source = r#"
pkgs.runCommand "n" {} /* bash */ ''
  export HOME=$TMPDIR
  local name="x"
  echo hi
'';
"#;
        let spans = highlight_source("nix", source);
        assert_eq!(covers(source, &spans, "/* bash */"), Some(TokenKind::Comment));
        assert_eq!(covers(source, &spans, "export"), Some(TokenKind::Keyword));
        assert_eq!(covers(source, &spans, "local"), Some(TokenKind::Keyword));
        // Delimiters stay string-colored, not flattened into one plain string span.
        assert_eq!(covers(source, &spans, "''"), Some(TokenKind::String));
    }

    #[test]
    fn nix_embed_python_and_javascript_hints() {
        let source = r#"
{
  a = /* python */ ''
    def foo():
      return 1
  '';
  b = /* javascript */ "const x = 1;";
}
"#;
        let spans = highlight_source("nix", source);
        assert_eq!(covers(source, &spans, "def"), Some(TokenKind::Keyword));
        assert_eq!(covers(source, &spans, "const"), Some(TokenKind::Keyword));
    }

    #[test]
    fn nix_embed_keeps_interpolation_as_nix() {
        let source = r#"
{
  c = /* bash */ ''
    echo ${toString 1}
  '';
}
"#;
        let spans = highlight_source("nix", source);
        assert_eq!(covers(source, &spans, "${"), Some(TokenKind::Keyword));
        assert_eq!(covers(source, &spans, "1"), Some(TokenKind::Type));
        // Identifiers stay unspanned (plain) in Nix — must not be swallowed by the guest string.
        assert_ne!(
            covers(source, &spans, "toString"),
            Some(TokenKind::String),
            "interpolation body must not be painted as the outer string: {spans:?}"
        );
    }

    #[test]
    fn nix_normal_comment_does_not_rehighlight_string() {
        let source = r#"
{
  # not a language hint because of spaces in block form
  x = /* please keep this as a string */ ''
    export should_stay_string
  '';
}
"#;
        let spans = highlight_source("nix", source);
        assert_eq!(
            covers(source, &spans, "export should_stay_string"),
            Some(TokenKind::String)
        );
    }

    #[test]
    fn language_hint_aliases() {
        assert_eq!(embed_language("py"), Some("python"));
        assert_eq!(embed_language("JS"), Some("javascript"));
        assert_eq!(embed_language("bash"), Some("shell"));
        assert_eq!(embed_language("ini"), Some("ini"));
        assert_eq!(embed_language("nope"), None);
    }

    #[test]
    fn allgrammars_farm_highlights_shell_when_present() {
        let farm = std::path::PathBuf::from("/tmp/ts-all");
        if !farm.join("bash.so").exists() {
            return;
        }
        crate::dynamic::with_grammar_dirs(&[farm], || {
            let source = "export HOME=1\nlocal x=2\n";
            let spans = highlight_source("shell", source);
            assert_eq!(covers(source, &spans, "export"), Some(TokenKind::Keyword));
            assert_eq!(covers(source, &spans, "local"), Some(TokenKind::Keyword));
        });
    }
}
