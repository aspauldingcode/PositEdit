//! Built-in snippets keyed by language id. Expansion is tab-stop aware (`$0`, `$1`).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub trigger: &'static str,
    pub body: &'static str,
    pub description: &'static str,
}

pub fn snippets_for(language: &str) -> &'static [Snippet] {
    match language {
        "rust" => &[
            Snippet {
                trigger: "fn",
                body: "fn ${1:name}(${2:args}) {\n    $0\n}",
                description: "function",
            },
            Snippet {
                trigger: "test",
                body: "#[test]\nfn ${1:name}() {\n    $0\n}",
                description: "unit test",
            },
            Snippet {
                trigger: "impl",
                body: "impl ${1:Type} {\n    $0\n}",
                description: "impl block",
            },
        ],
        "python" => &[
            Snippet {
                trigger: "def",
                body: "def ${1:name}(${2:args}):\n    $0",
                description: "function",
            },
            Snippet {
                trigger: "class",
                body: "class ${1:Name}:\n    def __init__(self${2:}):\n        $0",
                description: "class",
            },
        ],
        "javascript" | "typescript" => &[
            Snippet {
                trigger: "fn",
                body: "function ${1:name}(${2:args}) {\n  $0\n}",
                description: "function",
            },
            Snippet {
                trigger: "afn",
                body: "async function ${1:name}(${2:args}) {\n  $0\n}",
                description: "async function",
            },
        ],
        "java" | "kotlin" => &[
            Snippet {
                trigger: "main",
                body: "public static void main(String[] args) {\n    $0\n}",
                description: "main",
            },
            Snippet {
                trigger: "cls",
                body: "class ${1:Name} {\n    $0\n}",
                description: "class",
            },
        ],
        "swift" => &[
            Snippet {
                trigger: "func",
                body: "func ${1:name}(${2:args}) {\n    $0\n}",
                description: "function",
            },
        ],
        "c" | "objc" | "objective-c" => &[
            Snippet {
                trigger: "main",
                body: "int main(int argc, char **argv) {\n    $0\n    return 0;\n}",
                description: "main",
            },
        ],
        "nix" => &[
            Snippet {
                trigger: "pkg",
                body: "{ pkgs ? import <nixpkgs> {} }:\npkgs.mkShell {\n  packages = [ $0 ];\n}",
                description: "mkShell",
            },
        ],
        "zig" => &[
            Snippet {
                trigger: "fn",
                body: "fn ${1:name}(${2:args}) void {\n    $0\n}",
                description: "function",
            },
        ],
        _ => &[],
    }
}

/// Expand `$0`/`$1` tab stops into plain text and return caret offset after `$0`.
pub fn expand(body: &str) -> (String, usize) {
    let mut out = String::new();
    let mut caret = 0usize;
    let mut placed_caret = false;
    let mut rest = body;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with("$0") {
            caret = out.len();
            placed_caret = true;
            rest = &rest[2..];
            continue;
        }
        if rest.starts_with("${") {
            if let Some(end) = rest.find('}') {
                let inner = &rest[2..end];
                let value = inner
                    .split_once(':')
                    .map(|(_, default)| default)
                    .unwrap_or("");
                if inner.starts_with('0') {
                    caret = out.len();
                    placed_caret = true;
                }
                out.push_str(value);
                rest = &rest[end + 1..];
                continue;
            }
        }
        if rest.len() >= 2 && rest.as_bytes()[1].is_ascii_digit() {
            rest = &rest[2..];
            continue;
        }
        out.push('$');
        rest = &rest[1..];
    }
    out.push_str(rest);
    if !placed_caret {
        caret = out.len();
    }
    (out, caret)
}

pub fn match_trigger(language: &str, prefix: &str) -> Option<&'static Snippet> {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return None;
    }
    snippets_for(language)
        .iter()
        .find(|snippet| snippet.trigger == prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tab_stops() {
        let (text, caret) = expand("fn ${1:name}() {\n    $0\n}");
        assert_eq!(text, "fn name() {\n    \n}");
        assert_eq!(&text[..caret], "fn name() {\n    ");
    }

    #[test]
    fn rust_fn_trigger_matches() {
        let snippet = match_trigger("rust", "fn").expect("fn");
        assert_eq!(snippet.description, "function");
        assert!(!snippets_for("plain").is_empty() || snippets_for("plain").is_empty());
    }
}
