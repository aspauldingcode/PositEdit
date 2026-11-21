//! Highlighting for languages that have no tree-sitter grammar in this build.
//! Spans use byte offsets, same as the grammar highlighter.
use crate::{Span, TokenKind};

pub fn highlight(language: &str, text: &str) -> Option<Vec<Span>> {
    match language {
        "shell" => Some(shell(text)),
        "markdown" => Some(markdown(text)),
        "toml" => Some(toml(text)),
        "json" => Some(json(text)),
        "bend" => Some(bend(text)),
        _ => None,
    }
}

pub fn language_for_path(path: &std::path::Path) -> Option<&'static str> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if name == "Cargo.lock" {
        return Some("toml");
    }
    if name == "flake.lock" || name == "package-lock.json" {
        return Some("json");
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("sh" | "bash" | "zsh") => Some("shell"),
        Some("md" | "markdown") => Some("markdown"),
        Some("toml") => Some("toml"),
        Some("json") => Some("json"),
        Some("bend") => Some("bend"),
        _ => None,
    }
}

fn shell(text: &str) -> Vec<Span> {
    const KEYWORDS: &[&str] = &[
        "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in",
        "function", "select", "until", "return", "exit", "set", "export", "local", "readonly",
        "source", "alias", "time",
    ];
    scan_code(text, '#', KEYWORDS, true)
}

fn bend(text: &str) -> Vec<Span> {
    const KEYWORDS: &[&str] = &[
        "import", "def", "match", "case", "as", "let", "if", "then", "else", "true", "false",
    ];
    let mut spans = scan_code(text, '#', KEYWORDS, false);
    mark_after(&mut spans, text, "def", TokenKind::Function);
    spans
}

fn toml(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'#' => {
                let end = line_end(text, i);
                push(&mut spans, i, end, TokenKind::Comment);
                i = end;
            }
            b'"' | b'\'' => i = string_span(text, i, &mut spans),
            b'[' => {
                let end = text[i..]
                    .find(']')
                    .map(|at| i + at + 1)
                    .unwrap_or(line_end(text, i));
                push(&mut spans, i, end, TokenKind::Keyword);
                i = end;
            }
            b'=' => i += 1,
            ch if ch.is_ascii_alphabetic() || ch == b'_' => {
                let end = word_end(text, i);
                let word = &text[i..end];
                if matches!(word, "true" | "false") {
                    push(&mut spans, i, end, TokenKind::Keyword);
                } else if next_non_space(text, end) == Some(b'=') {
                    push(&mut spans, i, end, TokenKind::Function);
                }
                i = end;
            }
            ch if ch.is_ascii_digit() => {
                let end = number_end(text, i);
                push(&mut spans, i, end, TokenKind::Type);
                i = end;
            }
            _ => i = bump(text, i),
        }
    }
    spans
}

fn json(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i = string_span(text, i, &mut spans);
                if next_non_space(text, i) == Some(b':') {
                    if let Some(span) = spans.last_mut() {
                        if span.start == start {
                            span.kind = TokenKind::Function;
                        }
                    }
                }
            }
            ch if ch.is_ascii_alphabetic() => {
                let end = word_end(text, i);
                if matches!(&text[i..end], "true" | "false" | "null") {
                    push(&mut spans, i, end, TokenKind::Keyword);
                }
                i = end;
            }
            ch if ch.is_ascii_digit()
                || (ch == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) =>
            {
                let end = number_end(text, if ch == b'-' { i + 1 } else { i });
                push(&mut spans, i, end.max(i + 1), TokenKind::Type);
                i = end.max(i + 1);
            }
            _ => i = bump(text, i),
        }
    }
    spans
}

fn markdown(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut fenced = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let start = offset;
        if body.starts_with("```") || body.starts_with("~~~") {
            push(&mut spans, start, start + body.len(), TokenKind::Keyword);
            fenced = !fenced;
        } else if fenced {
            push(&mut spans, start, start + body.len(), TokenKind::String);
        } else if let Some(rest) = body.strip_prefix('#') {
            let marks = 1 + rest.chars().take_while(|ch| *ch == '#').count();
            push(
                &mut spans,
                start,
                start + marks.min(body.len()),
                TokenKind::Keyword,
            );
            if marks < body.len() {
                push(
                    &mut spans,
                    start + marks,
                    start + body.len(),
                    TokenKind::Function,
                );
            }
        } else if body.starts_with('>') {
            push(&mut spans, start, start + body.len(), TokenKind::Comment);
        } else {
            inline_markdown(&mut spans, body, start);
        }
        offset += line.len();
    }
    spans
}

fn inline_markdown(spans: &mut Vec<Span>, line: &str, base: usize) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(end) = line[i + 1..].find('`') {
                push(spans, base + i, base + i + end + 2, TokenKind::String);
                i += end + 2;
                continue;
            }
        }
        if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = line[i + 2..].find("**") {
                push(spans, base + i, base + i + end + 4, TokenKind::Function);
                i += end + 4;
                continue;
            }
        }
        i = bump(line, i);
    }
}

fn scan_code(text: &str, comment: char, keywords: &[&str], dollars: bool) -> Vec<Span> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == comment as u8 {
            let end = line_end(text, i);
            push(&mut spans, i, end, TokenKind::Comment);
            i = end;
            continue;
        }
        if ch == b'"' || ch == b'\'' {
            i = string_span(text, i, &mut spans);
            continue;
        }
        if dollars && ch == b'$' {
            let end = if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                text[i + 2..]
                    .find('}')
                    .map(|at| i + at + 3)
                    .unwrap_or(word_end(text, i + 1))
            } else {
                word_end(text, i + 1)
            };
            if end > i + 1 {
                push(&mut spans, i, end, TokenKind::Type);
                i = end;
                continue;
            }
        }
        if ch.is_ascii_alphabetic() || ch == b'_' {
            let end = word_end(text, i);
            if keywords.contains(&&text[i..end]) {
                push(&mut spans, i, end, TokenKind::Keyword);
            } else if text[i..end]
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_uppercase())
            {
                push(&mut spans, i, end, TokenKind::Type);
            }
            i = end;
            continue;
        }
        i = bump(text, i);
    }
    spans
}

fn mark_after(spans: &mut Vec<Span>, text: &str, keyword: &str, kind: TokenKind) {
    let mut search = 0;
    while let Some(at) = text[search..].find(keyword) {
        let start = search + at;
        let end = start + keyword.len();
        let boundary = start == 0
            || text.as_bytes()[start - 1].is_ascii_whitespace()
            || !text.as_bytes()[start - 1].is_ascii_alphanumeric();
        search = end;
        if !boundary {
            continue;
        }
        let Some(word) = text[end..].find(|ch: char| ch.is_ascii_alphabetic() || ch == '_') else {
            continue;
        };
        let name = end + word;
        if name > end && text[end..name].chars().all(char::is_whitespace) {
            let stop = word_end(text, name);
            push(spans, name, stop, kind);
        }
    }
}

fn string_span(text: &str, start: usize, spans: &mut Vec<Span>) -> usize {
    let quote = text.as_bytes()[start];
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == quote || bytes[i] == b'\n' {
            let end = if bytes[i] == quote { i + 1 } else { i };
            push(spans, start, end, TokenKind::String);
            return end;
        }
        i += 1;
    }
    push(spans, start, bytes.len(), TokenKind::String);
    bytes.len()
}

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: TokenKind) {
    if start < end {
        spans.push(Span { start, end, kind });
    }
}

fn line_end(text: &str, start: usize) -> usize {
    text[start..]
        .find('\n')
        .map(|at| start + at)
        .unwrap_or(text.len())
}

fn word_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
    {
        i += 1;
    }
    i
}

fn number_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn next_non_space(text: &str, start: usize) -> Option<u8> {
    text[start..]
        .bytes()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn bump(text: &str, index: usize) -> usize {
    match text[index..].chars().next() {
        Some(ch) => index + ch.len_utf8(),
        None => text.len(),
    }
}
