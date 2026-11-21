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
    if width >= height {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Chrome {
    pub body: Rect,
    pub tree: Rect,
    pub content: Rect,
    pub agent: Rect,
    pub status: Option<Rect>,
}

pub fn chrome(area: Rect, palette: bool, sidebar: bool, chat: bool) -> Chrome {
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
    let body = chunks[1];
    let tree_width = if !sidebar {
        3
    } else if area.width >= TREE_WIDTH + 16 {
        TREE_WIDTH
    } else {
        3
    };
    let tree = Rect {
        x: body.x,
        y: body.y,
        width: tree_width.min(body.width),
        height: body.height,
    };
    let after_tree = body.width.saturating_sub(tree.width);
    let room = after_tree >= AGENT_WIDTH + 96;
    let agent_width = if !room {
        0
    } else if chat {
        AGENT_WIDTH
    } else {
        3
    };
    let agent = Rect {
        x: body
            .x
            .saturating_add(body.width.saturating_sub(agent_width)),
        y: body.y,
        width: agent_width,
        height: body.height,
    };
    let content = Rect {
        x: body.x.saturating_add(tree.width),
        y: body.y,
        width: after_tree.saturating_sub(agent_width),
        height: body.height,
    };
    Chrome {
        body,
        tree,
        content,
        agent,
        status: (!palette).then_some(chunks[2]),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub viewport: Rect,
    pub tile_width: u16,
    pub tile_height: u16,
    pub stride: u16,
    pub axis: Axis,
}

pub fn window(content: Rect, orientation: Axis, show_many: bool, buffers: usize) -> Window {
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
    let stride = window.stride as i32;
    let origin = index as i32 * stride - view_i;
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
    intersect(
        tile_x,
        tile_y,
        window.tile_width,
        window.tile_height,
        window.viewport,
    )
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
    if local_x == 0
        || local_y == 0
        || local_x + 1 >= window.tile_width
        || local_y + 1 >= window.tile_height
    {
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
        assert_eq!(axis(40, 120), Axis::Vertical);
        assert_eq!(axis(80, 80), Axis::Horizontal);
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
}
