//! Tree-sitter highlighting. Token kinds come from the parse tree.
//! A keyword inside a string or comment stays part of that node.
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
    if let Some(spans) = crate::extra::highlight(language, text) {
        return spans;
    }
    let Some(grammar) = grammar(language) else {
        return vec![Span {
            start: 0,
            end: text.len(),
            kind: TokenKind::Plain,
        }];
    };
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
    collect(tree.root_node(), &mut spans);
    spans.retain(|span| span.start < span.end && span.end <= text.len());
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

fn grammar(language: &str) -> Option<Language> {
    Some(match language {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "kotlin" => tree_sitter_kotlin_ng::LANGUAGE.into(),
        "swift" => tree_sitter_swift::LANGUAGE.into(),
        "zig" => tree_sitter_zig::LANGUAGE.into(),
        "nix" => tree_sitter_nix::LANGUAGE.into(),
        _ => return None,
    })
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

fn classify(node: Node<'_>) -> Option<TokenKind> {
    let kind = node.kind();
    if !node.is_named() {
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
    if lower.contains("string") || lower.contains("char_literal") {
        return Some(TokenKind::String);
    }
    if lower.contains("integer") || lower.contains("float") || lower == "number" {
        return Some(TokenKind::Type);
    }
    if matches!(
        kind,
        "type_identifier" | "primitive_type" | "builtin_type" | "predefined_type"
    ) {
        return Some(TokenKind::Type);
    }
    if kind == "identifier" || kind == "property_identifier" || kind == "field_identifier" {
        if let Some(parent) = node.parent() {
            let parent_kind = parent.kind();
            let name = parent.child_by_field_name("name");
            if name.is_some_and(|child| child.id() == node.id())
                && (parent_kind.contains("function")
                    || parent_kind.contains("method")
                    || parent_kind == "function_declarator")
            {
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
