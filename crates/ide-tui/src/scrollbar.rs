//! Incremental right-edge scrollbars for every scrolling surface.
//!
//! Geometry is O(1). Thumb position is tracked in **eighths of a cell** so the
//! handle eases through a row instead of jumping a full terminal cell — text only,
//! via Unicode block elements.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

/// Vertical subdivisions per terminal cell (Unicode eighth-blocks).
pub const SUBDIV: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollMetrics {
    pub offset: usize,
    pub visible: usize,
    pub total: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FineThumb {
    /// Top of thumb in eighth-cells from the track origin.
    pub top_units: usize,
    /// Thumb height in eighth-cells.
    pub thumb_units: usize,
    /// Track height in eighth-cells (`track_cells * SUBDIV`).
    pub track_units: usize,
}

impl ScrollMetrics {
    #[allow(dead_code)] // coarse cell API kept for callers/tests; paint uses fine_thumb.
    pub fn thumb(self, track: u16) -> Option<(u16, u16)> {
        let fine = self.fine_thumb(track)?;
        let top = (fine.top_units / SUBDIV) as u16;
        let bottom = fine
            .top_units
            .saturating_add(fine.thumb_units)
            .div_ceil(SUBDIV)
            .min(fine.track_units.div_ceil(SUBDIV));
        let height = bottom.saturating_sub(top as usize).max(1) as u16;
        Some((top, height))
    }

    /// Sub-cell thumb geometry for smooth visualization.
    pub fn fine_thumb(self, track: u16) -> Option<FineThumb> {
        let track = track.max(1) as usize;
        let total = self.total.max(1);
        let visible = self.visible.max(1).min(total);
        if total <= visible || track <= 1 {
            return None;
        }
        let track_units = track.saturating_mul(SUBDIV);
        let thumb_units = ((track_units * visible) / total).max(1).min(track_units);
        let max_offset = total.saturating_sub(visible);
        let max_top = track_units.saturating_sub(thumb_units);
        let top_units = if max_offset == 0 {
            0
        } else {
            (self.offset.min(max_offset) * max_top) / max_offset
        };
        Some(FineThumb {
            top_units,
            thumb_units,
            track_units,
        })
    }

    /// Map a click on the track (0..track) back to a scroll offset.
    pub fn offset_for_track_y(self, y: u16, track: u16) -> usize {
        let track = track.max(1) as usize;
        let total = self.total.max(1);
        let visible = self.visible.max(1).min(total);
        let max_offset = total.saturating_sub(visible);
        if track <= 1 || max_offset == 0 {
            return 0;
        }
        let y = (y as usize).min(track.saturating_sub(1));
        (y * max_offset) / (track.saturating_sub(1)).max(1)
    }
}

/// Track + thumb palette.
///
/// Eighth-block glyphs (`▄`/`▀`/…) composite as: filled = `thumb` fg, empty =
/// **track** (not pane `bg` / Reset). Otherwise partial thumbs wash into the
/// editor and stop reading as one scrollbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollColors {
    pub track: Color,
    pub thumb: Color,
    /// Pane behind the idle `│` rail (usually Reset).
    pub bg: Color,
}

impl ScrollColors {
    pub fn for_theme(light: bool) -> Self {
        if light {
            // Ghostty cream / light hosts: dark ink on shared pane bg.
            Self {
                track: Color::Rgb(176, 174, 155),
                thumb: Color::Rgb(90, 90, 80),
                bg: Color::Reset,
            }
        } else {
            Self {
                track: Color::DarkGray,
                thumb: Color::Rgb(210, 210, 210),
                bg: Color::Reset,
            }
        }
    }

    #[allow(dead_code)] // public API for MCP / external themed callers
    pub fn from_env() -> Self {
        // Luminance — never `theme.id == …` (ids move; that silently forced dark).
        Self::for_theme(crate::minimap_gfx::terminal_theme().is_light())
    }
}

/// Paint a 1-column scrollbar with sub-cell thumb edges.
#[allow(dead_code)] // thin wrapper; UI prefers `paint_with` + live minimap theme
pub fn paint(buf: &mut Buffer, area: Rect, metrics: ScrollMetrics) {
    paint_with(buf, area, metrics, ScrollColors::from_env());
}

/// Paint with an explicit palette (tests / themed callers).
pub fn paint_with(buf: &mut Buffer, area: Rect, metrics: ScrollMetrics, colors: ScrollColors) {
    if area.width == 0 || area.height < 2 {
        return;
    }
    let x = area.x.saturating_add(area.width.saturating_sub(1));
    let track_y = area.y;
    let track_h = area.height;
    // Idle rail sits on the pane (`bg`). Partial Unicode blocks use thumb fg
    // over **track** so empty eighths stay scrollbar-colored, not editor wash.
    let track_style = Style::default().fg(colors.track).bg(colors.bg);
    let thumb_style = Style::default().fg(colors.thumb).bg(colors.track);
    for row in 0..track_h {
        let cell = &mut buf[(x, track_y.saturating_add(row))];
        cell.set_symbol("│");
        cell.set_style(track_style);
    }
    let Some(fine) = metrics.fine_thumb(track_h) else {
        return;
    };
    let thumb_end = fine.top_units.saturating_add(fine.thumb_units);
    for row in 0..track_h as usize {
        let cell_lo = row.saturating_mul(SUBDIV);
        let cell_hi = cell_lo.saturating_add(SUBDIV);
        let cover_lo = fine.top_units.max(cell_lo);
        let cover_hi = thumb_end.min(cell_hi);
        if cover_hi <= cover_lo {
            continue;
        }
        let from = (cover_lo - cell_lo) as u8;
        let to = (cover_hi - cell_lo) as u8;
        let glyph = eighth_glyph(from, to);
        let cell = &mut buf[(x, track_y.saturating_add(row as u16))];
        cell.set_symbol(glyph);
        cell.set_style(thumb_style);
    }
}

/// Map a filled `[from, to)` eighth-range (0..=8 from the top of the cell) to a glyph.
fn eighth_glyph(from: u8, to: u8) -> &'static str {
    let from = from.min(8);
    let to = to.min(8).max(from);
    if from == 0 && to == 8 {
        return "█";
    }
    if from == 0 {
        // Filled from the top downward.
        return match to {
            1 => "▔",
            2 => "▀",
            3 => "▀",
            4 => "▀",
            5 => "▀",
            6 => "▀",
            7 => "▀",
            _ => "█",
        };
    }
    if to == 8 {
        // Filled from `from` down to the bottom.
        return match from {
            1 => "▇",
            2 => "▆",
            3 => "▅",
            4 => "▄",
            5 => "▃",
            6 => "▂",
            7 => "▁",
            _ => "█",
        };
    }
    // Mid-cell span: pick the densest block that covers the band.
    let span = to.saturating_sub(from);
    match span {
        0 => "│",
        1 => "─",
        2 => "▄",
        3 | 4 => "▄",
        _ => "█",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_scales_and_maps_clicks() {
        let m = ScrollMetrics {
            offset: 50,
            visible: 20,
            total: 100,
        };
        let (top, thumb) = m.thumb(40).expect("thumb");
        assert!(thumb >= 1);
        assert!(top < 40);
        let back = m.offset_for_track_y(top, 40);
        assert!(back <= 50 + 5, "back={back}");
    }

    #[test]
    fn fine_thumb_moves_inside_a_cell() {
        let track = 20u16;
        let mut prev = None;
        let mut moved_inside = false;
        for offset in 0..80 {
            let m = ScrollMetrics {
                offset,
                visible: 20,
                total: 100,
            };
            let fine = m.fine_thumb(track).expect("fine");
            if let Some(p) = prev {
                if fine.top_units != p && fine.top_units / SUBDIV == p / SUBDIV {
                    moved_inside = true;
                    break;
                }
            }
            prev = Some(fine.top_units);
        }
        assert!(
            moved_inside,
            "expected sub-cell top_units motion within one row"
        );
    }

    #[test]
    fn eighth_edges_use_partial_blocks() {
        assert_eq!(eighth_glyph(0, 8), "█");
        assert_eq!(eighth_glyph(4, 8), "▄");
        assert_eq!(eighth_glyph(0, 2), "▀");
        assert_eq!(eighth_glyph(7, 8), "▁");
    }

    #[test]
    fn eighth_glyphs_share_track_background() {
        // Sub-cell caps composite against the track — not pane Reset.
        let light = ScrollColors::for_theme(true);
        let dark = ScrollColors::for_theme(false);
        assert_eq!(light.track, Color::Rgb(176, 174, 155));
        assert_eq!(light.thumb, Color::Rgb(90, 90, 80));
        assert_eq!(light.bg, Color::Reset);
        assert_eq!(dark.bg, Color::Reset);
        assert_ne!(light.thumb, dark.thumb);

        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 12));
        let metrics = ScrollMetrics {
            offset: 3,
            visible: 4,
            total: 40,
        };
        paint_with(&mut buf, Rect::new(0, 0, 2, 12), metrics, light);
        let fine = metrics.fine_thumb(12).expect("fine");
        let top_row = (fine.top_units / SUBDIV) as u16;
        let cell = &buf[(1, top_row)];
        assert_eq!(
            cell.bg, light.track,
            "thumb eighth empty must be track, got {:?}",
            cell.bg
        );
        assert_eq!(cell.fg, light.thumb);
        // Idle rail rows keep pane bg behind the │.
        let track_row = if top_row > 0 { 0 } else { 11 };
        let track = &buf[(1, track_row)];
        if track.symbol() == "│" {
            assert_eq!(track.bg, light.bg);
            assert_eq!(track.fg, light.track);
        }
    }

    #[test]
    fn scrollbar_theme_follows_minimap_luminance_not_id() {
        assert!(crate::minimap_gfx::MinimapTheme::light().is_light());
        assert!(!crate::minimap_gfx::MinimapTheme::dark().is_light());
        // Regression: light theme id moved off `1`; id==1 checks forced dark scrollbars.
        assert_ne!(crate::minimap_gfx::MinimapTheme::light().id, 1);
        let light = ScrollColors::for_theme(crate::minimap_gfx::MinimapTheme::light().is_light());
        let dark = ScrollColors::for_theme(crate::minimap_gfx::MinimapTheme::dark().is_light());
        assert_eq!(light.track, Color::Rgb(176, 174, 155));
        assert_eq!(dark.track, Color::DarkGray);
    }

    /// Exact vectors from `bend/scroll/LAWS.bend`.
    #[test]
    fn bend_scroll_laws_conformance() {
        let in_range = ScrollMetrics {
            offset: 10,
            visible: 20,
            total: 100,
        };
        let max_offset = in_range.total.saturating_sub(in_range.visible);
        assert_eq!(in_range.offset.min(max_offset), 10);
        let over = ScrollMetrics {
            offset: 90,
            visible: 20,
            total: 100,
        };
        assert_eq!(over.offset.min(over.total.saturating_sub(over.visible)), 80);
        assert_eq!(10usize.saturating_sub(20), 0);
        let m = ScrollMetrics {
            offset: 0,
            visible: 20,
            total: 100,
        };
        assert_eq!(m.offset_for_track_y(0, 40), 0);
        assert_eq!(m.offset_for_track_y(39, 40), 80);
    }
}
