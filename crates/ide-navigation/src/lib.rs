//! Workspace file listing, fuzzy match, literal search, and a jump stack.
//! Hidden tool directories are skipped. Matching does not claim semantic analysis.
use std::fs;
use std::path::{Path, PathBuf};

pub const COMPONENT: &str = "ide-navigation";
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
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JumpList {
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
}

impl JumpList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn visit(&mut self, path: PathBuf) {
        self.back.push(path);
        self.forward.clear();
    }

    pub fn back(&mut self) -> Option<PathBuf> {
        let current = self.back.pop()?;
        self.forward.push(current);
        self.back.last().cloned()
    }

    pub fn forward(&mut self) -> Option<PathBuf> {
        let next = self.forward.pop()?;
        self.back.push(next.clone());
        Some(next)
    }
}

const SKIP: &[&str] = &[".git", "target", "node_modules"];

pub fn list_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    files
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if SKIP.iter().any(|skip| *skip == name) {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, files);
        } else if path.is_file() {
            if let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_path_buf());
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzyHit {
    pub text: String,
    pub score: i32,
    pub positions: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepHit {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
    pub start: usize,
    pub end: usize,
}

pub fn fuzzy(query: &str, candidates: &[String]) -> Vec<(String, i32)> {
    fuzzy_matches(query, candidates)
        .into_iter()
        .map(|hit| (hit.text, hit.score))
        .collect()
}

pub fn fuzzy_matches(query: &str, candidates: &[String]) -> Vec<FuzzyHit> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return candidates
            .iter()
            .cloned()
            .map(|text| FuzzyHit {
                text,
                score: 0,
                positions: Vec::new(),
            })
            .collect();
    }
    let mut scored = Vec::new();
    for candidate in candidates {
        if let Some((score, positions)) = score(candidate, &needle) {
            scored.push(FuzzyHit {
                text: candidate.clone(),
                score,
                positions,
            });
        }
    }
    scored.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.text.cmp(&right.text))
    });
    scored
}

fn score(candidate: &str, needle: &[char]) -> Option<(i32, Vec<usize>)> {
    let mut folded = Vec::new();
    for (index, ch) in candidate.chars().enumerate() {
        for lower in ch.to_lowercase() {
            folded.push((lower, index));
        }
    }
    let mut score = 0_i32;
    let mut previous = None;
    let mut from = 0;
    let mut positions = Vec::new();
    for want in needle {
        let found = folded[from..].iter().position(|(ch, _)| ch == want)?;
        let index = from + found;
        score += if previous.is_some_and(|prev| prev + 1 == index) {
            5
        } else {
            1
        };
        previous = Some(index);
        let origin = folded[index].1;
        if positions.last() != Some(&origin) {
            positions.push(origin);
        }
        from = index + 1;
    }
    Some((score, positions))
}

pub fn search_text(root: &Path, pattern: &str, limit: usize) -> Vec<GrepHit> {
    if pattern.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for relative in list_files(root) {
        if hits.len() >= limit {
            break;
        }
        let path = root.join(&relative);
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.len() > 512 * 1024 {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if text.contains('\0') {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            if hits.len() >= limit {
                break;
            }
            if let Some((start, end)) = find_ignore_case(line, pattern) {
                hits.push(GrepHit {
                    path: relative.clone(),
                    line: index + 1,
                    text: line.to_string(),
                    start,
                    end,
                });
            }
        }
    }
    hits
}

fn find_ignore_case(line: &str, pattern: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let needle: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() || chars.len() < needle.len() {
        return None;
    }
    for start in 0..=chars.len() - needle.len() {
        let matched = chars[start..start + needle.len()]
            .iter()
            .zip(&needle)
            .all(|(ch, want)| {
                let mut lower = ch.to_lowercase();
                lower.next() == Some(*want) && lower.next().is_none()
            });
        if matched {
            return Some((start, start + needle.len()));
        }
    }
    None
}

pub fn grep(root: &Path, pattern: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for relative in list_files(root) {
        let path = root.join(&relative);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if line.contains(pattern) {
                hits.push(SearchHit {
                    path: relative.clone(),
                    line: index + 1,
                    text: line.to_string(),
                });
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("axiom-nav-{}-{n}", std::process::id()));
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git/secret.txt"), "token").unwrap();
        fs::write(dir.join("src.rs"), "fn value() {}\n").unwrap();
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested/note.txt"), "value lives here\n").unwrap();
        dir
    }

    #[test]
    fn skips_git_dir_and_finds_literal_text() {
        let dir = scratch();
        let files = list_files(&dir);
        assert!(files.iter().all(|path| !path.starts_with(".git")));
        let hits = grep(&dir, "value");
        assert_eq!(hits.len(), 2);
        let live = search_text(&dir, "VALUE", 10);
        assert!(live
            .iter()
            .any(|hit| hit.start < hit.end && hit.text.to_lowercase().contains("value")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fuzzy_prefers_consecutive_matches_and_jumps_are_ordered() {
        let ranked = fuzzy("ab", &["a-b".into(), "ab".into(), "zz".into()]);
        assert_eq!(ranked[0].0, "ab");
        assert!(ranked.iter().all(|item| item.0 != "zz"));
        let hits = fuzzy_matches("ab", &["a-b".into(), "ab".into()]);
        assert_eq!(hits[0].positions, vec![0, 1]);
        assert_eq!(hits[1].positions, vec![0, 2]);
        let mut jumps = JumpList::new();
        jumps.visit(PathBuf::from("a.rs"));
        jumps.visit(PathBuf::from("b.rs"));
        assert_eq!(jumps.back().unwrap(), PathBuf::from("a.rs"));
        assert_eq!(jumps.forward().unwrap(), PathBuf::from("b.rs"));
    }
}
