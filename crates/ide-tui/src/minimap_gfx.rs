//! Kitty graphics code minimap — VS Code–style proportional preview.
//!
//! Shown only when the buffer needs vertical scroll. On Kitty/Ghostty the image
//! strip **replaces** the text scrollbar. Paint is pressure-aware and caches the
//! base RGB so scroll-only frames patch the viewport band instead of resampling.

use crate::graphics::GraphicsBackend;
use ratatui::layout::Rect;

/// Stable Kitty image id pair for the focused editor minimap (away from Canvas).
/// Keep below 256 so unicode-placeholder FG uses indexed color (Ghostty-friendly).
pub const EDITOR_MINIMAP_IMAGE_ID: u32 = 41;
pub const EDITOR_MINIMAP_IMAGE_ID_ALT: u32 = 42;

pub fn minimap_alternate_id(id: u32) -> u32 {
    if id == EDITOR_MINIMAP_IMAGE_ID {
        EDITOR_MINIMAP_IMAGE_ID_ALT
    } else {
        EDITOR_MINIMAP_IMAGE_ID
    }
}

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
    pub theme: u8,
    /// Language id fingerprint so retargeting syntax invalidates the strip.
    pub language: u64,
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
    pub theme: u8,
    pub language: u64,
}

/// Stable fingerprint for minimap cache keys (language string).
pub fn language_key(language: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in language.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
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

/// Dark-host syntax hues (One Dark–ish). Editor `token_style` and the Kitty
/// strip both read these via `MinimapTheme` so they stay 1:1.
pub const SYNTAX_KEYWORD: (u8, u8, u8) = (0xc6, 0x78, 0xdd);
pub const SYNTAX_COMMENT: (u8, u8, u8) = (0xe5, 0xc0, 0x7b);
pub const SYNTAX_STRING: (u8, u8, u8) = (0x98, 0xc3, 0x79);
pub const SYNTAX_TYPE: (u8, u8, u8) = (0x56, 0xb6, 0xc2);
pub const SYNTAX_FUNCTION: (u8, u8, u8) = (0x61, 0xaf, 0xef);

/// Light-host syntax hues — darker cousins of the dark set. The One Dark pastels
/// wash out on Ghostty cream (~1.5:1 for comments); these clear ~4.5:1 on cream.
pub const SYNTAX_LIGHT_KEYWORD: (u8, u8, u8) = (0x7a, 0x3e, 0x9d);
pub const SYNTAX_LIGHT_COMMENT: (u8, u8, u8) = (0x8a, 0x6d, 0x3b);
pub const SYNTAX_LIGHT_STRING: (u8, u8, u8) = (0x3d, 0x7a, 0x37);
pub const SYNTAX_LIGHT_TYPE: (u8, u8, u8) = (0x1a, 0x7a, 0x85);
pub const SYNTAX_LIGHT_FUNCTION: (u8, u8, u8) = (0x1a, 0x6f, 0xb5);

/// Terminal-derived palette for the RGB strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimapTheme {
    pub id: u8,
    pub bg: (u8, u8, u8),
    /// Plain / default text (editor inherits terminal fg; strip uses this).
    pub fg: (u8, u8, u8),
    pub keyword: (u8, u8, u8),
    pub comment: (u8, u8, u8),
    pub string: (u8, u8, u8),
    pub ty: (u8, u8, u8),
    pub function: (u8, u8, u8),
    /// Added to RGB inside the viewport band (can be slightly negative via saturating).
    pub view_delta: i16,
}

impl MinimapTheme {
    pub fn light() -> Self {
        Self {
            id: 3,
            bg: (250, 245, 230),
            fg: (70, 80, 55),
            keyword: SYNTAX_LIGHT_KEYWORD,
            comment: SYNTAX_LIGHT_COMMENT,
            string: SYNTAX_LIGHT_STRING,
            ty: SYNTAX_LIGHT_TYPE,
            function: SYNTAX_LIGHT_FUNCTION,
            view_delta: -18,
        }
    }

    pub fn dark() -> Self {
        Self {
            id: 4,
            bg: (0x1e, 0x1e, 0x24),
            fg: (0xd4, 0xd4, 0xd4),
            keyword: SYNTAX_KEYWORD,
            comment: SYNTAX_COMMENT,
            string: SYNTAX_STRING,
            ty: SYNTAX_TYPE,
            function: SYNTAX_FUNCTION,
            view_delta: 28,
        }
    }

    /// Lightness from background luminance — never key off `id` (ids move).
    pub fn is_light(self) -> bool {
        let (r, g, b) = self.bg;
        // Rec. 601-ish: (3R + 4G + B) / 8
        (u16::from(r) * 3 + u16::from(g) * 4 + u16::from(b)) / 8 >= 128
    }
}

/// Probed host default colors (OSC 10 / OSC 11), when available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbedColors {
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

static PROBED: std::sync::Mutex<Option<ProbedColors>> = std::sync::Mutex::new(None);

/// True when OSC/env probe supplied a real background (not a hard-coded guess).
pub fn probed_background() -> Option<(u8, u8, u8)> {
    PROBED
        .lock()
        .ok()
        .and_then(|g| g.map(|c| c.bg))
        .or_else(env_override_bg)
}

fn env_override_bg() -> Option<(u8, u8, u8)> {
    parse_rgb_env("POSIT_EDIT_MINIMAP_BG")
}

fn env_override_fg() -> Option<(u8, u8, u8)> {
    parse_rgb_env("POSIT_EDIT_MINIMAP_FG")
}

/// `POSIT_EDIT_MINIMAP_BG=#rrggbb` or `r,g,b`.
pub fn parse_rgb_env(key: &str) -> Option<(u8, u8, u8)> {
    let raw = std::env::var(key).ok()?;
    parse_rgb_literal(&raw)
}

pub fn parse_rgb_literal(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some((r, g, b));
        }
    }
    let parts: Vec<_> = s.split([',', ' ']).filter(|p| !p.is_empty()).collect();
    if parts.len() == 3 {
        let r: u8 = parts[0].parse().ok()?;
        let g: u8 = parts[1].parse().ok()?;
        let b: u8 = parts[2].parse().ok()?;
        return Some((r, g, b));
    }
    None
}

/// Parse an OSC 10/11 payload (`rgb:RRRR/GGGG/BBBB` or `rgba:…`).
pub fn parse_osc_rgb(payload: &str) -> Option<(u8, u8, u8)> {
    let payload = payload.trim();
    let body = payload
        .strip_prefix("rgb:")
        .or_else(|| payload.strip_prefix("rgba:"))?;
    let mut parts = body.split('/');
    let r = parse_osc_component(parts.next()?)?;
    let g = parse_osc_component(parts.next()?)?;
    let b = parse_osc_component(parts.next()?)?;
    Some((r, g, b))
}

fn parse_osc_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(match s.len() {
        1 => (v * 0x11) as u8,
        2 => v as u8,
        3 => (v >> 4) as u8,
        _ => (v >> 8) as u8, // 16-bit channel → 8-bit
    })
}

/// Ask the host for default fg/bg via OSC 10/11 before raw mode.
///
/// Safe to call more than once; results are cached. No-ops when `/dev/tty` is
/// missing or the host ignores the query (50ms budget).
pub fn probe_terminal_colors() {
    if std::env::var_os("POSIT_EDIT_SKIP_COLOR_PROBE").is_some() {
        return;
    }
    if let (Some(fg), Some(bg)) = (env_override_fg(), env_override_bg()) {
        let _ = PROBED.lock().map(|mut g| *g = Some(ProbedColors { fg, bg }));
        return;
    }
    let Some(probed) = query_osc_colors_from_tty() else {
        // Env-only overrides still apply when OSC is silent.
        if let Some(bg) = env_override_bg() {
            let fg = env_override_fg().unwrap_or((0x20, 0x20, 0x20));
            let _ = PROBED.lock().map(|mut g| *g = Some(ProbedColors { fg, bg }));
        }
        return;
    };
    let mut colors = probed;
    if let Some(bg) = env_override_bg() {
        colors.bg = bg;
    }
    if let Some(fg) = env_override_fg() {
        colors.fg = fg;
    }
    let _ = PROBED.lock().map(|mut g| *g = Some(colors));
}

fn query_osc_colors_from_tty() -> Option<ProbedColors> {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;

    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    // Query bg then fg.
    tty.write_all(b"\x1b]11;?\x1b\\\x1b]10;?\x1b\\").ok()?;
    let _ = tty.flush();

    let mut buf = Vec::with_capacity(128);
    let mut tmp = [0u8; 64];
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    let fd = tty.as_raw_fd();
    while std::time::Instant::now() < deadline {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let remain = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .min(i32::MAX as u128) as i32;
        let n = unsafe { libc::poll(&mut pfd, 1, remain.max(1)) };
        if n <= 0 {
            break;
        }
        match tty.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(2).any(|w| w == b"\x1b\\") || buf.contains(&0x07) {
                    // Prefer waiting for both answers when they arrive together.
                    let text = String::from_utf8_lossy(&buf);
                    if text.matches("rgb:").count() + text.matches("rgba:").count() >= 2 {
                        break;
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => break,
        }
    }
    parse_osc_color_responses(&buf)
}

/// Extract OSC 10/11 answers from a raw tty byte stream.
pub fn parse_osc_color_responses(bytes: &[u8]) -> Option<ProbedColors> {
    let text = String::from_utf8_lossy(bytes);
    let mut fg = None;
    let mut bg = None;
    for chunk in text.split(['\u{1b}', '\u{7}']) {
        let chunk = chunk.trim_start_matches(']');
        let chunk = chunk.trim_end_matches('\\').trim();
        if let Some(rest) = chunk.strip_prefix("11;") {
            bg = parse_osc_rgb(rest).or(bg);
        } else if let Some(rest) = chunk.strip_prefix("10;") {
            fg = parse_osc_rgb(rest).or(fg);
        }
    }
    // Bg alone is enough to fix the strip; invent a readable fg if needed.
    let bg = bg?;
    let fg = fg.unwrap_or(if luminance(bg) >= 128 {
        (0x1a, 0x1a, 0x1a)
    } else {
        (0xd4, 0xd4, 0xd4)
    });
    Some(ProbedColors { fg, bg })
}

fn luminance(bg: (u8, u8, u8)) -> u16 {
    (u16::from(bg.0) * 3 + u16::from(bg.1) * 4 + u16::from(bg.2)) / 8
}

/// Detect light/dark from host defaults / `COLORFGBG`, then paint with the
/// real terminal RGB when OSC 10/11 (or env overrides) answered.
///
/// Ghostty's default UI is light cream, and it often exports `COLORFGBG=15;0`
/// even on light themes — so Ghostty prefers light unless explicitly dark.
/// Override with `POSIT_EDIT_THEME=light|dark` when the host lies.
pub fn terminal_theme() -> MinimapTheme {
    let probed = PROBED.lock().ok().and_then(|g| *g);
    let force = std::env::var("POSIT_EDIT_THEME")
        .ok()
        .map(|v| v.to_ascii_lowercase());
    let mut theme = match force.as_deref() {
        Some("light" | "cream" | "day") => MinimapTheme::light(),
        Some("dark" | "night") => MinimapTheme::dark(),
        _ => {
            // Prefer luminance of the real background when we have it.
            if let Some(colors) = probed {
                if luminance(colors.bg) >= 128 {
                    MinimapTheme::light()
                } else {
                    MinimapTheme::dark()
                }
            } else {
                guess_theme_without_probe()
            }
        }
    };
    if let Some(colors) = probed {
        theme.bg = colors.bg;
        theme.fg = colors.fg;
    } else if let Some(bg) = env_override_bg() {
        theme.bg = bg;
        if let Some(fg) = env_override_fg() {
            theme.fg = fg;
        }
    }
    theme
}

fn guess_theme_without_probe() -> MinimapTheme {
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
/// - Narrow tile (`< 18`) → `(0, 0)`.
/// - Focused editor keeps a minimap even when the file fits (VS Code–style).
/// - Text scrollbar only when the buffer overflows the viewport.
/// - Kitty minimap replaces the text scrollbar: `(N, 0)`.
/// - Glyph fallback → thin minimap + optional scrollbar: `(2, 0|1)`.
pub fn edge_chrome(
    tile_w: u16,
    kitty: bool,
    pressure_critical: bool,
    pressure_warning: bool,
    line_count: usize,
    visible: usize,
) -> (u16, u16) {
    if tile_w < 18 {
        return (0, 0);
    }
    let scrollable = needs_scroll(line_count, visible);
    // Jetsam-class pressure: keep a text scrollbar when needed, drop the strip
    // (unless dogfood force is on).
    if pressure_critical && !force_minimap() {
        return (0, u16::from(scrollable));
    }
    let cols = minimap_cols(tile_w, kitty, pressure_warning);
    if cols == 0 {
        return (0, u16::from(scrollable));
    }
    if kitty {
        (cols, 0)
    } else {
        (cols, u16::from(scrollable))
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
/// `line_cells(line, max_cols)` returns `(char, rgb)` using the same syntax
/// palette as the editor — callers must not invent a second color heuristic.
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

/// Test/helper: paint every non-space glyph with `theme.fg` (no syntax).
pub fn plain_line_cells(prefix: &str, theme: MinimapTheme) -> Vec<(char, (u8, u8, u8))> {
    prefix
        .chars()
        .map(|ch| {
            if ch == ' ' || ch == '\t' {
                (ch, theme.bg)
            } else {
                (ch, theme.fg)
            }
        })
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
fn glyph_rows(ch: char) -> Option<&'static [u8; 7]> {
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

/// In-memory cache so scroll-only frames skip rope resampling + full fills.
#[derive(Default)]
pub struct MinimapFrameCache {
    content: Option<MinimapContentKey>,
    base_rgb: Vec<u8>,
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
        if self.content != Some(content) || self.base_rgb.is_empty() {
            self.base_rgb = render_base_rgb(layout, img_w, img_h, theme, line_cells);
            self.content = Some(content);
        }
        apply_viewport_tint_into(
            &self.base_rgb,
            &mut self.tinted,
            img_w,
            img_h,
            layout,
            theme,
        );
        &self.tinted
    }

    pub fn clear(&mut self) {
        self.content = None;
        self.base_rgb.clear();
        self.tinted.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_luminance_and_env_override() {
        assert!(MinimapTheme::light().is_light());
        assert!(!MinimapTheme::dark().is_light());
        let _ = PROBED.lock().map(|mut g| *g = None);
        std::env::remove_var("POSIT_EDIT_MINIMAP_BG");
        std::env::remove_var("POSIT_EDIT_MINIMAP_FG");
        std::env::set_var("POSIT_EDIT_THEME", "dark");
        assert!(!terminal_theme().is_light());
        std::env::set_var("POSIT_EDIT_THEME", "light");
        assert!(terminal_theme().is_light());
        std::env::remove_var("POSIT_EDIT_THEME");
    }

    #[test]
    fn osc_and_env_rgb_feed_minimap_background() {
        assert_eq!(
            parse_osc_rgb("rgb:fafa/f5f5/e6e6"),
            Some((0xfa, 0xf5, 0xe6))
        );
        assert_eq!(parse_osc_rgb("rgb:ab/cd/ef"), Some((0xab, 0xcd, 0xef)));
        assert_eq!(parse_rgb_literal("#1e1e24"), Some((0x1e, 0x1e, 0x24)));
        assert_eq!(parse_rgb_literal("10,20,30"), Some((10, 20, 30)));
        let bytes = b"\x1b]11;rgb:1010/2020/3030\x1b\\\x1b]10;rgb:eeee/eeee/eeee\x1b\\";
        let probed = parse_osc_color_responses(bytes).expect("osc");
        assert_eq!(probed.bg, (0x10, 0x20, 0x30));
        assert_eq!(probed.fg, (0xee, 0xee, 0xee));

        let _ = PROBED.lock().map(|mut g| *g = None);
        std::env::set_var("POSIT_EDIT_MINIMAP_BG", "#112233");
        std::env::set_var("POSIT_EDIT_MINIMAP_FG", "#ddeeff");
        std::env::remove_var("POSIT_EDIT_THEME");
        // Seed probe via env path used by probe_terminal_colors.
        probe_terminal_colors();
        let theme = terminal_theme();
        assert_eq!(theme.bg, (0x11, 0x22, 0x33));
        assert_eq!(theme.fg, (0xdd, 0xee, 0xff));
        assert_eq!(probed_background(), Some((0x11, 0x22, 0x33)));
        std::env::remove_var("POSIT_EDIT_MINIMAP_BG");
        std::env::remove_var("POSIT_EDIT_MINIMAP_FG");
        let _ = PROBED.lock().map(|mut g| *g = None);
    }

    #[test]
    fn keeps_minimap_when_content_fits() {
        // Dogfood shells may export FORCE; this law is about production chrome.
        std::env::remove_var("POSIT_EDIT_MINIMAP_FORCE");
        assert!(!needs_scroll(20, 40));
        assert!(needs_scroll(100, 40));
        // Short files still get a minimap so the right edge isn't empty while editing.
        let (mm, sb) = edge_chrome(80, true, false, false, 20, 40);
        assert!(mm >= 12, "kitty minimap cols={mm}");
        assert_eq!(sb, 0);
        assert_eq!(edge_chrome(80, true, false, false, 100, 40).1, 0); // kitty replaces sb
        assert_eq!(edge_chrome(80, false, false, false, 100, 40), (2, 1));
        assert_eq!(edge_chrome(80, false, false, false, 20, 40), (2, 0)); // glyph, no sb
        assert_eq!(edge_chrome(80, true, true, true, 100, 40), (0, 1)); // critical → sb only
        assert_eq!(edge_chrome(80, true, true, true, 20, 40), (0, 0)); // critical + fits
    }

    #[test]
    fn render_base_rgb_stays_under_budget() {
        let layout = layout_proportional(200, 0, 40, 400, 1);
        let theme = MinimapTheme::dark();
        let started = std::time::Instant::now();
        let mut samples = Vec::new();
        for _ in 0..5 {
            let t0 = std::time::Instant::now();
            let rgb = render_base_rgb(layout, 96, 400, theme, |line, cols| {
                plain_line_cells(
                    &format!("fn sample_{line}() {{ let x = {line}; }}"),
                    theme,
                )
                .into_iter()
                .take(cols)
                .collect()
            });
            assert_eq!(rgb.len(), 96 * 400 * 3);
            samples.push(t0.elapsed());
        }
        samples.sort();
        let median = samples[2];
        assert!(
            median < std::time::Duration::from_millis(25),
            "median minimap render {median:?} over 25ms budget (wall {:?})",
            started.elapsed()
        );
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
            plain_line_cells(&format!("line{line}"), theme)
                .into_iter()
                .take(cols)
                .collect()
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
    fn kitty_glyphs_are_shaped_not_solid_bars() {
        let layout = layout_proportional(4, 0, 2, 32, 1);
        assert_eq!(layout.px_per_char, 6);
        let theme = MinimapTheme::light();
        let rgb = render_base_rgb(layout, 48, 32, theme, |line, _| match line {
            0 => plain_line_cells("A", theme),
            1 => plain_line_cells("fn", theme),
            _ => Vec::new(),
        });
        let bg = theme.bg;
        let ink = theme.fg;
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
                .map(|s| {
                    plain_line_cells(s, theme)
                        .into_iter()
                        .take(cols)
                        .collect()
                })
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
