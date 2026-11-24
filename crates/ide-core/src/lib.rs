//! Revision-checked transactional document core.
//! Edits commit only after every range is valid. Offsets convert between
//! Unicode scalars, UTF-8 bytes, UTF-16 code units, and grapheme clusters.
//! Save renames a temp file in the destination directory and will not replace a symlink.
mod bench;
mod memory;
mod persist;
mod position;

use ropey::Rope;
use std::{error::Error, fmt};

pub use bench::{
    duration_ns, time_iters, time_once, SampleStats, DEFAULT_SAMPLES, DEFAULT_WARMUP,
};
pub use memory::{
    pressure_level, process_rss_bytes, PressureLevel, HISTORY_HARD_MAX, HISTORY_NORMAL,
    HISTORY_PRESSURE,
};
pub use persist::{Newline, SaveError};
pub use position::{OffsetUnit, PositionError};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// Character-based offset, not UTF-8 byte offset.
    pub start: usize,
    /// Exclusive character-based end.
    pub end: usize,
    pub replacement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    StaleRevision { expected: u64, actual: u64 },
    InvalidRange,
    OverlappingEdits,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid document edit: {self:?}")
    }
}
impl Error for EditError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
pub enum BufferError {
    Edit(EditError),
    Position(PositionError),
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "buffer error: {self:?}")
    }
}
impl Error for BufferError {}

#[derive(Debug)]
pub struct Document {
    text: Rope,
    revision: u64,
    undo: Vec<Rope>,
    redo: Vec<Rope>,
    newline: Newline,
    /// Dynamic undo/redo cap (tightens under OS memory pressure).
    history_limit: usize,
    /// End caret of an in-progress single-char typing run (undo coalescing).
    typing_coalesce_end: Option<usize>,
}

impl Document {
    pub fn new(text: &str) -> Self {
        Self {
            text: Rope::from_str(text),
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            newline: Newline::Lf,
            history_limit: HISTORY_NORMAL.min(HISTORY_HARD_MAX),
            typing_coalesce_end: None,
        }
    }

    pub(crate) fn rope(&self) -> &Rope {
        &self.text
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }
    /// Rope line count (trailing newline yields an empty final line).
    pub fn len_lines(&self) -> usize {
        self.text.len_lines().max(1)
    }
    /// Line index containing `char_idx` (clamped).
    pub fn char_to_line(&self, char_idx: usize) -> usize {
        let len = self.text.len_chars();
        if len == 0 {
            return 0;
        }
        let idx = char_idx.min(len.saturating_sub(1));
        self.text.char_to_line(idx)
    }
    /// First character index of `line` (clamped).
    pub fn line_to_char(&self, line: usize) -> usize {
        let lines = self.text.len_lines();
        if lines == 0 {
            return 0;
        }
        self.text.line_to_char(line.min(lines.saturating_sub(1)))
    }

    /// First `max_chars` scalars of `line` (0-based), without the trailing newline.
    ///
    /// Used by the editor minimap to sample a sliding window without materializing
    /// the whole document string.
    pub fn line_prefix(&self, line: usize, max_chars: usize) -> String {
        if max_chars == 0 || self.text.len_chars() == 0 {
            return String::new();
        }
        let lines = self.text.len_lines();
        if line >= lines {
            return String::new();
        }
        let slice = self.text.line(line);
        let mut out = String::with_capacity(max_chars.min(64));
        let mut n = 0usize;
        for ch in slice.chars() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            out.push(ch);
            n += 1;
            if n >= max_chars {
                break;
            }
        }
        out
    }

    pub fn to_text(&self) -> String {
        self.text.to_string()
    }
    pub fn is_empty(&self) -> bool {
        self.text.len_chars() == 0
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// Apply the host memory-pressure budget and drop excess snapshots.
    pub fn apply_pressure(&mut self, level: PressureLevel) {
        self.set_history_limit(level.history_limit());
    }

    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit.clamp(1, HISTORY_HARD_MAX);
        self.trim_history();
    }

    fn trim_history(&mut self) {
        let limit = self.history_limit;
        if self.undo.len() > limit {
            let drop = self.undo.len() - limit;
            self.undo.drain(0..drop);
        }
        if self.redo.len() > limit {
            let drop = self.redo.len() - limit;
            self.redo.drain(0..drop);
        }
    }

    /// Drop redo entirely and keep only the newest `keep` undo snapshots.
    pub fn compact(&mut self, keep: usize) {
        self.redo.clear();
        self.set_history_limit(keep.max(1));
    }

    /// Edits are in document coordinates before the transaction.
    /// Validate all operations before making *any* changes.
    ///
    /// Consecutive single-character inserts at the advancing caret coalesce into
    /// one undo snapshot (typing a word is one Undo, not N rope clones).
    pub fn apply(&mut self, expected_revision: u64, edits: &[Edit]) -> Result<u64, EditError> {
        if self.revision != expected_revision {
            return Err(EditError::StaleRevision {
                expected: expected_revision,
                actual: self.revision,
            });
        }
        if edits.is_empty() {
            return Ok(self.revision);
        }
        // Fast path: single caret insert (dominant typing path).
        if edits.len() == 1 {
            let e = &edits[0];
            if e.start > e.end || e.end > self.text.len_chars() {
                return Err(EditError::InvalidRange);
            }
            let single_insert =
                e.start == e.end && e.replacement.chars().count() == 1;
            if single_insert && self.typing_coalesce_end == Some(e.start) {
                self.text.insert(e.start, &e.replacement);
                self.typing_coalesce_end = Some(e.start + 1);
                self.redo.clear();
                self.revision += 1;
                return Ok(self.revision);
            }
            let prior = self.text.clone();
            self.text.remove(e.start..e.end);
            self.text.insert(e.start, &e.replacement);
            self.undo.push(prior);
            self.redo.clear();
            self.trim_history();
            self.typing_coalesce_end = if single_insert {
                Some(e.start + 1)
            } else {
                None
            };
            self.revision += 1;
            return Ok(self.revision);
        }

        let mut sorted: Vec<&Edit> = edits.iter().collect();
        sorted.sort_by_key(|e| (e.start, e.end));
        let mut prev_end = 0;
        for (i, e) in sorted.iter().enumerate() {
            if e.start > e.end || e.end > self.text.len_chars() {
                return Err(EditError::InvalidRange);
            }
            let touches_same_caret = i > 0
                && e.start == prev_end
                && e.start == e.end
                && sorted[i - 1].start == sorted[i - 1].end;
            if i > 0 && (e.start < prev_end || touches_same_caret) {
                return Err(EditError::OverlappingEdits);
            }
            prev_end = e.end;
        }

        // A clone is an independent rope snapshot for undo, not a full string copy.
        let prior = self.text.clone();
        for e in sorted.into_iter().rev() {
            self.text.remove(e.start..e.end);
            self.text.insert(e.start, &e.replacement);
        }
        self.undo.push(prior);
        self.redo.clear();
        self.trim_history();
        self.typing_coalesce_end = None;
        self.revision += 1;
        Ok(self.revision)
    }

    pub fn undo(&mut self) -> bool {
        if let Some(before) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.text, before));
            self.typing_coalesce_end = None;
            self.trim_history();
            self.revision += 1;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(after) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.text, after));
            self.typing_coalesce_end = None;
            self.trim_history();
            self.revision += 1;
            true
        } else {
            false
        }
    }

    /// Apply one replacement at every selection as a single revision.
    /// Returned selections are carets after each replacement.
    pub fn apply_selections(
        &mut self,
        expected_revision: u64,
        selections: &[Selection],
        replacement: &str,
    ) -> Result<(u64, Vec<Selection>), EditError> {
        if self.revision != expected_revision {
            return Err(EditError::StaleRevision {
                expected: expected_revision,
                actual: self.revision,
            });
        }
        let mut ordered = selections.to_vec();
        ordered.sort_by_key(|sel| (sel.start, sel.end));
        for pair in ordered.windows(2) {
            if selection_overlap(&pair[0], &pair[1]) {
                return Err(EditError::OverlappingEdits);
            }
        }
        let edits: Vec<Edit> = ordered
            .iter()
            .map(|sel| Edit {
                start: sel.start,
                end: sel.end,
                replacement: replacement.to_string(),
            })
            .collect();
        let revision = self.apply(expected_revision, &edits)?;
        let carets = edits
            .iter()
            .map(|edit| {
                let caret = map_point(edit.end, &edits);
                Selection {
                    start: caret,
                    end: caret,
                }
            })
            .collect();
        Ok((revision, carets))
    }

    /// Edit by UTF-8, UTF-16, character, or grapheme offsets.
    /// A bad offset leaves the document unchanged.
    pub fn apply_unit(
        &mut self,
        expected_revision: u64,
        start: usize,
        end: usize,
        unit: OffsetUnit,
        replacement: &str,
    ) -> Result<u64, BufferError> {
        if self.revision != expected_revision {
            return Err(BufferError::Edit(EditError::StaleRevision {
                expected: expected_revision,
                actual: self.revision,
            }));
        }
        let start_char = self
            .unit_to_char(start, unit)
            .map_err(BufferError::Position)?;
        let end_char = self
            .unit_to_char(end, unit)
            .map_err(BufferError::Position)?;
        self.apply(
            expected_revision,
            &[Edit {
                start: start_char,
                end: end_char,
                replacement: replacement.into(),
            }],
        )
        .map_err(BufferError::Edit)
    }
}

fn selection_overlap(left: &Selection, right: &Selection) -> bool {
    if left.start == left.end && right.start == right.end {
        return left.start == right.start;
    }
    left.start < right.end && right.start < left.end
}

fn map_point(pos: usize, edits: &[Edit]) -> usize {
    let mut delta: isize = 0;
    for edit in edits {
        let old_len = edit.end - edit.start;
        let new_len = edit.replacement.chars().count();
        if edit.end <= pos {
            delta += new_len as isize - old_len as isize;
        } else if edit.start < pos {
            return (edit.start as isize + delta) as usize + new_len;
        }
    }
    (pos as isize + delta) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn line_prefix_culls_without_full_text() {
        let d = Document::new("alpha\nbeta-gamma\n");
        assert_eq!(d.line_prefix(0, 3), "alp");
        assert_eq!(d.line_prefix(1, 4), "beta");
        assert_eq!(d.line_prefix(1, 64), "beta-gamma");
        assert_eq!(d.line_prefix(99, 8), "");
    }

    /// Conformance for `bend/doc/LAWS.bend` (decide_fresh_bumps / apply_stale_*).
    #[test]
    fn bend_doc_laws_conformance() {
        // apply_zero_fresh / decide_fresh_bumps: matching rev → Ok, rev+1
        let mut d = Document::new("x");
        assert_eq!(d.revision(), 0);
        assert_eq!(
            d.apply(
                0,
                &[Edit {
                    start: 1,
                    end: 1,
                    replacement: "y".into(),
                }]
            ),
            Ok(1)
        );
        assert_eq!(d.revision(), 1);
        assert_eq!(d.to_text(), "xy");

        // apply_stale_keeps_rev / decide_stale_preserves
        let before = d.to_text();
        let rev = d.revision();
        assert!(matches!(
            d.apply(
                0,
                &[Edit {
                    start: 0,
                    end: 1,
                    replacement: "Z".into(),
                }]
            ),
            Err(EditError::StaleRevision { .. })
        ));
        assert_eq!(d.revision(), rev);
        assert_eq!(d.to_text(), before);

        // apply_fresh_revision_is_succ (concrete 7→8 path via successive applies)
        let mut e = Document::new("");
        for expected in 0u64..7 {
            assert_eq!(
                e.apply(
                    expected,
                    &[Edit {
                        start: e.len_chars(),
                        end: e.len_chars(),
                        replacement: ".".into(),
                    }]
                ),
                Ok(expected + 1)
            );
        }
        assert_eq!(e.revision(), 7);
        assert_eq!(
            e.apply(
                7,
                &[Edit {
                    start: e.len_chars(),
                    end: e.len_chars(),
                    replacement: "!".into(),
                }]
            ),
            Ok(8)
        );
    }

    #[test]
    fn atomic_edit_and_undo() {
        let mut d = Document::new("A🦀C");
        let edits = [Edit {
            start: 1,
            end: 2,
            replacement: "rust".into(),
        }];
        assert_eq!(d.apply(0, &edits), Ok(1));
        assert_eq!(d.to_text(), "ArustC");
        assert!(d.undo());
        assert_eq!(d.to_text(), "A🦀C");
        assert!(d.redo());
        assert_eq!(d.to_text(), "ArustC");
    }
    #[test]
    fn stale_revision_leaves_document_unchanged() {
        let mut d = Document::new("é🙂");
        assert!(matches!(
            d.apply(
                1,
                &[Edit {
                    start: 0,
                    end: 1,
                    replacement: "X".into()
                }]
            ),
            Err(EditError::StaleRevision { .. })
        ));
        assert_eq!(d.to_text(), "é🙂");
        assert_eq!(d.revision(), 0);
    }
    #[test]
    fn overlapping_edits_rejected_atomically() {
        let mut d = Document::new("abcde");
        let bad = [
            Edit {
                start: 0,
                end: 3,
                replacement: "Z".into(),
            },
            Edit {
                start: 2,
                end: 4,
                replacement: "Q".into(),
            },
        ];
        assert_eq!(d.apply(0, &bad), Err(EditError::OverlappingEdits));
        assert_eq!(d.to_text(), "abcde");
    }
    #[test]
    fn multi_edit_reverse_order_is_stable() {
        let mut d = Document::new("abcdef");
        d.apply(
            0,
            &[
                Edit {
                    start: 1,
                    end: 2,
                    replacement: "X".into(),
                },
                Edit {
                    start: 4,
                    end: 5,
                    replacement: "Y".into(),
                },
            ],
        )
        .unwrap();
        assert_eq!(d.to_text(), "aXcdYf");
    }
    #[test]
    fn same_caret_inserts_do_not_apply() {
        let mut d = Document::new("ab");
        let bad = [
            Edit {
                start: 1,
                end: 1,
                replacement: "X".into(),
            },
            Edit {
                start: 1,
                end: 1,
                replacement: "Y".into(),
            },
        ];
        assert_eq!(d.apply(0, &bad), Err(EditError::OverlappingEdits));
        assert_eq!(d.to_text(), "ab");
        assert_eq!(d.revision(), 0);
    }
    #[test]
    fn multi_cursor_is_one_revision_and_undo() {
        let mut d = Document::new("abcd");
        let sels = [
            Selection { start: 0, end: 0 },
            Selection { start: 2, end: 2 },
        ];
        let (rev, carets) = d.apply_selections(0, &sels, "X").unwrap();
        assert_eq!(rev, 1);
        assert_eq!(d.to_text(), "XabXcd");
        assert_eq!(
            carets,
            vec![
                Selection { start: 1, end: 1 },
                Selection { start: 4, end: 4 }
            ]
        );
        assert!(d.undo());
        assert_eq!(d.to_text(), "abcd");
    }
    #[test]
    fn overlapping_cursors_leave_text_unchanged() {
        let mut d = Document::new("abcde");
        let sels = [
            Selection { start: 0, end: 3 },
            Selection { start: 2, end: 4 },
        ];
        assert_eq!(
            d.apply_selections(0, &sels, "Z"),
            Err(EditError::OverlappingEdits)
        );
        assert_eq!(d.to_text(), "abcde");
        assert_eq!(d.revision(), 0);
    }
    #[test]
    fn unit_edit_rejects_a_split_scalar_and_deletes_whole_graphemes() {
        let mut d = Document::new("a\u{0301}🦀");
        assert_eq!(
            d.char_to_unit(1, OffsetUnit::Grapheme),
            Err(PositionError::MidGrapheme)
        );
        let before = d.to_text();
        let crab = d.char_to_unit(2, OffsetUnit::Utf8).unwrap();
        assert!(matches!(
            d.apply_unit(0, crab + 1, crab + 2, OffsetUnit::Utf8, "x"),
            Err(BufferError::Position(PositionError::MidScalar))
        ));
        assert_eq!(d.to_text(), before);
        assert_eq!(d.revision(), 0);
        d.apply_unit(0, 0, 1, OffsetUnit::Grapheme, "").unwrap();
        assert_eq!(d.to_text(), "🦀");
        assert_eq!(d.revision(), 1);
    }

    #[test]
    fn undo_history_stays_bounded_under_edit_storm() {
        let payload = "x".repeat(8_192);
        let mut d = Document::new(&payload);
        for _ in 0..2_000 {
            let rev = d.revision();
            d.apply(
                rev,
                &[Edit {
                    start: 0,
                    end: 0,
                    replacement: "a".into(),
                }],
            )
            .unwrap();
        }
        assert!(
            d.undo_len() <= HISTORY_HARD_MAX,
            "undo grew without bound: {}",
            d.undo_len()
        );
        assert!(
            d.undo_len() <= HISTORY_NORMAL,
            "undo exceeded normal cap: {}",
            d.undo_len()
        );
        // Newest edits must remain undoable.
        assert!(d.undo());
        assert!(d.to_text().starts_with('a') || d.to_text().starts_with('x'));
    }

    #[test]
    fn typing_run_coalesces_into_one_undo() {
        let mut d = Document::new("");
        for (i, ch) in "hello".chars().enumerate() {
            let rev = d.revision();
            d.apply(
                rev,
                &[Edit {
                    start: i,
                    end: i,
                    replacement: ch.to_string(),
                }],
            )
            .unwrap();
        }
        assert_eq!(d.to_text(), "hello");
        assert_eq!(d.undo_len(), 1, "typing should be one undo snapshot");
        assert!(d.undo());
        assert_eq!(d.to_text(), "");
    }

    #[test]
    fn pressure_compacts_history_aggressively() {
        let mut d = Document::new("seed");
        for _ in 0..80 {
            let rev = d.revision();
            d.apply(
                rev,
                &[Edit {
                    start: 0,
                    end: 0,
                    replacement: ".".into(),
                }],
            )
            .unwrap();
        }
        assert_eq!(d.undo_len(), HISTORY_NORMAL);
        d.apply_pressure(PressureLevel::Critical);
        assert!(d.undo_len() <= HISTORY_PRESSURE);
        assert_eq!(d.redo_len(), 0);
    }
}
