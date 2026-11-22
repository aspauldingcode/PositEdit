//! Adaptive code minimap. Cached by document revision.
//!
//! When line samples are provided, each minimap row shows a real character from
//! the mapped source line (VS Code-style density at terminal resolution).

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MinimapPriority {
    Structure = 0,
    Diff = 1,
    Diagnostics = 2,
    Debugger = 3,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimapCell {
    pub glyph: char,
    pub priority: MinimapPriority,
}

#[derive(Clone, Debug, Default)]
pub struct MinimapCache {
    revision: u64,
    cells: Vec<MinimapCell>,
    hidden: bool,
    sample_sig: u64,
}

impl MinimapCache {
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn cells(&self) -> &[MinimapCell] {
        &self.cells
    }

    pub fn render(
        &mut self,
        revision: u64,
        line_count: usize,
        tile_width: u16,
        frame_budget_ok: bool,
        priority: MinimapPriority,
        braille: bool,
    ) -> Option<&[MinimapCell]> {
        self.render_sampled(
            revision,
            line_count,
            tile_width,
            frame_budget_ok,
            priority,
            braille,
            None,
        )
    }

    /// `samples[i]` is the representative glyph for source line `i` (0-based).
    #[allow(clippy::too_many_arguments)]
    pub fn render_sampled(
        &mut self,
        revision: u64,
        line_count: usize,
        tile_width: u16,
        frame_budget_ok: bool,
        priority: MinimapPriority,
        braille: bool,
        samples: Option<&[char]>,
    ) -> Option<&[MinimapCell]> {
        if tile_width < 12 || !frame_budget_ok {
            self.hidden = true;
            return None;
        }
        self.hidden = false;
        let sig = samples
            .map(|s| s.len() as u64)
            .unwrap_or(0)
            .wrapping_mul(0x9E37)
            ^ revision;
        if self.revision == revision && self.sample_sig == sig && !self.cells.is_empty() {
            return Some(&self.cells);
        }
        self.revision = revision;
        self.sample_sig = sig;
        self.cells = if let Some(samples) = samples {
            build_from_samples(line_count, samples, priority)
        } else {
            build_heatmap(line_count, priority, braille)
        };
        Some(&self.cells)
    }

    /// Map a minimap row click to a 0-based source line.
    pub fn line_for_row(line_count: usize, rows: usize, row: usize) -> usize {
        let rows = rows.max(1);
        let line_count = line_count.max(1);
        ((row * line_count) / rows).min(line_count.saturating_sub(1))
    }
}

fn build_from_samples(
    line_count: usize,
    samples: &[char],
    priority: MinimapPriority,
) -> Vec<MinimapCell> {
    let rows = line_count.clamp(1, 256);
    (0..rows)
        .map(|row| {
            let src = MinimapCache::line_for_row(line_count, rows, row);
            let glyph = samples.get(src).copied().unwrap_or(' ');
            let glyph = if glyph.is_whitespace() { '·' } else { glyph };
            MinimapCell { glyph, priority }
        })
        .collect()
}

fn build_heatmap(line_count: usize, priority: MinimapPriority, braille: bool) -> Vec<MinimapCell> {
    let rows = line_count.max(1).div_ceil(4).min(64);
    let dense = match priority {
        MinimapPriority::Debugger => {
            if braille {
                '⣿'
            } else {
                '#'
            }
        }
        MinimapPriority::Diagnostics => {
            if braille {
                '⣶'
            } else {
                '*'
            }
        }
        MinimapPriority::Diff => {
            if braille {
                '⣤'
            } else {
                '+'
            }
        }
        MinimapPriority::Structure => {
            if braille {
                '⠄'
            } else {
                '.'
            }
        }
    };
    (0..rows)
        .map(|row| MinimapCell {
            glyph: if row % 3 == 0 {
                dense
            } else if braille {
                '⠁'
            } else {
                ' '
            },
            priority,
        })
        .collect()
}

/// Pure priority picker for Bend differential tests.
pub fn pick_priority(debugger: bool, failing_diag: bool, has_diff: bool) -> MinimapPriority {
    if debugger {
        MinimapPriority::Debugger
    } else if failing_diag {
        MinimapPriority::Diagnostics
    } else if has_diff {
        MinimapPriority::Diff
    } else {
        MinimapPriority::Structure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_when_narrow_and_caches_revision() {
        let mut cache = MinimapCache::default();
        assert!(cache
            .render(1, 40, 8, true, MinimapPriority::Structure, true)
            .is_none());
        let first = cache
            .render(1, 40, 40, true, MinimapPriority::Structure, false)
            .map(|c| c.len())
            .unwrap();
        let second = cache
            .render(1, 40, 40, true, MinimapPriority::Debugger, false)
            .map(|c| c.len())
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(pick_priority(true, true, true), MinimapPriority::Debugger);
    }

    #[test]
    fn samples_real_glyphs_and_maps_rows() {
        let mut cache = MinimapCache::default();
        let samples: Vec<char> = "abcdef".chars().collect();
        let cells = cache
            .render_sampled(
                2,
                6,
                40,
                true,
                MinimapPriority::Structure,
                false,
                Some(&samples),
            )
            .unwrap();
        assert_eq!(cells[0].glyph, 'a');
        assert_eq!(MinimapCache::line_for_row(100, 10, 5), 50);
    }
}
