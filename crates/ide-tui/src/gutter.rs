//! Editor chrome: breakpoint column, git diff marks, line numbers.
//!
//! Layout (left → right): `[bp][git][linenr ]` then code. Intuitive IDE defaults:
//! absolute numbers, optional relative distance on other lines (current stays absolute).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitMark {
    None,
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, Default)]
pub struct GutterState {
    /// 1-based document lines with breakpoints (path-keyed).
    pub breakpoints: HashMap<PathBuf, HashSet<usize>>,
    /// Relative line numbers (VS Code-style hybrid when enabled).
    pub relative_numbers: bool,
    /// Cached per-path git line marks (1-based).
    git_marks: HashMap<PathBuf, HashMap<usize, GitMark>>,
    git_revision: HashMap<PathBuf, u64>,
}

impl GutterState {
    pub fn toggle_breakpoint(&mut self, path: &Path, line: usize) -> bool {
        if line == 0 {
            return false;
        }
        let set = self.breakpoints.entry(path.to_path_buf()).or_default();
        if !set.remove(&line) {
            set.insert(line);
            true
        } else {
            false
        }
    }

    pub fn has_breakpoint(&self, path: Option<&Path>, line: usize) -> bool {
        path.and_then(|p| self.breakpoints.get(p))
            .is_some_and(|set| set.contains(&line))
    }

    pub fn width_for(&self, line_count: usize) -> u16 {
        let digits = ((line_count.max(1) as f32).log10().floor() as u16) + 1;
        // bp(1) + git(1) + gap(1) + digits + gap(1)
        1 + 1 + 1 + digits.max(2) + 1
    }

    pub fn ensure_git_marks(&mut self, workspace: &Path, path: &Path, doc_revision: u64) {
        let key = path.to_path_buf();
        if self.git_revision.get(&key) == Some(&doc_revision) {
            return;
        }
        self.git_revision.insert(key.clone(), doc_revision);
        self.git_marks.insert(key.clone(), load_git_line_marks(workspace, path));
    }

    pub fn git_mark(&self, path: Option<&Path>, line: usize) -> GitMark {
        path.and_then(|p| self.git_marks.get(p))
            .and_then(|m| m.get(&line).copied())
            .unwrap_or(GitMark::None)
    }

    /// Render one gutter row for `line` (1-based) with caret on `caret_line` (1-based).
    ///
    /// `light` selects ink that stays readable on cream hosts (White-on-cream
    /// caret numbers were effectively invisible on Ghostty's default theme).
    pub fn line_spans(
        &self,
        path: Option<&Path>,
        line: usize,
        caret_line: usize,
        line_count: usize,
        width: u16,
        light: bool,
    ) -> Vec<Span<'static>> {
        let digits = ((line_count.max(1) as f32).log10().floor() as usize) + 1;
        let digits = digits.max(2);
        let bp = if self.has_breakpoint(path, line) {
            Span::styled(
                "●",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw(" ")
        };
        let git = match self.git_mark(path, line) {
            GitMark::Added => Span::styled(
                "┃",
                Style::default().fg(if light {
                    Color::Rgb(0x2e, 0x7d, 0x32)
                } else {
                    Color::Green
                }),
            ),
            GitMark::Modified => Span::styled(
                "┃",
                Style::default().fg(if light {
                    Color::Rgb(0xb5, 0x86, 0x00)
                } else {
                    Color::Yellow
                }),
            ),
            GitMark::Deleted => Span::styled("▁", Style::default().fg(Color::Red)),
            GitMark::None => Span::raw(" "),
        };
        let number = if self.relative_numbers && line != caret_line {
            let dist = line.abs_diff(caret_line);
            format!("{dist:>digits$}")
        } else {
            format!("{line:>digits$}")
        };
        let num_style = if line == caret_line {
            Style::default()
                .fg(if light {
                    Color::Rgb(0x1a, 0x1a, 0x1a)
                } else {
                    Color::White
                })
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(if light {
                Color::Rgb(0x6e, 0x6a, 0x5e)
            } else {
                Color::DarkGray
            })
        };
        let mut spans = vec![bp, git, Span::raw(" "), Span::styled(number, num_style), Span::raw(" ")];
        let used = 1 + 1 + 1 + digits + 1;
        if (used as u16) < width {
            spans.push(Span::raw(" ".repeat((width as usize).saturating_sub(used))));
        }
        spans
    }
}

/// Parse `git diff -U0` hunks into 1-based new-file line marks.
fn load_git_line_marks(workspace: &Path, path: &Path) -> HashMap<usize, GitMark> {
    let mut marks = HashMap::new();
    let rel = path.strip_prefix(workspace).unwrap_or(path);
    let output = Command::new("git")
        .args(["diff", "-U0", "--", &rel.to_string_lossy()])
        .current_dir(workspace)
        .output();
    let Ok(output) = output else {
        return marks;
    };
    if !output.status.success() && output.stdout.is_empty() {
        return marks;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            // @@ -a,b +c,d @@
            let plus = rest.split_whitespace().find(|p| p.starts_with('+'));
            let Some(plus) = plus else { continue };
            let body = plus.trim_start_matches('+');
            let mut parts = body.split(',');
            let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let count: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
            if start == 0 {
                continue;
            }
            if count == 0 {
                marks.insert(start, GitMark::Deleted);
            } else {
                for line_no in start..start + count {
                    marks.insert(line_no, GitMark::Modified);
                }
            }
        } else if let Some(rest) = line.strip_prefix('+') {
            if rest.starts_with("++") {
                continue;
            }
            // Heuristic: last @@ region lines are Modified; treat pure adds as Added when count==1 handled above.
        }
    }
    // Second pass: mark added lines from + prefixes inside hunks more precisely.
    let mut current: Option<(usize, usize)> = None;
    let mut add_index = 0usize;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            let plus = rest.split_whitespace().find(|p| p.starts_with('+'));
            if let Some(plus) = plus {
                let body = plus.trim_start_matches('+');
                let mut parts = body.split(',');
                let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let count: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
                current = Some((start, count));
                add_index = 0;
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            if let Some((start, _)) = current {
                let line_no = start + add_index;
                marks.insert(line_no, GitMark::Added);
                add_index += 1;
            }
        } else if line.starts_with('-') && !line.starts_with("---") {
            // deletions already marked via count==0 or remain Modified
        }
    }
    marks
}

/// Map a click column inside the left gutter to an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GutterHit {
    Breakpoint,
    Number,
}

pub fn hit(column_in_gutter: u16, _width: u16) -> Option<GutterHit> {
    match column_in_gutter {
        0 => Some(GutterHit::Breakpoint),
        _ => Some(GutterHit::Number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_numbers_keep_absolute_on_caret_line() {
        let mut state = GutterState::default();
        state.relative_numbers = true;
        let spans = state.line_spans(None, 5, 5, 99, 8, false);
        let joined: String = spans.iter().map(|s| s.content.clone()).collect();
        assert!(joined.contains('5'), "{joined}");
        let spans = state.line_spans(None, 8, 5, 99, 8, false);
        let joined: String = spans.iter().map(|s| s.content.clone()).collect();
        assert!(joined.contains('3'), "{joined}");
    }

    #[test]
    fn caret_line_ink_stays_visible_on_light_hosts() {
        let state = GutterState::default();
        let light = state.line_spans(None, 3, 3, 20, 8, true);
        let dark = state.line_spans(None, 3, 3, 20, 8, false);
        let light_num = light.iter().find(|s| s.content.contains('3')).expect("num");
        let dark_num = dark.iter().find(|s| s.content.contains('3')).expect("num");
        assert_eq!(
            light_num.style.fg,
            Some(Color::Rgb(0x1a, 0x1a, 0x1a)),
            "cream hosts must not use White caret numbers"
        );
        assert_eq!(dark_num.style.fg, Some(Color::White));
        assert_ne!(light_num.style.fg, dark_num.style.fg);
    }

    #[test]
    fn toggle_breakpoint_round_trips() {
        let mut state = GutterState::default();
        let path = PathBuf::from("/tmp/demo.rs");
        assert!(state.toggle_breakpoint(&path, 12));
        assert!(state.has_breakpoint(Some(&path), 12));
        assert!(!state.toggle_breakpoint(&path, 12));
        assert!(!state.has_breakpoint(Some(&path), 12));
    }
}
