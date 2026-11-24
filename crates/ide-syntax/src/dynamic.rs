//! Load tree-sitter grammars from a nixpkgs `withPlugins (_: allGrammars)` farm.
//!
//! Directory layout (from `pkgs.tree-sitter-grammars.withPlugins`):
//!   `$POSIT_TREE_SITTER_GRAMMARS/bash.so` → `tree_sitter_bash`
//!
//! Only paths from the env (or an explicit test override) are searched —
//! never the cwd — so untrusted local dylibs are not auto-loaded.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use libloading::{Library, Symbol};
use tree_sitter::Language;
use tree_sitter_language::LanguageFn;

type EntryFn = unsafe extern "C" fn() -> *const ();

struct Loaded {
    /// Keep the dylib mapped for as long as `language` is used.
    _library: Library,
    language: Language,
}

static CACHE: OnceLock<Mutex<HashMap<String, Loaded>>> = OnceLock::new();

thread_local! {
    static DIRS_OVERRIDE: std::cell::RefCell<Option<Vec<PathBuf>>> =
        const { std::cell::RefCell::new(None) };
}

/// Test helper: prefer these grammar dirs on the current thread.
#[cfg(test)]
pub fn with_grammar_dirs<R>(dirs: &[PathBuf], f: impl FnOnce() -> R) -> R {
    DIRS_OVERRIDE.with(|slot| {
        let previous = slot.replace(Some(dirs.to_vec()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        *slot.borrow_mut() = previous;
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

fn grammar_dirs() -> Vec<PathBuf> {
    if let Some(dirs) = DIRS_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return dirs;
    }
    let mut dirs = Vec::new();
    for key in ["POSIT_TREE_SITTER_GRAMMARS", "TREE_SITTER_GRAMMAR_PATH"] {
        if let Ok(value) = std::env::var(key) {
            for part in value.split(':') {
                let part = part.trim();
                if !part.is_empty() {
                    dirs.push(PathBuf::from(part));
                }
            }
        }
    }
    dirs
}

/// Resolve language id → shared-library stem(s) in the allGrammars farm.
fn grammar_stems(language: &str) -> Vec<String> {
    let lower = language.to_ascii_lowercase();
    let mut stems = Vec::new();
    match lower.as_str() {
        "shell" | "bash" | "sh" | "zsh" => {
            stems.push("bash".into());
        }
        "csharp" | "c#" | "c-sharp" | "cs" => {
            stems.push("c_sharp".into());
        }
        "objc" | "objective-c" | "objectivec" => {
            stems.push("objc".into());
        }
        "cpp" | "c++" | "cxx" => {
            stems.push("cpp".into());
        }
        "tsx" => {
            stems.push("tsx".into());
        }
        "typescript" | "ts" => {
            stems.push("typescript".into());
        }
        "javascript" | "js" => {
            stems.push("javascript".into());
        }
        "markdown" | "md" => {
            stems.push("markdown".into());
        }
        "dockerfile" | "docker" => {
            stems.push("dockerfile".into());
        }
        "make" | "makefile" => {
            stems.push("make".into());
        }
        "yaml" | "yml" => {
            stems.push("yaml".into());
        }
        other => {
            stems.push(other.replace('-', "_"));
            if other.contains('-') {
                stems.push(other.to_string());
            }
        }
    }
    stems.sort();
    stems.dedup();
    stems
}

/// Try to load a grammar from the configured allGrammars farm.
pub fn load(language: &str) -> Option<Language> {
    let dirs = grammar_dirs();
    if dirs.is_empty() {
        return None;
    }
    for stem in grammar_stems(language) {
        if let Some(lang) = load_stem(&dirs, &stem) {
            return Some(lang);
        }
    }
    None
}

fn load_stem(dirs: &[PathBuf], stem: &str) -> Option<Language> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache.lock().ok()?;
        if let Some(entry) = guard.get(stem) {
            return Some(entry.language.clone());
        }
    }

    let mut last_error = None;
    for dir in dirs {
        match try_load_file(dir, stem) {
            Ok(loaded) => {
                let language = loaded.language.clone();
                if let Ok(mut guard) = cache.lock() {
                    guard.entry(stem.to_string()).or_insert(loaded);
                }
                return Some(language);
            }
            Err(error) => last_error = Some(error),
        }
    }
    let _ = last_error;
    None
}

fn try_load_file(dir: &Path, stem: &str) -> Result<Loaded, String> {
    // nixpkgs farm always uses `.so` even on Darwin (Mach-O still).
    let path = dir.join(format!("{stem}.so"));
    if !path.exists() {
        return Err(format!("missing {}", path.display()));
    }
    // SAFETY: grammar farms from nixpkgs are trusted system parsers. We only
    // open paths under POSIT_TREE_SITTER_GRAMMARS / TREE_SITTER_GRAMMAR_PATH.
    let library = unsafe { Library::new(&path) }.map_err(|error| error.to_string())?;
    let symbol_name = format!("tree_sitter_{stem}");
    let language = {
        let symbol: Symbol<'_, EntryFn> = unsafe { library.get(symbol_name.as_bytes()) }
            .map_err(|error| format!("{symbol_name}: {error}"))?;
        let entry: EntryFn = *symbol;
        // SAFETY: entry is the grammar constructor exported by tree-sitter-cli.
        let language_fn = unsafe { LanguageFn::from_raw(entry) };
        Language::new(language_fn)
    };
    Ok(Loaded {
        _library: library,
        language,
    })
}

/// Extension → language id for grammars that may only exist in the allGrammars farm.
pub fn language_for_extension(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "go" => "go",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "yml" | "yaml" => "yaml",
        "lua" => "lua",
        "rb" => "ruby",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "hs" => "haskell",
        "ex" | "exs" => "elixir",
        "erl" => "erlang",
        "ml" | "mli" => "ocaml",
        "clj" | "cljs" | "cljc" => "clojure",
        "scala" => "scala",
        "sql" => "sql",
        "vue" => "vue",
        "svelte" => "svelte",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "tf" | "hcl" => "hcl",
        "dockerfile" => "dockerfile",
        "makefile" => "make",
        "cmake" => "cmake",
        "vim" => "vim",
        "elm" => "elm",
        "zig" => "zig",
        "toml" => "toml",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "sh" | "bash" | "zsh" => "shell",
        "ini" | "cfg" | "conf" => "ini",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_map_shell_and_csharp() {
        assert_eq!(grammar_stems("shell"), vec!["bash".to_string()]);
        assert_eq!(grammar_stems("c#"), vec!["c_sharp".to_string()]);
        assert_eq!(grammar_stems("cpp"), vec!["cpp".to_string()]);
    }

    #[test]
    fn loads_bash_from_allgrammars_farm_when_present() {
        let farm = PathBuf::from("/tmp/ts-all");
        if !farm.join("bash.so").exists() {
            return;
        }
        with_grammar_dirs(&[farm], || {
            let language = load("shell").expect("bash grammar");
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&language).expect("set language");
            let tree = parser.parse("export HOME=1\n", None).expect("parse");
            assert!(!tree.root_node().has_error());
        });
    }
}
