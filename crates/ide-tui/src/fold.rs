//! Code folding: VS Code–style gutter chevrons with LSP-preferred ranges.
//!
//! Ranges are 0-based inclusive line indices. Collapsing a range hides
//! `start_line + 1 ..= end_line` (the header line stays visible with a chevron).

use std::collections::HashSet;

use ide_syntax::FoldRange;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldSource {
    Syntax,
    Lsp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chevron {
    /// Collapsed — body hidden (`▸`).
    Collapsed,
    /// Expanded — body visible (`▾`).
    Expanded,
}

#[derive(Clone, Debug, Default)]
pub struct FoldState {
    ranges: Vec<FoldRange>,
    collapsed: HashSet<(usize, usize)>,
    source: Option<FoldSource>,
    revision: Option<u64>,
    /// When set, refresh prefers these over syntax folds.
    lsp_ranges: Option<Vec<FoldRange>>,
}

impl FoldState {
    pub fn ranges(&self) -> &[FoldRange] {
        &self.ranges
    }

    pub fn source(&self) -> Option<FoldSource> {
        self.source
    }

    pub fn collapsed_count(&self) -> usize {
        self.collapsed.len()
    }

    /// Inject LSP folding ranges (0-based). `None` clears LSP preference.
    pub fn set_lsp_ranges(&mut self, ranges: Option<Vec<FoldRange>>) {
        self.lsp_ranges = ranges;
        self.revision = None;
    }

    pub fn refresh(&mut self, text: &str, doc_revision: u64) {
        if self.revision == Some(doc_revision) {
            return;
        }
        self.revision = Some(doc_revision);
        if let Some(lsp) = &self.lsp_ranges {
            self.ranges = lsp.clone();
            self.source = Some(FoldSource::Lsp);
        } else {
            self.ranges = ide_syntax::fold_ranges(text);
            self.source = if self.ranges.is_empty() {
                None
            } else {
                Some(FoldSource::Syntax)
            };
        }
        self.collapsed
            .retain(|&(s, e)| self.ranges.iter().any(|r| r.start_line == s && r.end_line == e));
    }

    /// Outermost fold that starts on `line` (0-based), if any.
    pub fn range_at(&self, line: usize) -> Option<&FoldRange> {
        self.ranges
            .iter()
            .filter(|r| r.start_line == line && r.end_line > line)
            .max_by_key(|r| r.end_line)
    }

    pub fn chevron(&self, line: usize) -> Option<Chevron> {
        let range = self.range_at(line)?;
        let key = (range.start_line, range.end_line);
        Some(if self.collapsed.contains(&key) {
            Chevron::Collapsed
        } else {
            Chevron::Expanded
        })
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        self.collapsed
            .iter()
            .any(|&(start, end)| line > start && line <= end)
    }

    pub fn toggle_at(&mut self, line: usize) -> Option<Chevron> {
        let range = self.range_at(line)?;
        let key = (range.start_line, range.end_line);
        if self.collapsed.contains(&key) {
            self.collapsed.remove(&key);
            Some(Chevron::Expanded)
        } else {
            self.collapsed.insert(key);
            Some(Chevron::Collapsed)
        }
    }

    pub fn close_at(&mut self, line: usize) -> bool {
        let Some(range) = self.range_at(line) else {
            return false;
        };
        let key = (range.start_line, range.end_line);
        self.collapsed.insert(key);
        true
    }

    pub fn open_at(&mut self, line: usize) -> bool {
        let Some(range) = self.range_at(line) else {
            // Also open a fold that currently hides this line.
            let victim: Option<(usize, usize)> = self
                .collapsed
                .iter()
                .copied()
                .filter(|&(s, e)| line > s && line <= e)
                .min_by_key(|&(s, e)| e.saturating_sub(s));
            if let Some(key) = victim {
                self.collapsed.remove(&key);
                return true;
            }
            return false;
        };
        let key = (range.start_line, range.end_line);
        self.collapsed.remove(&key)
    }

    /// Document lines (0-based) shown starting at `scroll` for up to `count` rows.
    pub fn visible_window(&self, scroll: usize, count: usize, line_count: usize) -> Vec<usize> {
        let total = line_count.max(1);
        let mut out = Vec::with_capacity(count);
        let mut line = scroll.min(total.saturating_sub(1));
        // Skip leading hidden lines so scroll lands on a visible row.
        while line < total && self.is_hidden(line) {
            line += 1;
        }
        while out.len() < count && line < total {
            if !self.is_hidden(line) {
                out.push(line);
            }
            line += 1;
        }
        out
    }

    /// Map a view row under `scroll` to a 0-based document line.
    pub fn doc_line_for_view_row(&self, scroll: usize, view_row: usize, line_count: usize) -> Option<usize> {
        self.visible_window(scroll, view_row.saturating_add(1), line_count)
            .into_iter()
            .nth(view_row)
    }

    /// Count of non-hidden lines (for scrollbar totals).
    pub fn visible_line_count(&self, line_count: usize) -> usize {
        let total = line_count.max(1);
        (0..total).filter(|&line| !self.is_hidden(line)).count().max(1)
    }
}

pub fn chevron_span(chevron: Option<Chevron>) -> Span<'static> {
    match chevron {
        Some(Chevron::Collapsed) => Span::styled(
            "▸",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Some(Chevron::Expanded) => Span::styled("▾", Style::default().fg(Color::DarkGray)),
        None => Span::raw(" "),
    }
}

/// Convert LSP folding-range JSON result into syntax-compatible ranges.
pub fn ranges_from_lsp_json(result: &serde_json::Value) -> Vec<FoldRange> {
    ide_lsp::parse_folding_ranges(result)
        .into_iter()
        .map(|r| FoldRange {
            start_line: r.start_line,
            end_line: r.end_line,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn syntax_refresh_and_chevron_toggle() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        let mut folds = FoldState::default();
        folds.refresh(text, 1);
        assert!(folds.ranges().iter().any(|r| r.start_line == 0));
        assert_eq!(folds.chevron(0), Some(Chevron::Expanded));
        assert_eq!(folds.toggle_at(0), Some(Chevron::Collapsed));
        assert!(folds.is_hidden(1));
        assert!(!folds.is_hidden(0));
        // Closing brace is inside the fold (VS Code–style); only the header stays.
        assert_eq!(folds.visible_window(0, 10, 3), vec![0]);
        assert_eq!(folds.toggle_at(0), Some(Chevron::Expanded));
        assert!(!folds.is_hidden(1));
    }

    #[test]
    fn lsp_ranges_preferred_over_syntax() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        let mut folds = FoldState::default();
        folds.set_lsp_ranges(Some(vec![FoldRange {
            start_line: 0,
            end_line: 1,
        }]));
        folds.refresh(text, 1);
        assert_eq!(folds.source(), Some(FoldSource::Lsp));
        assert_eq!(folds.ranges().len(), 1);
        assert_eq!(folds.ranges()[0].end_line, 1);
    }

    #[test]
    fn lsp_json_parse_feeds_fold_state() {
        let result = json!([
            {"startLine": 2, "endLine": 5, "kind": "region"},
            {"startLine": 10, "endLine": 12}
        ]);
        let ranges = ranges_from_lsp_json(&result);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 2);
        assert_eq!(ranges[0].end_line, 5);
    }
}
