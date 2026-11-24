//! Resize-seam hit testing and macOS-style split-handle pills.
//!
//! Hover highlights only the active edge (not the whole panel border), with a
//! centered pill at the midpoint of that edge so top/bottom/left/right grabs
//! are obvious.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// Preferred pill size along the edge (cells).
pub const PILL_LEN: u16 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeamAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeamVisual {
    pub axis: SeamAxis,
    /// Full divider strip (width=1 or height=1).
    pub line: Rect,
    /// Centered handle painted over the line.
    pub pill: Rect,
}

impl SeamVisual {
    pub fn horizontal(x: u16, y: u16, width: u16) -> Self {
        let width = width.max(1);
        let pill_w = PILL_LEN.min(width).max(1);
        let pill_x = x.saturating_add(width.saturating_sub(pill_w) / 2);
        Self {
            axis: SeamAxis::Horizontal,
            line: Rect {
                x,
                y,
                width,
                height: 1,
            },
            pill: Rect {
                x: pill_x,
                y,
                width: pill_w,
                height: 1,
            },
        }
    }

    pub fn vertical(x: u16, y: u16, height: u16) -> Self {
        let height = height.max(1);
        let pill_h = PILL_LEN.min(height).max(1);
        let pill_y = y.saturating_add(height.saturating_sub(pill_h) / 2);
        Self {
            axis: SeamAxis::Vertical,
            line: Rect {
                x,
                y,
                width: 1,
                height,
            },
            pill: Rect {
                x,
                y: pill_y,
                width: 1,
                height: pill_h,
            },
        }
    }

    /// Slightly enlarged pill hit so the midpoint is easier to grab.
    pub fn contains_grab(&self, column: u16, row: u16) -> bool {
        match self.axis {
            SeamAxis::Horizontal => {
                let pad = 1u16;
                let x0 = self.pill.x.saturating_sub(pad);
                let x1 = self.pill.x.saturating_add(self.pill.width).saturating_add(pad);
                row == self.pill.y && column >= x0 && column < x1
            }
            SeamAxis::Vertical => {
                let pad = 1u16;
                let y0 = self.pill.y.saturating_sub(pad);
                let y1 = self.pill.y.saturating_add(self.pill.height).saturating_add(pad);
                column == self.pill.x && row >= y0 && row < y1
            }
        }
    }
}

/// Right-edge seam of a left-docked pane (file tree).
pub fn left_pane_seam(pane: Rect) -> Option<SeamVisual> {
    if pane.width == 0 || pane.height == 0 {
        return None;
    }
    let x = pane.x.saturating_add(pane.width).saturating_sub(1);
    Some(SeamVisual::vertical(x, pane.y, pane.height))
}

/// Left-edge seam of a right-docked pane (agent / canvas).
pub fn right_pane_seam(pane: Rect) -> Option<SeamVisual> {
    if pane.width == 0 || pane.height == 0 {
        return None;
    }
    Some(SeamVisual::vertical(pane.x, pane.y, pane.height))
}

/// Top-edge seam of a bottom-docked pane (terminal).
pub fn bottom_pane_seam(pane: Rect) -> Option<SeamVisual> {
    if pane.width == 0 || pane.height == 0 {
        return None;
    }
    Some(SeamVisual::horizontal(pane.x, pane.y, pane.width))
}

/// Hit-test a left-docked pane's right edge (exact border ± adjacent cell, or pill).
pub fn hit_left_pane(pane: Rect, column: u16, row: u16) -> bool {
    let Some(seam) = left_pane_seam(pane) else {
        return false;
    };
    if row < pane.y || row >= pane.y.saturating_add(pane.height) {
        return false;
    }
    if pane.width <= 4 {
        return column >= pane.x && column < pane.x.saturating_add(pane.width);
    }
    let edge = pane.x.saturating_add(pane.width).saturating_sub(1);
    column == edge || column == edge.saturating_add(1) || seam.contains_grab(column, row)
}

/// Hit-test a right-docked pane's left edge.
///
/// Expand **into** the pane (`pane.x` / `pane.x+1`), never one cell left into
/// the editor — that column is the text buffer's right-edge resize handle.
pub fn hit_right_pane(pane: Rect, column: u16, row: u16) -> bool {
    let Some(seam) = right_pane_seam(pane) else {
        return false;
    };
    if row < pane.y || row >= pane.y.saturating_add(pane.height) {
        return false;
    }
    if pane.width <= 4 {
        return column >= pane.x && column < pane.x.saturating_add(pane.width);
    }
    column == pane.x
        || column == pane.x.saturating_add(1)
        || seam.contains_grab(column, row)
}

/// Hit-test a bottom pane's top edge: title/border row, one row above, or center pill.
pub fn hit_bottom_pane(pane: Rect, column: u16, row: u16, title_blocked: bool) -> bool {
    let Some(seam) = bottom_pane_seam(pane) else {
        return false;
    };
    if column < pane.x || column >= pane.x.saturating_add(pane.width) {
        return false;
    }
    if seam.contains_grab(column, row) && !title_blocked {
        return true;
    }
    // Row immediately above the pane (editor side of the split).
    if row + 1 == pane.y {
        return true;
    }
    // Top border / title row — caller can exclude control buttons via title_blocked.
    if pane.height > 1 && row == pane.y && !title_blocked {
        return true;
    }
    false
}

/// Paint edge tint + centered split pill. Only the hovered edge lights up.
pub fn paint(buf: &mut Buffer, area: Rect, seam: SeamVisual, active: bool) {
    if !active || seam.line.width == 0 || seam.line.height == 0 {
        return;
    }
    let line_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let pill_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    match seam.axis {
        SeamAxis::Horizontal => {
            for dx in 0..seam.line.width {
                let x = seam.line.x.saturating_add(dx);
                let y = seam.line.y;
                if x < area.right() && y < area.bottom() {
                    let cell = &mut buf[(x, y)];
                    cell.set_fg(Color::Yellow);
                    // Keep existing glyph; only tint until pill overwrites.
                    let _ = line_style;
                }
            }
            for dx in 0..seam.pill.width {
                let x = seam.pill.x.saturating_add(dx);
                let y = seam.pill.y;
                if x >= area.right() || y >= area.bottom() {
                    continue;
                }
                let glyph = horizontal_pill_glyph(dx, seam.pill.width);
                let cell = &mut buf[(x, y)];
                cell.set_symbol(glyph);
                cell.set_style(pill_style);
            }
        }
        SeamAxis::Vertical => {
            for dy in 0..seam.line.height {
                let x = seam.line.x;
                let y = seam.line.y.saturating_add(dy);
                if x < area.right() && y < area.bottom() {
                    buf[(x, y)].set_fg(Color::Yellow);
                }
            }
            for dy in 0..seam.pill.height {
                let x = seam.pill.x;
                let y = seam.pill.y.saturating_add(dy);
                if x >= area.right() || y >= area.bottom() {
                    continue;
                }
                let glyph = vertical_pill_glyph(dy, seam.pill.height);
                let cell = &mut buf[(x, y)];
                cell.set_symbol(glyph);
                cell.set_style(pill_style);
            }
        }
    }
}

fn horizontal_pill_glyph(index: u16, width: u16) -> &'static str {
    if width <= 1 {
        return "━";
    }
    if index == 0 {
        "╸"
    } else if index + 1 >= width {
        "╺"
    } else {
        "━"
    }
}

fn vertical_pill_glyph(index: u16, height: u16) -> &'static str {
    if height <= 1 {
        return "┃";
    }
    if index == 0 {
        "╻"
    } else if index + 1 >= height {
        "╹"
    } else {
        "┃"
    }
}

/// Idle (non-hover) pills: dim mid-edge handles so splits stay discoverable.
pub fn paint_idle(buf: &mut Buffer, area: Rect, seam: SeamVisual) {
    let style = Style::default().fg(Color::DarkGray);
    match seam.axis {
        SeamAxis::Horizontal => {
            for dx in 0..seam.pill.width {
                let x = seam.pill.x.saturating_add(dx);
                let y = seam.pill.y;
                if x >= area.right() || y >= area.bottom() {
                    continue;
                }
                let cell = &mut buf[(x, y)];
                cell.set_symbol(horizontal_pill_glyph(dx, seam.pill.width));
                cell.set_style(style);
            }
        }
        SeamAxis::Vertical => {
            for dy in 0..seam.pill.height {
                let x = seam.pill.x;
                let y = seam.pill.y.saturating_add(dy);
                if x >= area.right() || y >= area.bottom() {
                    continue;
                }
                let cell = &mut buf[(x, y)];
                cell.set_symbol(vertical_pill_glyph(dy, seam.pill.height));
                cell.set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_pill_centers_on_edge() {
        let seam = SeamVisual::horizontal(10, 5, 20);
        assert_eq!(seam.line.y, 5);
        assert_eq!(seam.line.width, 20);
        assert_eq!(seam.pill.width, PILL_LEN);
        assert_eq!(seam.pill.x, 10 + (20 - PILL_LEN) / 2);
        assert!(seam.contains_grab(seam.pill.x + 2, 5));
        assert!(!seam.contains_grab(seam.pill.x + 2, 6));
    }

    #[test]
    fn vertical_pill_centers_on_edge() {
        let seam = SeamVisual::vertical(3, 0, 30);
        assert_eq!(seam.pill.height, PILL_LEN);
        assert_eq!(seam.pill.y, (30 - PILL_LEN) / 2);
        assert!(seam.contains_grab(3, seam.pill.y + 2));
    }

    #[test]
    fn bottom_pane_hits_title_and_row_above() {
        let pane = Rect {
            x: 0,
            y: 10,
            width: 40,
            height: 8,
        };
        assert!(hit_bottom_pane(pane, 20, 9, false)); // above
        assert!(hit_bottom_pane(pane, 20, 10, false)); // title
        assert!(!hit_bottom_pane(pane, 20, 10, true)); // blocked control
        assert!(!hit_bottom_pane(pane, 20, 12, false)); // body
    }

    #[test]
    fn left_pane_hits_right_border_only() {
        let pane = Rect {
            x: 0,
            y: 3,
            width: 20,
            height: 20,
        };
        assert!(hit_left_pane(pane, 19, 10));
        assert!(hit_left_pane(pane, 20, 10));
        assert!(!hit_left_pane(pane, 5, 10));
        assert!(!hit_left_pane(pane, 19, 2)); // above pane
    }

    #[test]
    fn right_pane_does_not_steal_cell_left_of_edge() {
        let pane = Rect {
            x: 40,
            y: 3,
            width: 20,
            height: 20,
        };
        assert!(hit_right_pane(pane, 40, 10)); // edge
        assert!(hit_right_pane(pane, 41, 10)); // into pane
        assert!(!hit_right_pane(pane, 39, 10)); // editor / buffer right edge
    }
}
