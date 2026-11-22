//! Scrolling tiles. Landscape columns cap at 80 once enough files are open.
//! Fewer files grow to fill the width. Portrait tiles use the full width.
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub const COLUMNS: u16 = 80;
pub const TREE_WIDTH: u16 = 28;
pub const AGENT_WIDTH: u16 = 32;
pub const PORTRAIT_SPLIT_HEIGHT: u16 = 24;

/// How many slots of `slot` columns fit. 80, 160, and 240 stay on the previous count.
/// The next column begins at 81, 161, and 241.
pub fn slot_capacity(available: u16, slot: u16) -> usize {
    let available = available.max(1) as usize;
    let slot = slot.max(1) as usize;
    available.div_ceil(slot)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

pub fn axis(width: u16, height: u16) -> Axis {
    axis_of(width, height, None)
}

/// A cell is about twice as tall as it is wide. The window is portrait when the
/// screen is taller than it is wide. Pixel size wins when the host reports it.
pub fn axis_of(width: u16, height: u16, pixels: Option<(u16, u16)>) -> Axis {
    if let Some((px_w, px_h)) = pixels {
        if px_w > 0 && px_h > 0 {
            return if px_h > px_w {
                Axis::Vertical
            } else {
                Axis::Horizontal
            };
        }
    }
    if height.saturating_mul(2) > width {
        Axis::Vertical
    } else {
        Axis::Horizontal
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Chrome {
    pub body: Rect,
    pub toolbar: Rect,
    pub editor: Rect,
    pub tree: Rect,
    pub content: Rect,
    pub agent: Rect,
    /// Dedicated Canvas for Browser / Phone / Android pixel previews.
    /// Sits to the right of the whole workbench (editor, chat, terminal).
    pub canvas: Rect,
    pub terminal: Rect,
    pub status: Option<Rect>,
}

#[derive(Clone, Copy, Debug)]
pub struct TerminalPane {
    pub open: bool,
    pub rows: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasPane {
    pub open: bool,
    pub width: Option<u16>,
}

pub fn chrome(
    area: Rect,
    palette: bool,
    sidebar: bool,
    chat: bool,
    tree_pref: u16,
    agent_pref: u16,
    terminal: TerminalPane,
) -> Chrome {
    chrome_with_canvas(
        area,
        palette,
        sidebar,
        chat,
        tree_pref,
        agent_pref,
        terminal,
        CanvasPane::default(),
    )
}

/// Layout: title / status span the full width. The Canvas is a dedicated right
/// column for device/web pixel previews. The workbench (files, editor, agent,
/// terminal) fills the left column so the terminal never sits under the Canvas.
pub fn chrome_with_canvas(
    area: Rect,
    palette: bool,
    sidebar: bool,
    chat: bool,
    tree_pref: u16,
    agent_pref: u16,
    terminal: TerminalPane,
    canvas: CanvasPane,
) -> Chrome {
    let rows = if palette {
        [
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ]
    } else {
        [
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(rows)
        .split(area);
    let split = chunks[1];

    let canvas_width = if !canvas.open || split.width < 64 {
        0
    } else {
        let max = split.width.saturating_sub(40);
        let default = (split.width / 3).clamp(32, 64).min(max.max(32));
        canvas.width.unwrap_or(default).clamp(28, max.max(28))
    };
    let work_width = split.width.saturating_sub(canvas_width);
    let work = Rect {
        x: split.x,
        y: split.y,
        width: work_width,
        height: split.height,
    };
    let canvas_rect = Rect {
        x: split.x.saturating_add(work_width),
        y: split.y,
        width: canvas_width,
        height: split.height,
    };

    let toolbar_height = work.height.min(2);
    let toolbar = Rect {
        x: work.x,
        y: work.y,
        width: work.width,
        height: toolbar_height,
    };
    let body = Rect {
        x: work.x,
        y: work.y.saturating_add(toolbar_height),
        width: work.width,
        height: work.height.saturating_sub(toolbar_height),
    };
    let tree_width = if !sidebar {
        3
    } else if tree_pref == TREE_WIDTH {
        if work.width >= TREE_WIDTH + 16 {
            TREE_WIDTH
        } else {
            3
        }
    } else {
        let max = body.width.saturating_sub(16);
        let wanted = tree_pref.clamp(12, max.max(12));
        if max >= 12 && body.width >= wanted.saturating_add(16) {
            wanted
        } else {
            3
        }
    };
    let panel = terminal_height(area.height, body.height, terminal.open, terminal.rows);
    let editor_height = body.height.saturating_sub(panel);
    let editor = Rect {
        x: body.x,
        y: body.y,
        width: body.width,
        height: editor_height,
    };
    let tree = Rect {
        x: editor.x,
        y: editor.y,
        width: tree_width.min(editor.width),
        height: editor.height,
    };
    let after_tree = editor.width.saturating_sub(tree.width);
    let room = after_tree >= AGENT_WIDTH + 96;
    let agent_width = if !room {
        0
    } else if !chat {
        3
    } else if agent_pref == AGENT_WIDTH {
        AGENT_WIDTH
    } else {
        let max = after_tree.saturating_sub(16);
        agent_pref.clamp(12, max.max(12)).min(max.max(12))
    };
    let agent = Rect {
        x: editor
            .x
            .saturating_add(editor.width.saturating_sub(agent_width)),
        y: editor.y,
        width: agent_width,
        height: editor.height,
    };
    let content = Rect {
        x: editor.x.saturating_add(tree.width),
        y: editor.y,
        width: after_tree.saturating_sub(agent_width),
        height: editor.height,
    };
    let terminal = Rect {
        x: body.x,
        y: body.y.saturating_add(editor_height),
        width: body.width,
        height: panel,
    };
    Chrome {
        body,
        toolbar,
        editor,
        tree,
        content,
        agent,
        canvas: canvas_rect,
        terminal,
        status: (!palette).then_some(chunks[2]),
    }
}

/// Back-compat alias used by older call sites.
pub fn chrome_with_runtime(
    area: Rect,
    palette: bool,
    sidebar: bool,
    chat: bool,
    tree_pref: u16,
    agent_pref: u16,
    terminal: TerminalPane,
    runtime: CanvasPane,
) -> Chrome {
    chrome_with_canvas(
        area, palette, sidebar, chat, tree_pref, agent_pref, terminal, runtime,
    )
}

/// The open terminal stays on the bottom. The default is a quarter of the screen.
/// A drag stores a preferred height and still leaves the editor a few rows.
fn terminal_height(screen: u16, body: u16, open: bool, preferred: Option<u16>) -> u16 {
    if body < 8 {
        return 0;
    }
    if !open {
        return 1;
    }
    let cap = body.saturating_sub(4).max(1);
    let default = (screen / 4).min(cap).max(3);
    preferred.unwrap_or(default).clamp(1, cap)
}

const MAX_TILES: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub viewport: Rect,
    pub tile_width: u16,
    pub tile_height: u16,
    pub stride: u16,
    pub axis: Axis,
    spans: [u16; MAX_TILES],
    span_len: u8,
}

impl Window {
    pub fn along(&self, index: usize) -> u16 {
        if index < self.span_len as usize {
            self.spans[index]
        } else {
            self.stride
        }
    }

    pub fn origin(&self, index: usize) -> i32 {
        let known = index.min(self.span_len as usize);
        let sum: i32 = self.spans[..known].iter().map(|span| *span as i32).sum();
        let extra = index.saturating_sub(self.span_len as usize) as i32;
        sum + extra * self.stride as i32
    }

    pub fn tile_box(&self, index: usize) -> (u16, u16) {
        match self.axis {
            Axis::Horizontal => (self.along(index), self.tile_height),
            Axis::Vertical => (self.tile_width, self.along(index)),
        }
    }
}

pub fn window(content: Rect, orientation: Axis, show_many: bool, buffers: usize) -> Window {
    window_spans(content, orientation, show_many, buffers, None)
}

/// `custom` is the along-axis size of each buffer. Fullscreen ignores it.
/// `tile_width` stays the automatic equal size so an undragged layout is unchanged.
pub fn window_spans(
    content: Rect,
    orientation: Axis,
    show_many: bool,
    buffers: usize,
    custom: Option<&[u16]>,
) -> Window {
    let buffers = buffers.max(1);
    let width = content.width.max(1);
    let height = content.height.max(1);
    let (tile_width, tile_height, stride) = match orientation {
        Axis::Horizontal => {
            let shown = if show_many {
                buffers.min(slot_capacity(width, COLUMNS)).max(1)
            } else {
                1
            };
            let tile_width = (width / shown as u16).max(1);
            (tile_width, height, tile_width)
        }
        Axis::Vertical => {
            let shown = if show_many {
                buffers
                    .min(slot_capacity(height, PORTRAIT_SPLIT_HEIGHT))
                    .max(1)
            } else {
                1
            };
            let tile_height = (height / shown as u16).max(1);
            (width, tile_height, tile_height)
        }
    };
    let along = match orientation {
        Axis::Horizontal => tile_width,
        Axis::Vertical => tile_height,
    };
    let mut spans = [along; MAX_TILES];
    // One buffer always fills the view. Leftover drag sizes from a prior split
    // must not leave a half-width pane beside empty space.
    if show_many && (2..=MAX_TILES).contains(&buffers) {
        if let Some(custom) = custom {
            if custom.len() == buffers && custom.iter().all(|span| *span >= 1) {
                for (slot, span) in spans.iter_mut().zip(custom) {
                    *slot = *span;
                }
            }
        }
    }
    Window {
        viewport: Rect {
            x: content.x,
            y: content.y,
            width,
            height,
        },
        tile_width,
        tile_height,
        stride,
        axis: orientation,
        spans,
        span_len: buffers.min(MAX_TILES) as u8,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slice {
    pub screen_x: u16,
    pub screen_y: u16,
    pub source_x: u16,
    pub source_y: u16,
    pub width: u16,
    pub height: u16,
}

pub fn tile_slice(window: Window, index: usize, view: f32) -> Option<Slice> {
    if window.viewport.width == 0 || window.viewport.height == 0 {
        return None;
    }
    let view_i = view.round() as i32;
    let origin = window.origin(index) - view_i;
    let (tile_w, tile_h) = window.tile_box(index);
    let (tile_x, tile_y) = match window.axis {
        Axis::Horizontal => {
            let x = window.viewport.x as i32 + origin;
            (x, window.viewport.y as i32)
        }
        Axis::Vertical => {
            let y = window.viewport.y as i32 + origin;
            let spare = window.viewport.width as i32 - window.tile_width as i32;
            let x = window.viewport.x as i32 + spare.max(0) / 2;
            (x, y)
        }
    };
    intersect(tile_x, tile_y, tile_w, tile_h, window.viewport)
}

fn intersect(tile_x: i32, tile_y: i32, tile_w: u16, tile_h: u16, view: Rect) -> Option<Slice> {
    let x0 = tile_x.max(view.x as i32);
    let y0 = tile_y.max(view.y as i32);
    let x1 = (tile_x + tile_w as i32).min(view.x as i32 + view.width as i32);
    let y1 = (tile_y + tile_h as i32).min(view.y as i32 + view.height as i32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Slice {
        screen_x: x0 as u16,
        screen_y: y0 as u16,
        source_x: (x0 - tile_x) as u16,
        source_y: (y0 - tile_y) as u16,
        width: (x1 - x0) as u16,
        height: (y1 - y0) as u16,
    })
}

pub fn hit_tile(window: Window, column: u16, row: u16, view: f32, count: usize) -> Option<usize> {
    (0..count).find(|&index| {
        tile_slice(window, index, view).is_some_and(|slice| {
            column >= slice.screen_x
                && row >= slice.screen_y
                && column < slice.screen_x.saturating_add(slice.width)
                && row < slice.screen_y.saturating_add(slice.height)
        })
    })
}

pub fn cell_in_tile(
    window: Window,
    index: usize,
    view: f32,
    column: u16,
    row: u16,
) -> Option<(usize, usize)> {
    let slice = tile_slice(window, index, view)?;
    if column < slice.screen_x || row < slice.screen_y {
        return None;
    }
    let local_x = slice.source_x + (column - slice.screen_x);
    let local_y = slice.source_y + (row - slice.screen_y);
    let (tile_w, tile_h) = window.tile_box(index);
    if local_x == 0 || local_y == 0 || local_x + 1 >= tile_w || local_y + 1 >= tile_h {
        return None;
    }
    Some(((local_x - 1) as usize, (local_y - 1) as usize))
}

#[cfg(test)]
pub fn text_origin(window: Window, index: usize, view: f32) -> Option<(u16, u16)> {
    let slice = tile_slice(window, index, view)?;
    let inner_x = 1u16;
    let inner_y = 1u16;
    if inner_x < slice.source_x || inner_y < slice.source_y {
        return None;
    }
    if inner_x >= slice.source_x.saturating_add(slice.width)
        || inner_y >= slice.source_y.saturating_add(slice.height)
    {
        return None;
    }
    Some((
        slice.screen_x + (inner_x - slice.source_x),
        slice.screen_y + (inner_y - slice.source_y),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_picks_the_axis() {
        assert_eq!(axis(120, 40), Axis::Horizontal);
        assert_eq!(axis(160, 40), Axis::Horizontal);
        assert_eq!(axis(80, 40), Axis::Horizontal);
        assert_eq!(axis(80, 50), Axis::Vertical);
        assert_eq!(axis(40, 120), Axis::Vertical);
        assert_eq!(axis(80, 80), Axis::Vertical);
        assert_eq!(axis_of(200, 40, Some((800, 900))), Axis::Vertical);
        assert_eq!(axis_of(40, 80, Some((900, 800))), Axis::Horizontal);
    }

    #[test]
    fn landscape_columns_follow_width_and_open_files() {
        assert_eq!(slot_capacity(80, 80), 1);
        assert_eq!(slot_capacity(81, 80), 2);
        assert_eq!(slot_capacity(160, 80), 2);
        assert_eq!(slot_capacity(161, 80), 3);
        assert_eq!(slot_capacity(240, 80), 3);
        assert_eq!(slot_capacity(241, 80), 4);

        let pair = Rect::new(0, 0, 160, 30);
        assert_eq!(window(pair, Axis::Horizontal, true, 1).tile_width, 160);
        assert_eq!(window(pair, Axis::Horizontal, true, 2).tile_width, 80);
        assert_eq!(window(pair, Axis::Horizontal, true, 4).tile_width, 80);

        let wide = Rect::new(0, 0, 200, 30);
        assert_eq!(window(wide, Axis::Horizontal, true, 2).tile_width, 100);
        assert_eq!(window(wide, Axis::Horizontal, true, 3).tile_width, 66);
        assert_eq!(window(wide, Axis::Horizontal, false, 5).tile_width, 200);

        let just_over = Rect::new(0, 0, 81, 20);
        assert_eq!(window(just_over, Axis::Horizontal, true, 2).tile_width, 40);
        let third = Rect::new(0, 0, 161, 20);
        assert_eq!(window(third, Axis::Horizontal, true, 2).tile_width, 80);
        assert_eq!(window(third, Axis::Horizontal, true, 3).tile_width, 53);
    }

    #[test]
    fn portrait_stacks_tiles_at_full_width() {
        let content = Rect::new(0, 0, 130, 90);
        let window = window(content, Axis::Vertical, true, 3);
        assert_eq!(window.axis, Axis::Vertical);
        assert_eq!(window.tile_width, 130);
        assert!(window.tile_width > COLUMNS);
        let first = tile_slice(window, 0, 0.0).unwrap();
        let second = tile_slice(window, 1, 0.0).unwrap();
        assert!(second.screen_y > first.screen_y);
        assert_eq!(second.screen_x, first.screen_x);
    }

    #[test]
    fn custom_spans_move_the_seam_and_fullscreen_ignores_them() {
        let content = Rect::new(0, 0, 160, 30);
        let even = window(content, Axis::Horizontal, true, 2);
        assert_eq!(even.along(0), 80);
        assert_eq!(even.origin(1), 80);
        let custom = window_spans(content, Axis::Horizontal, true, 2, Some(&[100, 60]));
        assert_eq!(custom.tile_width, 80);
        assert_eq!(custom.along(0), 100);
        assert_eq!(custom.along(1), 60);
        assert_eq!(custom.origin(1), 100);
        let first = tile_slice(custom, 0, 0.0).unwrap();
        let second = tile_slice(custom, 1, 0.0).unwrap();
        assert_eq!(first.width, 100);
        assert_eq!(second.screen_x, first.screen_x + 100);
        let ignored = window_spans(content, Axis::Horizontal, true, 2, Some(&[100]));
        assert_eq!(ignored.along(0), 80);
        let full = window_spans(content, Axis::Horizontal, false, 2, Some(&[100, 60]));
        assert_eq!(full.tile_width, 160);
        assert_eq!(full.along(0), 160);
        // A lone buffer ignores leftover drag spans and fills the content.
        let alone = window_spans(content, Axis::Horizontal, true, 1, Some(&[100]));
        assert_eq!(alone.tile_width, 160);
        assert_eq!(alone.along(0), 160);

        let stacked = Rect::new(0, 0, 40, 80);
        let rows = window_spans(stacked, Axis::Vertical, true, 2, Some(&[50, 30]));
        let top = tile_slice(rows, 0, 0.0).unwrap();
        let bottom = tile_slice(rows, 1, 0.0).unwrap();
        assert_eq!(top.height, 50);
        assert_eq!(bottom.screen_y, top.screen_y + 50);
    }

    #[test]
    fn open_terminal_is_the_bottom_quarter() {
        let area = Rect::new(0, 0, 80, 24);
        let open = chrome(
            area,
            false,
            true,
            true,
            TREE_WIDTH,
            AGENT_WIDTH,
            TerminalPane {
                open: true,
                rows: None,
            },
        );
        assert_eq!(open.terminal.height, 6);
        assert_eq!(open.terminal.y + open.terminal.height + 1, area.height);
        assert!(open.editor.y < open.terminal.y);
        assert_eq!(open.editor.height + open.terminal.height, open.body.height);
        let closed = chrome(
            area,
            false,
            true,
            true,
            TREE_WIDTH,
            AGENT_WIDTH,
            TerminalPane {
                open: false,
                rows: None,
            },
        );
        assert_eq!(closed.terminal.height, 1);
        assert!(closed.editor.height > open.editor.height);

        let tall = Rect::new(0, 0, 40, 90);
        let stacked = chrome(
            tall,
            false,
            true,
            true,
            TREE_WIDTH,
            AGENT_WIDTH,
            TerminalPane {
                open: true,
                rows: Some(10),
            },
        );
        assert_eq!(axis(40, 90), Axis::Vertical);
        assert_eq!(stacked.terminal.height, 10);
        assert_eq!(stacked.terminal.x, tall.x);
        assert_eq!(stacked.terminal.width, tall.width);
        assert!(stacked.terminal.y > stacked.editor.y);
    }
}
