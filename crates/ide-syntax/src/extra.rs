//! Highlighting for languages that have no tree-sitter grammar in this build.
//! Spans use byte offsets, same as the grammar highlighter.
use crate::{Span, TokenKind};

pub fn highlight(language: &str, text: &str) -> Option<Vec<Span>> {
    match language {
        "shell" => Some(shell(text)),
        "markdown" => Some(markdown(text)),
        "toml" => Some(toml(text)),
        "ini" => Some(ini(text)),
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
        Some("ini" | "cfg" | "conf") => Some("ini"),
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

fn ini(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b';' | b'#' => {
                let end = line_end(text, i);
                push(&mut spans, i, end, TokenKind::Comment);
                i = end;
            }
            b'[' => {
                let end = text[i..]
                    .find(']')
                    .map(|at| i + at + 1)
                    .unwrap_or(line_end(text, i));
                push(&mut spans, i, end, TokenKind::Keyword);
                i = end;
            }
            b'"' | b'\'' => i = string_span(text, i, &mut spans),
            ch if ch.is_ascii_alphabetic() || ch == b'_' => {
                let end = word_end(text, i);
                if next_non_space(text, end) == Some(b'=') {
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
        let trimmed = body.trim_start();
        let indent = body.len() - trimmed.len();
        if body.starts_with("```") || body.starts_with("~~~") {
            push(&mut spans, start, start + body.len(), TokenKind::Markup);
            fenced = !fenced;
        } else if fenced {
            push(&mut spans, start, start + body.len(), TokenKind::String);
        } else if trimmed.starts_with('#') {
            let marks = trimmed.chars().take_while(|ch| *ch == '#').count();
            let mark_end = start + indent + marks;
            push(&mut spans, start + indent, mark_end.min(start + body.len()), TokenKind::Markup);
            if mark_end < start + body.len() {
                // Prefer heading style on the title text after `#`.
                let title_start = mark_end;
                // Still honor inline bold/italic inside headings.
                inline_markdown(&mut spans, &body[indent + marks..], title_start, true);
            }
        } else if trimmed.starts_with('>') {
            push(&mut spans, start, start + body.len(), TokenKind::Comment);
        } else if is_table_row(trimmed) {
            table_row(&mut spans, body, start);
        } else if trimmed
            .chars()
            .all(|ch| matches!(ch, '-' | '*' | '_' | ' '))
            && trimmed.chars().filter(|ch| matches!(ch, '-' | '*' | '_')).count() >= 3
        {
            push(&mut spans, start, start + body.len(), TokenKind::Markup);
        } else if let Some(rest) = list_marker(trimmed) {
            let marker_len = trimmed.len() - rest.len();
            push(
                &mut spans,
                start + indent,
                start + indent + marker_len,
                TokenKind::Keyword,
            );
            inline_markdown(
                &mut spans,
                rest,
                start + indent + marker_len,
                false,
            );
        } else {
            inline_markdown(&mut spans, body, start, false);
        }
        offset += line.len();
    }
    spans
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.matches('|').count() >= 2
}

fn list_marker(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if matches!(bytes[0], b'-' | b'*' | b'+') && bytes.get(1) == Some(&b' ') {
        return Some(&line[2..]);
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && bytes.get(i) == Some(&b'.') && bytes.get(i + 1) == Some(&b' ') {
        return Some(&line[i + 2..]);
    }
    None
}

fn table_row(spans: &mut Vec<Span>, line: &str, base: usize) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'|' {
            push(spans, base + i, base + i + 1, TokenKind::Markup);
            i += 1;
            continue;
        }
        if bytes[i] == b'-' || bytes[i] == b':' {
            let start = i;
            while i < bytes.len() && matches!(bytes[i], b'-' | b':' | b' ') {
                i += 1;
            }
            push(spans, base + start, base + i, TokenKind::Markup);
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'|' {
            i += 1;
        }
        if i > start {
            push(spans, base + start, base + i, TokenKind::Table);
        }
    }
}

fn inline_markdown(spans: &mut Vec<Span>, line: &str, base: usize, heading: bool) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(end) = line[i + 1..].find('`') {
                push(spans, base + i, base + i + 1, TokenKind::Markup);
                push(
                    spans,
                    base + i + 1,
                    base + i + 1 + end,
                    TokenKind::String,
                );
                push(
                    spans,
                    base + i + 1 + end,
                    base + i + end + 2,
                    TokenKind::Markup,
                );
                i += end + 2;
                continue;
            }
        }
        if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = line[i + 2..].find("**") {
                push(spans, base + i, base + i + 2, TokenKind::Markup);
                push(
                    spans,
                    base + i + 2,
                    base + i + 2 + end,
                    if heading {
                        TokenKind::Heading
                    } else {
                        TokenKind::Bold
                    },
                );
                push(
                    spans,
                    base + i + 2 + end,
                    base + i + end + 4,
                    TokenKind::Markup,
                );
                i += end + 4;
                continue;
            }
        }
        if bytes[i] == b'*' || bytes[i] == b'_' {
            let delim = bytes[i];
            if let Some(rel) = line[i + 1..].find(delim as char) {
                let end = i + 1 + rel;
                // Avoid treating `*` list leftovers or empty emphasis.
                if end > i + 1 && !line[i + 1..end].contains('\n') {
                    push(spans, base + i, base + i + 1, TokenKind::Markup);
                    push(spans, base + i + 1, base + end, TokenKind::Italic);
                    push(spans, base + end, base + end + 1, TokenKind::Markup);
                    i = end + 1;
                    continue;
                }
            }
        }
        if heading {
            let start = i;
            while i < bytes.len()
                && bytes[i] != b'*'
                && bytes[i] != b'_'
                && bytes[i] != b'`'
            {
                i += 1;
            }
            if i > start {
                push(spans, base + start, base + i, TokenKind::Heading);
            }
            continue;
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
