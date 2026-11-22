//! In-process lexical Tab suggestions. No network and no model weights.
//! The Python worker under `extensions/local-tab-dlc3` is the protocol reference.
//! A missing local FIM model stays on this path and is reported as Basic, not Ready.

use std::collections::HashMap;

/// Identifier completion matching the DLC lexical fallback: the most frequent
/// longer word that shares the trailing prefix, then first-seen order.
pub fn suggest(prefix: &str, suffix: &str, context: &[String]) -> String {
    let fragment = trailing_word(prefix);
    if fragment.len() < 2 {
        return String::new();
    }
    let head = &prefix[..prefix.len() - fragment.len()];
    let window = tail_chars(head, 8000);
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut order: HashMap<String, usize> = HashMap::new();
    for text in std::iter::once(window.as_str()).chain(context.iter().map(String::as_str)) {
        for word in words(text) {
            if word != fragment
                && word.starts_with(fragment.as_str())
                && word.len() > fragment.len()
            {
                *counts.entry(word.clone()).or_insert(0) += 1;
                let next = order.len();
                order.entry(word).or_insert(next);
            }
        }
    }
    let mut ranked: Vec<String> = counts.keys().cloned().collect();
    ranked.sort_by(|a, b| {
        counts[b]
            .cmp(&counts[a])
            .then_with(|| order[a].cmp(&order[b]))
            .then_with(|| a.len().cmp(&b.len()))
    });
    let Some(best) = ranked.first() else {
        return String::new();
    };
    let candidate = best[fragment.len()..].to_string();
    sanitize(&candidate, suffix)
}

pub fn secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains(".env")
        || lower.contains("credential")
        || lower.contains("secret")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
}

fn sanitize(candidate: &str, suffix: &str) -> String {
    if candidate.starts_with("```") || candidate.contains('\0') {
        return String::new();
    }
    let mut out = String::new();
    for ch in candidate.chars().take(2048) {
        if ch == '\n' || ch == '\t' || (ch >= ' ' && ch != '\u{7f}') {
            out.push(ch);
        }
    }
    if out.trim().is_empty() {
        return String::new();
    }
    let take = suffix.chars().take(32).collect::<String>();
    if !take.is_empty() && out.starts_with(&take) {
        return String::new();
    }
    out
}

fn trailing_word(prefix: &str) -> String {
    let mut word = String::new();
    for ch in prefix.chars().rev() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
        } else {
            break;
        }
    }
    word.chars().rev().collect()
}

fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    text.chars().skip(count - max).collect()
}

fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_nearest_identifier() {
        let prefix = "let width = 1;\nlet w = wid";
        assert_eq!(suggest(prefix, "", &[]), "th");
    }

    #[test]
    fn skips_short_fragments_and_secrets() {
        assert!(suggest("let x", "", &[]).is_empty());
        assert!(secret_name(".env"));
        assert!(secret_name("id_rsa.key"));
        assert!(!secret_name("main.rs"));
    }
}
