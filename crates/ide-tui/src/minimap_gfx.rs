//! Kitty graphics code minimap — VS Code–style proportional preview.
//!
//! Shown only when the buffer needs vertical scroll. On Kitty/Ghostty the image
//! strip **replaces** the text scrollbar. Paint is pressure-aware and caches the
//! base RGB so scroll-only frames patch the viewport band instead of resampling.

use crate::graphics::GraphicsBackend;
use ratatui::layout::Rect;

/// Stable Kitty image id for the focused editor minimap (away from Canvas ids).
/// Keep below 256 so unicode-placeholder FG uses indexed color (Ghostty-friendly).
pub const EDITOR_MINIMAP_IMAGE_ID: u32 = 41;

/// Default device pixels per source line (readable preview height).
/// Locked to Bend `bend/minimap` laws (scale 1 → 8 px/line).
pub const PX_PER_LINE_BASE: u32 = 8;

/// Horizontal pixels per source character (5×7 bitmap + 1px gap).
pub const PX_PER_CHAR_BASE: u32 = 6;

/// Bitmap glyph width (bits used in each font row).
const GLYPH_W: u32 = 5;
/// Bitmap glyph height (fits in 8px line with 1px pad).
const GLYPH_H: u32 = 7;

/// Cache key: re-upload when any of these change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimapPaintKey {
    pub revision: u64,
    pub line_count: usize,
    pub scroll: usize,
    pub visible: usize,
    pub start_line: usize,
    pub cols: u16,
    pub rows: u16,
    pub px_w: u32,
    pub px_h: u32,
    pub scale: u32,
    /// Fingerprint of the live terminal palette (OSC 10/11/4).
    pub theme: u64,
}

/// Content identity without scroll/viewport (base RGB cache).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimapContentKey {
    pub revision: u64,
    pub start_line: usize,
    pub shown_lines: usize,
    pub px_w: u32,
    pub px_h: u32,
    pub scale: u32,
    pub theme: u64,
}

/// Proportional minimap layout (sliding window + viewport slider).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimapLayout {
    pub start_line: usize,
    pub shown_lines: usize,
    pub px_per_line: u32,
    pub px_per_char: u32,
    pub slider_top_px: u32,
    pub slider_height_px: u32,
}

/// Terminal-derived palette for the RGB strip (mirrors buffer ANSI token colors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimapTheme {
    pub fingerprint: u64,
    pub bg: (u8, u8, u8),
    pub fg: (u8, u8, u8),
    pub digit: (u8, u8, u8),
    pub string: (u8, u8, u8),
    pub punct: (u8, u8, u8),
    pub comment: (u8, u8, u8),
    pub ident: (u8, u8, u8),
    pub keyword: (u8, u8, u8),
    pub function: (u8, u8, u8),
    pub markup: (u8, u8, u8),
    /// Added to RGB inside the viewport band (can be slightly negative via saturating).
    pub view_delta: i16,
}

impl MinimapTheme {
    pub fn light() -> Self {
        Self {
            fingerprint: 1,
            bg: (250, 245, 230),
            fg: (70, 80, 55),
            digit: (90, 120, 70),
            string: (150, 100, 55),
            punct: (90, 90, 80),
            comment: (110, 130, 80),
            ident: (55, 95, 120),
            keyword: (150, 60, 120),
            function: (40, 80, 160),
            markup: (120, 120, 110),
            view_delta: -18,
        }
    }

    pub fn dark() -> Self {
        Self {
            fingerprint: 2,
            bg: (0x1e, 0x1e, 0x24),
            fg: (0x80, 0x80, 0x88),
            digit: (0xb5, 0xce, 0xa8),
            string: (0xce, 0x91, 0x78),
            punct: (0xd4, 0xd4, 0xd4),
            comment: (0x6a, 0x99, 0x55),
            ident: (0x9c, 0xdc, 0xfe),
            keyword: (0xc5, 0x86, 0xc0),
            function: (0x56, 0x9c, 0xd6),
            markup: (0x6a, 0x6a, 0x6a),
            view_delta: 28,
        }
    }

    pub fn is_light(self) -> bool {
        // Relative luminance of bg vs mid-gray.
        let y = |c: (u8, u8, u8)| {
            0.2126 * f32::from(c.0) + 0.7152 * f32::from(c.1) + 0.0722 * f32::from(c.2)
        };
        y(self.bg) > y(self.fg) || y(self.bg) > 128.0
    }
}

/// Env-only heuristic (no OSC). Prefer [`crate::term_palette::probe`] at runtime.
pub fn terminal_theme_from_env() -> MinimapTheme {
    let program = std::env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let ghostty = program.contains("ghostty")
        || std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some();
    if let Ok(v) = std::env::var("COLORFGBG") {
        if let Some(bg) = v.split(';').nth(1).and_then(|s| s.trim().parse::<i32>().ok()) {
            // Classic dark indexed backgrounds.
            if (0..=7).contains(&bg) {
                if ghostty {
                    // Ambiguous on Ghostty (often wrong); match the cream default.
                    return MinimapTheme::light();
                }
                return MinimapTheme::dark();
            }
            return MinimapTheme::light();
        }
    }
    if ghostty || program.contains("apple_terminal") {
        return MinimapTheme::light();
    }
    MinimapTheme::dark()
}

/// Env heuristic (safe in tests). Runtime paint uses [`crate::term_palette::probe`].
pub fn terminal_theme() -> MinimapTheme {
    terminal_theme_from_env()
}

/// True when the document is taller than the editor viewport.
pub fn needs_scroll(line_count: usize, visible: usize) -> bool {
    line_count > visible.max(1)
}

/// `POSIT_EDIT_MINIMAP_FORCE=1` keeps the Kitty strip up even under Urgent pressure
/// (dogfood / screenshot verification). Production still hides on jetsam-class.
pub fn force_minimap() -> bool {
    matches!(
        std::env::var("POSIT_EDIT_MINIMAP_FORCE").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

/// Editor right-edge chrome: `(minimap_cols, scrollbar_cols)`.
///
/// - No scroll needed → `(0, 0)` (hide both).
/// - Kitty minimap → replaces text scrollbar: `(N, 0)`.
/// - Glyph fallback → thin minimap + text scrollbar: `(2, 1)`.
/// - Callers pass `kitty=false` while scrolling / sliding buffers so the
///   precision text scrollbar takes the edge until motion settles.
pub fn edge_chrome(
    tile_w: u16,
    kitty: bool,
    pressure_critical: bool,
    pressure_warning: bool,
    line_count: usize,
    visible: usize,
) -> (u16, u16) {
    if !needs_scroll(line_count, visible) || tile_w < 18 {
        return (0, 0);
    }
    // Jetsam-class pressure: keep a text scrollbar, drop the Kitty strip
    // (unless dogfood force is on).
    if pressure_critical && !force_minimap() {
        return (0, 1);
    }
    let cols = minimap_cols(tile_w, kitty, pressure_warning);
    if cols == 0 {
        return (0, 1);
    }
    if kitty {
        (cols, 0)
    } else {
        (cols, 1)
    }
}

/// Cell width of the editor minimap column.
pub fn minimap_cols(tile_w: u16, kitty: bool, pressure_warning: bool) -> u16 {
    if tile_w < 18 {
        return 0;
    }
    if !kitty {
        return 2;
    }
    // Wider + readable; shrink under Warning pressure.
    if pressure_warning {
        if tile_w >= 40 {
            14
        } else if tile_w >= 28 {
            12
        } else {
            10
        }
    } else if tile_w >= 56 {
        24
    } else if tile_w >= 44 {
        20
    } else if tile_w >= 32 {
        16
    } else {
        12
    }
}

pub fn host_is_kitty() -> bool {
    if !crate::graphics::kitty_allowed() {
        return false;
    }
    crate::graphics::GraphicsArbiter::probe_env(
        std::env::var("TERM").ok().as_deref(),
        std::env::var("TERM_PROGRAM").ok().as_deref(),
    ) == GraphicsBackend::Kitty
}

/// Map a local tile rect into screen space given a visible [`layout::Slice`].
pub fn screen_rect_from_tile_local(slice: crate::layout::Slice, local: Rect) -> Option<Rect> {
    let tile_sx = slice.screen_x as i32 - slice.source_x as i32;
    let tile_sy = slice.screen_y as i32 - slice.source_y as i32;
    let x0 = (tile_sx + local.x as i32).max(slice.screen_x as i32);
    let y0 = (tile_sy + local.y as i32).max(slice.screen_y as i32);
    let x1 = (tile_sx + local.x as i32 + local.width as i32)
        .min(slice.screen_x as i32 + slice.width as i32);
    let y1 = (tile_sy + local.y as i32 + local.height as i32)
        .min(slice.screen_y as i32 + slice.height as i32);
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return None;
    }
    Some(Rect {
        x: x0 as u16,
        y: y0 as u16,
        width: (x1 - x0) as u16,
        height: (y1 - y0) as u16,
    })
}

/// Scale factor: 1 normal, 1 under warning (dims handle width), still readable.
pub fn paint_scale(pressure_warning: bool) -> u32 {
    if pressure_warning {
        1
    } else {
        1
    }
}

pub fn layout_proportional(
    line_count: usize,
    scroll: usize,
    visible: usize,
    img_h: u32,
    scale: u32,
) -> MinimapLayout {
    let line_count = line_count.max(1);
    let visible = visible.max(1);
    let px_per_line = PX_PER_LINE_BASE.saturating_mul(scale.max(1)).max(1);
    let px_per_char = PX_PER_CHAR_BASE.saturating_mul(scale.max(1)).max(1);
    let shown_lines = ((img_h / px_per_line) as usize).max(1);
    let start_line = if line_count <= shown_lines {
        0
    } else {
        let max_start = line_count.saturating_sub(shown_lines);
        let slack = shown_lines.saturating_sub(visible);
        let ideal = scroll.saturating_sub(slack / 2);
        ideal.min(max_start)
    };
    let slider_top_line = scroll.saturating_sub(start_line).min(shown_lines.saturating_sub(1));
    let slider_lines = visible.min(shown_lines);
    let slider_top_px = (slider_top_line as u32).saturating_mul(px_per_line);
    let slider_height_px = (slider_lines as u32)
        .saturating_mul(px_per_line)
        .max(px_per_line)
        .min(img_h.saturating_sub(slider_top_px).max(1));
    MinimapLayout {
        start_line,
        shown_lines,
        px_per_line,
        px_per_char,
        slider_top_px,
        slider_height_px,
    }
}

pub fn line_at_px(layout: MinimapLayout, y_px: u32, line_count: usize) -> usize {
    let line_count = line_count.max(1);
    let idx = (y_px / layout.px_per_line.max(1)) as usize;
    layout
        .start_line
        .saturating_add(idx)
        .min(line_count.saturating_sub(1))
}

pub fn line_at_cell_row(
    layout: MinimapLayout,
    row: usize,
    track_rows: usize,
    img_h: u32,
    line_count: usize,
) -> usize {
    let track_rows = track_rows.max(1);
    let y_px = if track_rows <= 1 {
        0
    } else {
        ((row as u32).saturating_mul(img_h.saturating_sub(1))) / (track_rows as u32 - 1)
    };
    line_at_px(layout, y_px, line_count)
}

/// Base RGB without viewport tint (cacheable across scroll when window stable).
///
/// `line_cells` returns `(char, rgb)` runs for each source line — typically the
/// buffer's syntax colors resolved through the live terminal palette.
pub fn render_base_rgb(
    layout: MinimapLayout,
    img_w: u32,
    img_h: u32,
    theme: MinimapTheme,
    mut line_cells: impl FnMut(usize, usize) -> Vec<(char, (u8, u8, u8))>,
) -> Vec<u8> {
    let img_w = img_w.max(1);
    let img_h = img_h.max(1);
    let mut rgb = vec![0u8; (img_w as usize) * (img_h as usize) * 3];
    fill_bg(&mut rgb, theme.bg);

    let px_per_line = layout.px_per_line.max(1);
    let px_per_char = layout.px_per_char.max(1);
    let source_cols = ((img_w / px_per_char) as usize).clamp(1, 256);

    for line_idx in 0..layout.shown_lines {
        let src_line = layout.start_line.saturating_add(line_idx);
        let cells = line_cells(src_line, source_cols);
        let y0 = (line_idx as u32).saturating_mul(px_per_line);
        if y0 >= img_h {
            break;
        }
        let y1 = (y0 + px_per_line).min(img_h);
        paint_line_cells(&mut rgb, img_w, y0, y1, &cells, px_per_char);
    }
    rgb
}

/// Heuristic colors when no syntax spans are available (tests / empty cache).
#[cfg(test)]
pub fn heuristic_line_cells(
    text: &str,
    cols: usize,
    theme: MinimapTheme,
) -> Vec<(char, (u8, u8, u8))> {
    text.chars()
        .take(cols)
        .map(|ch| (ch, sample_color(ch, theme)))
        .collect()
}

/// Apply viewport band tint into `out` (reuses caller buffer; no extra alloc).
pub fn apply_viewport_tint_into(
    base: &[u8],
    out: &mut Vec<u8>,
    img_w: u32,
    img_h: u32,
    layout: MinimapLayout,
    theme: MinimapTheme,
) {
    out.clear();
    out.extend_from_slice(base);
    let img_w = img_w.max(1) as usize;
    let slider_y0 = layout.slider_top_px.min(img_h);
    let slider_y1 = layout
        .slider_top_px
        .saturating_add(layout.slider_height_px)
        .min(img_h);
    let delta = theme.view_delta;
    for y in slider_y0..slider_y1 {
        let row = (y as usize).saturating_mul(img_w).saturating_mul(3);
        for x in 0..img_w {
            let i = row + x * 3;
            if i + 2 >= out.len() {
                break;
            }
            out[i] = tint(out[i], delta);
            out[i + 1] = tint(out[i + 1], delta);
            out[i + 2] = tint(out[i + 2], delta);
        }
    }
}

/// Apply viewport band tint onto a copy of base RGB.
#[allow(dead_code)] // exercised by unit tests; release path uses `apply_viewport_tint_into`.
pub fn apply_viewport_tint(
    base: &[u8],
    img_w: u32,
    img_h: u32,
    layout: MinimapLayout,
    theme: MinimapTheme,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(base.len());
    apply_viewport_tint_into(base, &mut out, img_w, img_h, layout, theme);
    out
}

fn tint(v: u8, delta: i16) -> u8 {
    (i16::from(v) + delta).clamp(0, 255) as u8
}

fn fill_bg(rgb: &mut [u8], bg: (u8, u8, u8)) {
    for px in rgb.chunks_exact_mut(3) {
        px[0] = bg.0;
        px[1] = bg.1;
        px[2] = bg.2;
    }
}

fn paint_line_cells(
    rgb: &mut [u8],
    img_w: u32,
    y0: u32,
    y1: u32,
    cells: &[(char, (u8, u8, u8))],
    px_per_char: u32,
) {
    let img_w = img_w as usize;
    let cell = px_per_char.max(1);
    let line_h = y1.saturating_sub(y0).max(1);
    // Prefer real 5×7 letter shapes; only fall back to greeked bars when the
    // cell is too small to carry a glyph (pressure / tiny strips).
    let use_glyphs = cell >= GLYPH_W && line_h >= GLYPH_H;
    let mut x = 0u32;
    for &(ch, (r, g, b)) in cells {
        if x as usize >= img_w {
            break;
        }
        if ch == ' ' || ch == '\t' {
            x = x.saturating_add(cell);
            continue;
        }
        if use_glyphs {
            paint_glyph(rgb, img_w, x, y0, y1, ch, r, g, b);
        } else {
            paint_bar(rgb, img_w, x, y0, y1, cell, r, g, b);
        }
        x = x.saturating_add(cell);
    }
}

fn paint_bar(
    rgb: &mut [u8],
    img_w: usize,
    x: u32,
    y0: u32,
    y1: u32,
    cell: u32,
    r: u8,
    g: u8,
    b: u8,
) {
    let line_h = y1.saturating_sub(y0).max(1);
    let pad = (line_h / 4).max(if line_h > 2 { 1 } else { 0 });
    let ink_y0 = y0.saturating_add(pad);
    let ink_y1 = y1.saturating_sub(pad).max(ink_y0.saturating_add(1));
    let ink_w = cell.saturating_sub(1).max(1) as usize;
    let x0 = x as usize;
    let x1 = (x0 + ink_w).min(img_w);
    for y in ink_y0..ink_y1 {
        let row = (y as usize).saturating_mul(img_w).saturating_mul(3);
        for xi in x0..x1 {
            let i = row + xi * 3;
            if i + 2 >= rgb.len() {
                break;
            }
            rgb[i] = r;
            rgb[i + 1] = g;
            rgb[i + 2] = b;
        }
    }
}

fn paint_glyph(
    rgb: &mut [u8],
    img_w: usize,
    x: u32,
    y0: u32,
    y1: u32,
    ch: char,
    r: u8,
    g: u8,
    b: u8,
) {
    let Some(rows) = glyph_rows(ch) else {
        paint_bar(rgb, img_w, x, y0, y1, GLYPH_W, r, g, b);
        return;
    };
    let line_h = y1.saturating_sub(y0).max(1);
    let pad_y = line_h.saturating_sub(GLYPH_H) / 2;
    let gy0 = y0.saturating_add(pad_y);
    for (row_i, bits) in rows.iter().enumerate() {
        let y = gy0.saturating_add(row_i as u32);
        if y >= y1 {
            break;
        }
        let row = (y as usize).saturating_mul(img_w).saturating_mul(3);
        for col in 0..GLYPH_W {
            // bit4 = leftmost pixel of the 5-wide cell
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            let xi = x.saturating_add(col) as usize;
            if xi >= img_w {
                break;
            }
            let i = row + xi * 3;
            if i + 2 >= rgb.len() {
                break;
            }
            rgb[i] = r;
            rgb[i + 1] = g;
            rgb[i + 2] = b;
        }
    }
}

/// 5×7 ASCII bitmaps (bit4 = left). Missing → solid fallback bar.
/// Shared with fullscreen Help Kitty rasterization.
pub(crate) fn glyph_rows(ch: char) -> Option<&'static [u8; 7]> {
    let c = if ch.is_ascii() { ch as u8 } else { b'?' };
    Some(match c {
        b'!' => &[0x04, 0x04, 0x04, 0x04, 0x00, 0x00, 0x04],
        b'"' => &[0x0A, 0x0A, 0x0A, 0x00, 0x00, 0x00, 0x00],
        b'#' => &[0x0A, 0x1F, 0x0A, 0x0A, 0x1F, 0x0A, 0x00],
        b'$' => &[0x04, 0x0F, 0x14, 0x0E, 0x05, 0x1E, 0x04],
        b'%' => &[0x19, 0x19, 0x02, 0x04, 0x08, 0x13, 0x13],
        b'&' => &[0x0C, 0x12, 0x14, 0x08, 0x15, 0x12, 0x0D],
        b'\'' => &[0x04, 0x04, 0x04, 0x00, 0x00, 0x00, 0x00],
        b'(' => &[0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => &[0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        b'*' => &[0x00, 0x04, 0x15, 0x0E, 0x15, 0x04, 0x00],
        b'+' => &[0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        b',' => &[0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x08],
        b'-' => &[0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        b'.' => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        b'/' => &[0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        b'0' => &[0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => &[0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => &[0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        b'3' => &[0x1F, 0x01, 0x02, 0x06, 0x01, 0x11, 0x0E],
        b'4' => &[0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => &[0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        b'6' => &[0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => &[0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => &[0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => &[0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        b':' => &[0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        b';' => &[0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x04, 0x08],
        b'<' => &[0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        b'=' => &[0x00, 0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00],
        b'>' => &[0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        b'?' => &[0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        b'@' => &[0x0E, 0x11, 0x17, 0x15, 0x17, 0x10, 0x0E],
        b'A' => &[0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => &[0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => &[0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'D' => &[0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        b'E' => &[0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => &[0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => &[0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => &[0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => &[0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'J' => &[0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        b'K' => &[0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => &[0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => &[0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => &[0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => &[0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => &[0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'Q' => &[0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'R' => &[0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => &[0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => &[0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => &[0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => &[0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => &[0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        b'X' => &[0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        b'Y' => &[0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'Z' => &[0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b'[' => &[0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
        b'\\' => &[0x10, 0x10, 0x08, 0x04, 0x02, 0x01, 0x01],
        b']' => &[0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
        b'^' => &[0x04, 0x0A, 0x11, 0x00, 0x00, 0x00, 0x00],
        b'_' => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        b'`' => &[0x08, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00],
        b'a' => &[0x00, 0x00, 0x0E, 0x01, 0x0F, 0x11, 0x0F],
        b'b' => &[0x10, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x1E],
        b'c' => &[0x00, 0x00, 0x0E, 0x10, 0x10, 0x11, 0x0E],
        b'd' => &[0x01, 0x01, 0x0F, 0x11, 0x11, 0x11, 0x0F],
        b'e' => &[0x00, 0x00, 0x0E, 0x11, 0x1F, 0x10, 0x0E],
        b'f' => &[0x06, 0x08, 0x08, 0x1C, 0x08, 0x08, 0x08],
        b'g' => &[0x00, 0x0F, 0x11, 0x11, 0x0F, 0x01, 0x0E],
        b'h' => &[0x10, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x11],
        b'i' => &[0x04, 0x00, 0x0C, 0x04, 0x04, 0x04, 0x0E],
        b'j' => &[0x02, 0x00, 0x06, 0x02, 0x02, 0x12, 0x0C],
        b'k' => &[0x10, 0x10, 0x12, 0x14, 0x18, 0x14, 0x12],
        b'l' => &[0x0C, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'm' => &[0x00, 0x00, 0x1A, 0x15, 0x15, 0x15, 0x15],
        b'n' => &[0x00, 0x00, 0x1E, 0x11, 0x11, 0x11, 0x11],
        b'o' => &[0x00, 0x00, 0x0E, 0x11, 0x11, 0x11, 0x0E],
        b'p' => &[0x00, 0x00, 0x1E, 0x11, 0x1E, 0x10, 0x10],
        b'q' => &[0x00, 0x00, 0x0F, 0x11, 0x0F, 0x01, 0x01],
        b'r' => &[0x00, 0x00, 0x16, 0x19, 0x10, 0x10, 0x10],
        b's' => &[0x00, 0x00, 0x0F, 0x10, 0x0E, 0x01, 0x1E],
        b't' => &[0x08, 0x08, 0x1C, 0x08, 0x08, 0x09, 0x06],
        b'u' => &[0x00, 0x00, 0x11, 0x11, 0x11, 0x11, 0x0F],
        b'v' => &[0x00, 0x00, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'w' => &[0x00, 0x00, 0x11, 0x11, 0x15, 0x15, 0x0A],
        b'x' => &[0x00, 0x00, 0x11, 0x0A, 0x04, 0x0A, 0x11],
        b'y' => &[0x00, 0x00, 0x11, 0x11, 0x0F, 0x01, 0x0E],
        b'z' => &[0x00, 0x00, 0x1F, 0x02, 0x04, 0x08, 0x1F],
        b'{' => &[0x02, 0x04, 0x04, 0x08, 0x04, 0x04, 0x02],
        b'|' => &[0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'}' => &[0x08, 0x04, 0x04, 0x02, 0x04, 0x04, 0x08],
        b'~' => &[0x00, 0x00, 0x08, 0x15, 0x02, 0x00, 0x00],
        _ => return None,
    })
}

#[cfg(test)]
fn sample_color(ch: char, theme: MinimapTheme) -> (u8, u8, u8) {
    match ch {
        ' ' | '\t' => theme.bg,
        c if c.is_ascii_digit() => theme.digit,
        c if matches!(c, '"' | '\'' | '`') => theme.string,
        c if matches!(
            c,
            '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>' | ',' | ';' | ':' | '.'
        ) =>
        {
            theme.punct
        }
        c if matches!(c, '/' | '*' | '#' | '-' | '=') => theme.comment,
        c if c.is_ascii_alphabetic() || c == '_' => theme.ident,
        _ => theme.fg,
    }
}

/// Flip-card preview — same glyph raster as Kitty minimap (`render_base_rgb`).
/// AFK tick28: restore 960×640 (tick25 240×150 was illegible after shear).
pub const FLIP_CARD_W: u32 = 960;
pub const FLIP_CARD_H: u32 = 640;
/// Under elevated memory pressure — smaller still, still readable.
pub const FLIP_CARD_PRESSURE_W: u32 = 640;
pub const FLIP_CARD_PRESSURE_H: u32 = 400;
pub const FLIP_GLYPH_SCALE: u32 = 2;

/// Flip preview bitmap size for normal vs elevated pressure.
pub fn flip_card_dims(elevated_pressure: bool) -> (u32, u32) {
    if elevated_pressure {
        (FLIP_CARD_PRESSURE_W, FLIP_CARD_PRESSURE_H)
    } else {
        (FLIP_CARD_W, FLIP_CARD_H)
    }
}

/// Glyph cell scale for Flip previews — 1× under pressure (faster paint_glyph).
pub fn flip_glyph_scale(elevated_pressure: bool) -> u32 {
    if elevated_pressure {
        1
    } else {
        FLIP_GLYPH_SCALE
    }
}
/// Soft cap on cached Flip/minimap preview Arcs (RAM).
/// AFK: neighbor warm is ±1 (≤3 keys) — 4 is enough; was 24 when every buffer warmed.
/// AFK tick28: 8 slots — larger card Arcs; still ±1 warm window.
pub const PREVIEW_POOL_CAP: usize = 8;

/// Stable cache key for a buffer strip / Flip card preview.
pub fn preview_key(slot: usize, revision: u64, theme: u64, width: u32, height: u32) -> u64 {
    let mut k = (slot as u64).wrapping_mul(0x9E3779B97F4A7C15);
    k ^= revision.wrapping_mul(0xC2B2AE3D27D4EB4F);
    k ^= theme;
    k ^= (u64::from(width) << 32) | u64::from(height);
    k
}

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Shared headless preview: Flip 3D cards + future minimap warm-start.
/// Raster runs on a background thread; UI only clones `Arc` (zero-copy bytes).
pub struct PreviewPool {
    cache: Arc<Mutex<HashMap<u64, Arc<crate::flip3d::PreviewImage>>>>,
    inflight: Arc<Mutex<HashSet<u64>>>,
    tx: Mutex<Option<mpsc::Sender<PreviewJob>>>,
    /// Generation bump when a job completes — UI presents without blocking.
    ready_gen: Arc<AtomicU64>,
    seen_gen: AtomicU64,
}

#[derive(Clone)]
pub struct PreviewJob {
    pub key: u64,
    pub theme: MinimapTheme,
    pub width: u32,
    pub height: u32,
    /// Glyph cell scale (1 under pressure, 2 normally).
    pub glyph_scale: u32,
    /// Token-colored cells (snapshot from UI highlight cache — worker never touches ropes).
    pub lines: Vec<Vec<(char, (u8, u8, u8))>>,
}

impl Default for PreviewPool {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewPool {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            tx: Mutex::new(None),
            ready_gen: Arc::new(AtomicU64::new(0)),
            seen_gen: AtomicU64::new(0),
        }
    }

    fn ensure_worker(&self) {
        let mut guard = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel::<PreviewJob>();
        let cache = Arc::clone(&self.cache);
        let inflight = Arc::clone(&self.inflight);
        let ready = Arc::clone(&self.ready_gen);
        std::thread::Builder::new()
            .name("posit-preview".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    let key = job.key;
                    let img = render_preview_job(&job);
                    if let Ok(mut map) = cache.lock() {
                        map.insert(key, Arc::new(img));
                        trim_preview_cache(&mut map);
                    }
                    if let Ok(mut set) = inflight.lock() {
                        set.remove(&key);
                    }
                    ready.fetch_add(1, Ordering::Release);
                }
            })
            .ok();
        *guard = Some(tx);
    }

    pub fn get(&self, key: u64) -> Option<Arc<crate::flip3d::PreviewImage>> {
        self.cache
            .lock()
            .ok()
            .and_then(|map| map.get(&key).cloned())
    }

    /// Publish an already-rasterized Arc (zero-copy share with Flip / minimap).
    pub fn insert(&self, key: u64, img: Arc<crate::flip3d::PreviewImage>) {
        if let Ok(mut map) = self.cache.lock() {
            map.insert(key, img);
            trim_preview_cache(&mut map);
        }
        if let Ok(mut set) = self.inflight.lock() {
            set.remove(&key);
        }
        self.ready_gen.fetch_add(1, Ordering::Release);
    }

    /// True when UI should snapshot + enqueue (not cached, not already queued).
    pub fn needs_work(&self, key: u64) -> bool {
        if self.get(key).is_some() {
            return false;
        }
        self.inflight
            .lock()
            .ok()
            .is_none_or(|set| !set.contains(&key))
    }

    /// Non-blocking enqueue. No-op if already cached or in flight.
    pub fn request(&self, job: PreviewJob) {
        if self.get(job.key).is_some() {
            return;
        }
        let key = job.key;
        {
            let Ok(mut set) = self.inflight.lock() else {
                return;
            };
            if !set.insert(key) {
                return;
            }
        }
        self.ensure_worker();
        let ok = self
            .tx
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|tx| tx.send(job)))
            .is_some_and(|r| r.is_ok());
        if !ok {
            if let Ok(mut set) = self.inflight.lock() {
                set.remove(&key);
            }
        }
    }

    /// Cache hit or synchronous raster (Flip open path — front cards must not show dots).
    /// Still Arc-shared afterward so later frames are zero-copy.
    pub fn get_or_render(&self, job: PreviewJob) -> Arc<crate::flip3d::PreviewImage> {
        if let Some(hit) = self.get(job.key) {
            return hit;
        }
        let key = job.key;
        let img = Arc::new(render_preview_job(&job));
        if let Ok(mut map) = self.cache.lock() {
            map.insert(key, Arc::clone(&img));
            trim_preview_cache(&mut map);
        }
        if let Ok(mut set) = self.inflight.lock() {
            set.remove(&key);
        }
        self.ready_gen.fetch_add(1, Ordering::Release);
        img
    }

    /// True when background work finished since last poll (schedule a present).
    pub fn take_ready(&self) -> bool {
        let now = self.ready_gen.load(Ordering::Acquire);
        let seen = self.seen_gen.load(Ordering::Relaxed);
        if now != seen {
            self.seen_gen.store(now, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.cache.lock() {
            map.clear();
        }
        if let Ok(mut set) = self.inflight.lock() {
            set.clear();
        }
    }
}

fn trim_preview_cache(map: &mut HashMap<u64, Arc<crate::flip3d::PreviewImage>>) {
    while map.len() > PREVIEW_POOL_CAP {
        if let Some(old) = map.keys().next().copied() {
            map.remove(&old);
        } else {
            break;
        }
    }
}

fn render_preview_job(job: &PreviewJob) -> crate::flip3d::PreviewImage {
    let width = job.width.max(8);
    let height = job.height.max(8);
    let line_count = job.lines.len().max(1);
    let scale = job.glyph_scale.max(1);
    let layout = layout_proportional(
        line_count,
        0,
        line_count.min(24).max(1),
        height,
        scale,
    );
    let lines = &job.lines;
    let rgb = render_base_rgb(layout, width, height, job.theme, |line, cols| {
        lines
            .get(line)
            .map(|cells| cells.iter().take(cols).copied().collect())
            .unwrap_or_default()
    });
    crate::flip3d::PreviewImage {
        width,
        height,
        rgb,
    }
}

/// In-memory cache so scroll-only frames skip rope resampling + full fills.
/// Base is Arc-shared with PreviewPool so Flip samples the same Kitty glyphs.
#[derive(Default)]
pub struct MinimapFrameCache {
    content: Option<MinimapContentKey>,
    base: Option<Arc<crate::flip3d::PreviewImage>>,
    /// Reused tinted frame buffer (avoids alloc on every scroll).
    tinted: Vec<u8>,
}

impl MinimapFrameCache {
    /// Returns a slice of the reusable tinted buffer (valid until next call).
    pub fn rgb_for(
        &mut self,
        content: MinimapContentKey,
        layout: MinimapLayout,
        img_w: u32,
        img_h: u32,
        theme: MinimapTheme,
        line_cells: impl FnMut(usize, usize) -> Vec<(char, (u8, u8, u8))>,
    ) -> &[u8] {
        if self.content != Some(content) || self.base.is_none() {
            let rgb = render_base_rgb(layout, img_w, img_h, theme, line_cells);
            self.base = Some(Arc::new(crate::flip3d::PreviewImage {
                width: img_w,
                height: img_h,
                rgb,
            }));
            self.content = Some(content);
        }
        let base = self.base.as_ref().map(|b| b.rgb.as_slice()).unwrap_or(&[]);
        apply_viewport_tint_into(base, &mut self.tinted, img_w, img_h, layout, theme);
        &self.tinted
    }

    pub fn base_arc(&self) -> Option<Arc<crate::flip3d::PreviewImage>> {
        self.base.clone()
    }

    pub fn clear(&mut self) {
        self.content = None;
        self.base = None;
        self.tinted.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_when_content_fits() {
        // Dogfood shells may export FORCE; this law is about production hide path.
        std::env::remove_var("POSIT_EDIT_MINIMAP_FORCE");
        assert!(!needs_scroll(20, 40));
        assert!(needs_scroll(100, 40));
        assert_eq!(edge_chrome(80, true, false, false, 20, 40), (0, 0));
        assert_eq!(edge_chrome(80, true, false, false, 100, 40).1, 0); // kitty replaces sb
        assert_eq!(edge_chrome(80, false, false, false, 100, 40), (2, 1));
        assert_eq!(edge_chrome(80, true, true, true, 100, 40), (0, 1)); // critical → sb only
    }

    #[test]
    fn large_file_uses_sliding_window() {
        let img_h = 400;
        let layout = layout_proportional(10_000, 5_000, 40, img_h, 1);
        assert_eq!(layout.shown_lines, 50); // 400/8
        assert!(layout.start_line > 0);
        assert!(5_000 >= layout.start_line);
        assert!(5_000 < layout.start_line + layout.shown_lines);
    }

    #[test]
    fn scroll_tint_reuses_base() {
        // File fits the strip → start_line stays 0; only the viewport band moves.
        let layout = layout_proportional(20, 0, 5, 160, 1);
        let theme = MinimapTheme::light();
        let base = render_base_rgb(layout, 60, 160, theme, |line, cols| {
            heuristic_line_cells(&format!("line{line}"), cols, theme)
        });
        let a = apply_viewport_tint(&base, 60, 160, layout, theme);
        let layout2 = layout_proportional(20, 10, 5, 160, 1);
        let b = apply_viewport_tint(&base, 60, 160, layout2, theme);
        assert_eq!(layout.start_line, layout2.start_line);
        assert_ne!(layout.slider_top_px, layout2.slider_top_px);
        assert_ne!(a, b);
        assert_eq!(a.len(), base.len());
    }

    #[test]
    fn kitty_minimap_wider_readable() {
        assert_eq!(minimap_cols(56, true, false), 24);
        assert_eq!(minimap_cols(40, false, false), 2);
        assert_eq!(edge_chrome(10, true, false, false, 100, 20), (0, 0));
    }

    #[test]
    fn bend_minimap_laws_conformance_scaled() {
        // scale 1 → 8 px/line; 160px → 20 lines.
        let layout = layout_proportional(10, 0, 5, 160, 1);
        assert_eq!(layout.px_per_line, 8);
        assert_eq!(layout.shown_lines, 20);
        assert_eq!(layout.start_line, 0);
        let large = layout_proportional(80, 40, 10, 160, 1);
        assert_eq!(large.start_line, 35);
        assert_eq!(line_at_px(large, 0, 80), 35);
    }

    #[test]
    fn flip_preview_uses_double_scale_glyphs() {
        assert!(FLIP_GLYPH_SCALE >= 2, "1× Flip glyphs read as TV static");
        assert_eq!(FLIP_CARD_W, 960);
        assert_eq!(FLIP_CARD_H, 640);
        assert_eq!(FLIP_GLYPH_SCALE, 2);
        assert!(PREVIEW_POOL_CAP <= 8);
        assert_eq!(flip_card_dims(false), (FLIP_CARD_W, FLIP_CARD_H));
        assert_eq!(
            flip_card_dims(true),
            (FLIP_CARD_PRESSURE_W, FLIP_CARD_PRESSURE_H)
        );
        assert!(FLIP_CARD_PRESSURE_W * FLIP_CARD_PRESSURE_H < FLIP_CARD_W * FLIP_CARD_H);
        let theme = MinimapTheme::light();
        assert_eq!(flip_glyph_scale(false), 2);
        assert_eq!(flip_glyph_scale(true), 1);
        let job = PreviewJob {
            key: 1,
            theme,
            width: FLIP_CARD_W,
            height: FLIP_CARD_H,
            glyph_scale: FLIP_GLYPH_SCALE,
            lines: vec![
                vec![('A', theme.fg), ('B', theme.keyword), ('C', theme.string)],
                vec![('1', theme.digit), ('2', theme.fg), ('3', theme.function)],
            ],
        };
        let img = render_preview_job(&job);
        assert_eq!(img.width, FLIP_CARD_W);
        assert_eq!(img.height, FLIP_CARD_H);
        // Light paper must dominate — not a dark static field.
        let light = img
            .rgb
            .chunks(3)
            .filter(|p| p[0] > 200 && p[1] > 190 && p[2] > 180)
            .count();
        assert!(
            light > (img.rgb.len() / 3) / 2,
            "Flip preview should be mostly light paper, got {light} light px"
        );
    }

    #[test]
    fn kitty_glyphs_are_shaped_not_solid_bars() {
        let layout = layout_proportional(4, 0, 2, 32, 1);
        assert_eq!(layout.px_per_char, 6);
        let theme = MinimapTheme::light();
        let rgb = render_base_rgb(layout, 48, 32, theme, |line, cols| match line {
            0 => heuristic_line_cells("A", cols, theme),
            1 => heuristic_line_cells("fn", cols, theme),
            _ => Vec::new(),
        });
        let bg = theme.bg;
        let ink = theme.ident;
        // First glyph cell (0..5) on line 0 must mix bg + ink (holes in 'A').
        let mut bg_n = 0usize;
        let mut ink_n = 0usize;
        for y in 0..8u32 {
            for x in 0..5usize {
                let i = (y as usize * 48 + x) * 3;
                let px = (rgb[i], rgb[i + 1], rgb[i + 2]);
                if px == bg {
                    bg_n += 1;
                } else if px == ink {
                    ink_n += 1;
                }
            }
        }
        assert!(ink_n >= 8, "expected letter ink, got {ink_n}");
        assert!(bg_n >= 8, "expected glyph holes (bg), got {bg_n}");
        // 'A' (line 0) and 'f' (line 1) must not paint identical cells.
        let mut a_cell = Vec::with_capacity(40);
        let mut f_cell = Vec::with_capacity(40);
        for y in 0..8usize {
            for x in 0..5usize {
                let i = (y * 48 + x) * 3;
                a_cell.push((rgb[i], rgb[i + 1], rgb[i + 2]));
                let j = ((y + 8) * 48 + x) * 3;
                f_cell.push((rgb[j], rgb[j + 1], rgb[j + 2]));
            }
        }
        assert_ne!(a_cell, f_cell, "distinct letters must produce distinct bitmaps");
    }

    #[test]
    fn dump_legible_minimap_ppm_for_evidence() {
        let layout = layout_proportional(40, 0, 12, 320, 1);
        let theme = MinimapTheme::light();
        let lines = [
            "fn line_0000() { println!(\"0\"); }",
            "fn line_0001() { println!(\"1\"); }",
            "fn line_0002() { println!(\"2\"); }",
            "    let ready = true;",
            "    // comment preview",
        ];
        let rgb = render_base_rgb(layout, 180, 320, theme, |line, cols| {
            lines
                .get(line % lines.len())
                .map(|s| heuristic_line_cells(s, cols, theme))
                .unwrap_or_default()
        });
        let tinted = apply_viewport_tint(&rgb, 180, 320, layout, theme);
        let path = std::env::temp_dir().join("posit-minimap-glyphs.ppm");
        let mut out = String::new();
        out.push_str("P3\n180 320\n255\n");
        for px in tinted.chunks_exact(3) {
            out.push_str(&format!("{} {} {}\n", px[0], px[1], px[2]));
        }
        std::fs::write(&path, out).expect("write ppm");
        assert!(path.is_file());
    }
}
