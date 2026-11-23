//! ShojiWM Flip 3D–style buffer switcher.
//!
//! Layout constants mirror `packages/config/src/flip-3d.tsx` (`slotPose` /
//! `slotOf` / `tick`). Rendering is a Kitty RGB stack (text fallback when
//! graphics are unavailable).

use std::f32::consts::PI;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Kitty image id for the switcher scene (away from Help/Canvas/minimap).
pub const FLIP3D_KITTY_IMAGE_ID: u32 = 91;

const SCROLL_TIME_CONSTANT_MS: f32 = 70.0;
const VISIBLE_WINDOWS: usize = 7;
const TURN_DEGREES: f32 = 20.0;
const PITCH_DEGREES: f32 = 6.0;
const FRONT_X: f32 = 0.13;
const FRONT_Y: f32 = -0.05;
const STEP_X: f32 = -0.085;
const STEP_Y: f32 = 0.048;
const SHRINK_PER_SLOT: f32 = 0.3;
const FRONT_MAX_WIDTH: f32 = 0.5;
const FRONT_MAX_HEIGHT: f32 = 0.5;
const CAMERA_FOV_DEGREES: f32 = 28.0;
/// Cap Kitty scene pixels so flip frames stay cheap (PNG+base64 bound).
/// AFK: 480×300 (~28% fewer RGB bytes than 560×360) — still sharp on laptop cells.
pub const SCENE_MAX_W: u32 = 480;
pub const SCENE_MAX_H: u32 = 300;

/// Terminal-derived colors for the Flip scene (from OSC/minimap theme).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneTheme {
    pub backdrop: (u8, u8, u8),
    pub paper: (u8, u8, u8),
    pub fg: (u8, u8, u8),
    pub title_bg: (u8, u8, u8),
    pub title_fg: (u8, u8, u8),
    pub border: (u8, u8, u8),
    pub border_hot: (u8, u8, u8),
}

impl SceneTheme {
    pub fn from_rgb(bg: (u8, u8, u8), fg: (u8, u8, u8), light: bool) -> Self {
        let paper = if light {
            (
                bg.0.saturating_add(6).min(255),
                bg.1.saturating_add(6).min(255),
                bg.2.saturating_add(4).min(255),
            )
        } else {
            (
                bg.0.saturating_add(12).min(255),
                bg.1.saturating_add(12).min(255),
                bg.2.saturating_add(16).min(255),
            )
        };
        let title_bg = if light {
            (
                bg.0.saturating_sub(18),
                bg.1.saturating_sub(16),
                bg.2.saturating_sub(20),
            )
        } else {
            (
                bg.0.saturating_add(28).min(255),
                bg.1.saturating_add(28).min(255),
                bg.2.saturating_add(36).min(255),
            )
        };
        Self {
            backdrop: bg,
            paper,
            fg,
            title_bg,
            title_fg: fg,
            border: if light {
                (140, 130, 110)
            } else {
                (90, 100, 120)
            },
            border_hot: (230, 160, 50),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FlipSwitcher {
    pub open: bool,
    pub selected: usize,
    /// Eased scroll toward `selected` (ShojiWM `state.scroll`).
    pub scroll: f32,
    /// Buffer index that was focused when the switcher opened (Esc restores).
    pub restore: usize,
    /// Dirty flag so Kitty retransmits after a flip.
    pub dirty: bool,
    /// Last Kitty placement cell box — skip delete when unchanged.
    pub placed: Option<(u16, u16, u16, u16)>,
    /// Cached last PNG so settled frames can rebind placeholders without re-encode.
    pub scene_png: Vec<u8>,
    pub scene_rgb: Vec<u8>,
}

impl Default for FlipSwitcher {
    fn default() -> Self {
        Self {
            open: false,
            selected: 0,
            scroll: 0.0,
            restore: 0,
            dirty: false,
            placed: None,
            scene_png: Vec::new(),
            scene_rgb: Vec::new(),
        }
    }
}

impl FlipSwitcher {
    /// Open on the focused buffer. Arrows change selection; bare Tab release keeps it.
    pub fn open_at(&mut self, focus: usize, count: usize) {
        let count = count.max(1);
        self.open = true;
        self.restore = focus.min(count - 1);
        self.selected = self.restore;
        self.scroll = self.selected as f32;
        self.dirty = true;
    }

    pub fn flip(&mut self, steps: i32, count: usize) {
        if !self.open || count == 0 || steps == 0 {
            return;
        }
        let count = count as i32;
        let next = (self.selected as i32 + steps).rem_euclid(count) as usize;
        self.selected = next;
        self.dirty = true;
    }

    pub fn tick(&mut self, dt_ms: f32) -> bool {
        if !self.open {
            return false;
        }
        let distance = self.selected as f32 - self.scroll;
        if distance.abs() < 0.001 {
            self.scroll = self.selected as f32;
            return false;
        }
        self.scroll += distance * (1.0 - (-dt_ms / SCROLL_TIME_CONSTANT_MS).exp());
        self.dirty = true;
        true
    }

    pub fn settle(&mut self) {
        self.scroll = self.selected as f32;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.dirty = true;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub x: f32,
    pub y: f32,
    #[allow(dead_code)]
    pub z: f32,
    pub rotate_x: f32,
    pub rotate_y: f32,
    pub scale: f32,
    pub opacity: f32,
}

fn camera_distance(height: f32) -> f32 {
    height / 2.0 / ((CAMERA_FOV_DEGREES * PI / 360.0).tan())
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn rem_euclid_f(a: f32, b: f32) -> f32 {
    let r = a % b;
    if r < 0.0 {
        r + b
    } else {
        r
    }
}

/// ShojiWM `slotOf(index, scroll, count)`.
pub fn slot_of(index: usize, scroll: f32, count: usize) -> f32 {
    let count = count.max(1) as f32;
    rem_euclid_f(index as f32 - scroll + 0.5, count) - 0.5
}

/// ShojiWM `slotPose` — front window low-right, stack recedes up-left.
pub fn slot_pose(
    slot: f32,
    count: usize,
    card_w: f32,
    card_h: f32,
    width: f32,
    height: f32,
) -> Pose {
    let fit = (1.0_f32)
        .min((width * FRONT_MAX_WIDTH) / card_w.max(1.0))
        .min((height * FRONT_MAX_HEIGHT) / card_h.max(1.0));
    let depth = slot * SHRINK_PER_SLOT * camera_distance(height);
    let depth_scale = 1.0 + slot * SHRINK_PER_SLOT;
    let last = count.min(VISIBLE_WINDOWS) as f32;
    let opacity = if slot < 0.0 {
        clamp01(1.0 + slot * 2.0)
    } else if slot > last - 1.0 {
        clamp01((last - 0.5 - slot) * 2.0)
    } else {
        1.0
    };
    Pose {
        x: (FRONT_X + slot * STEP_X) * width * depth_scale,
        y: (FRONT_Y + slot * STEP_Y) * height * depth_scale,
        z: -depth,
        rotate_x: PITCH_DEGREES,
        rotate_y: TURN_DEGREES,
        scale: fit,
        opacity,
    }
}

/// Pre-rendered syntax RGB for a Flip card (shared Arc — zero-copy across frames).
#[derive(Clone, Debug)]
pub struct PreviewImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct BufferCard {
    pub title: String,
    /// Plain lines for text-fallback hosts (no Kitty).
    pub lines: Vec<String>,
    #[allow(dead_code)]
    pub active: bool,
    /// Kitty path: minimap-style syntax glyphs. `None` until background cache fills.
    pub preview: Option<std::sync::Arc<PreviewImage>>,
}

/// Render Flip 3D scene to tightly packed RGB24 (Kitty PNG path).
///
/// Cards are painted at full scene pixel density and sample the shared
/// minimap-glyph `PreviewImage` (or theme paper + text bars as fallback).
pub fn render_scene_rgb(
    cards: &[BufferCard],
    scroll: f32,
    selected: usize,
    width: u32,
    height: u32,
    theme: SceneTheme,
) -> Vec<u8> {
    let w = width.max(2).min(SCENE_MAX_W) as usize;
    let h = height.max(2).min(SCENE_MAX_H) as usize;
    let mut rgb = vec![0u8; w * h * 3];
    for px in rgb.chunks_exact_mut(3) {
        px[0] = theme.backdrop.0;
        px[1] = theme.backdrop.1;
        px[2] = theme.backdrop.2;
    }
    if cards.is_empty() {
        return rgb;
    }
    let count = cards.len();
    let card_w = ((w as f32) * FRONT_MAX_WIDTH).max(40.0);
    let card_h = ((h as f32) * FRONT_MAX_HEIGHT).max(24.0);
    // Paint back-to-front (largest slot first).
    let mut order: Vec<(usize, f32)> = (0..count)
        .map(|i| (i, slot_of(i, scroll, count)))
        .collect();
    order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for &(index, slot) in &order {
        if slot < -0.6 || slot > VISIBLE_WINDOWS as f32 {
            continue;
        }
        let pose = slot_pose(slot, count, card_w, card_h, w as f32, h as f32);
        if pose.opacity < 0.05 {
            continue;
        }
        blit_card(
            &mut rgb,
            w,
            h,
            &cards[index],
            pose,
            card_w,
            card_h,
            index == selected,
            theme,
        );
    }
    rgb
}

fn blit_card(
    rgb: &mut [u8],
    w: usize,
    h: usize,
    card: &BufferCard,
    pose: Pose,
    card_w: f32,
    card_h: f32,
    selected: bool,
    theme: SceneTheme,
) {
    let scale = pose.scale.max(0.15);
    let cw = (card_w * scale).max(8.0);
    let ch = (card_h * scale).max(8.0);
    // Screen centre + pose offset (ShojiWM y is up; our bitmap y is down).
    let cx = (w as f32) * 0.5 + pose.x;
    let cy = (h as f32) * 0.5 - pose.y;
    let shear = (pose.rotate_y * PI / 180.0).tan() * 0.35;
    let pitch = (pose.rotate_x * PI / 180.0).sin() * 0.08;
    let alpha = pose.opacity.clamp(0.0, 1.0);
    // Full pixel density — the old cols=cw/7 grid left backdrop holes (dot look).
    let px_h = ch.ceil().max(8.0) as i32;
    let border = if selected {
        theme.border_hot
    } else {
        theme.border
    };
    let title_chars: Vec<char> = card.title.chars().collect();

    for row in 0..px_h {
        let v = row as f32 / px_h.max(1) as f32;
        let row_scale = 1.0 - v * pitch;
        let row_w = cw * row_scale;
        let y = cy - ch * 0.5 + v * ch;
        let x0 = cx - row_w * 0.5 + (v - 0.5) * cw * shear;
        let row_px = row_w.ceil().max(1.0) as i32;
        for col in 0..row_px {
            let u = col as f32 / row_px.max(1) as f32;
            let x = x0 + u * row_w;
            let (px, py) = (x.round() as isize, y.round() as isize);
            if px < 0 || py < 0 || px as usize >= w || py as usize >= h {
                continue;
            }
            let edge = row == 0 || row + 1 == px_h || col == 0 || col + 1 == row_px;
            let (r, g, b) = if edge {
                border
            } else if v < 0.12 {
                // Title bar: paper strip + ink for name glyphs (block samples).
                let ti = ((u * title_chars.len().max(1) as f32) as usize).min(title_chars.len().saturating_sub(1));
                if !title_chars.is_empty() && title_chars[ti] != ' ' && (col + row) % 2 == 0 {
                    theme.title_fg
                } else {
                    theme.title_bg
                }
            } else {
                // Body: sample shared syntax preview, else theme paper + line bars.
                let body_v = ((v - 0.12) / 0.88).clamp(0.0, 1.0);
                card_body_pixel(card, u, body_v, theme)
            };
            put_blend(rgb, w, px as usize, py as usize, r, g, b, alpha);
            if col + 1 == row_px {
                put_blend(
                    rgb,
                    w,
                    (px as usize).saturating_add(1).min(w - 1),
                    py as usize,
                    r.saturating_sub(30),
                    g.saturating_sub(30),
                    b.saturating_sub(30),
                    alpha * 0.75,
                );
            }
        }
    }
}

fn card_body_pixel(card: &BufferCard, u: f32, v: f32, theme: SceneTheme) -> (u8, u8, u8) {
    if let Some(preview) = card.preview.as_ref() {
        if preview.width > 0 && preview.height > 0 && !preview.rgb.is_empty() {
            return sample_preview(preview, u, v, theme.paper);
        }
    }
    // Fallback while PreviewPool fills: theme paper + greeked source lines.
    let line_i = (v * card.lines.len().max(1) as f32) as usize;
    if let Some(line) = card.lines.get(line_i.min(card.lines.len().saturating_sub(1))) {
        let chars: Vec<char> = line.chars().collect();
        if !chars.is_empty() {
            let ci = ((u * chars.len() as f32) as usize).min(chars.len() - 1);
            if chars[ci] != ' ' {
                return theme.fg;
            }
        }
    }
    theme.paper
}

fn sample_preview(
    preview: &PreviewImage,
    u: f32,
    v: f32,
    fallback: (u8, u8, u8),
) -> (u8, u8, u8) {
    let w = preview.width.max(1);
    let h = preview.height.max(1);
    let x = ((u.clamp(0.0, 1.0) * (w.saturating_sub(1) as f32)).round() as u32).min(w - 1);
    let y = ((v.clamp(0.0, 1.0) * (h.saturating_sub(1) as f32)).round() as u32).min(h - 1);
    let i = ((y * w + x) * 3) as usize;
    if i + 2 >= preview.rgb.len() {
        return fallback;
    }
    (preview.rgb[i], preview.rgb[i + 1], preview.rgb[i + 2])
}

fn put_blend(rgb: &mut [u8], w: usize, x: usize, y: usize, r: u8, g: u8, b: u8, a: f32) {
    let i = (y * w + x) * 3;
    if i + 2 >= rgb.len() {
        return;
    }
    let a = a.clamp(0.0, 1.0);
    rgb[i] = ((rgb[i] as f32) * (1.0 - a) + r as f32 * a) as u8;
    rgb[i + 1] = ((rgb[i + 1] as f32) * (1.0 - a) + g as f32 * a) as u8;
    rgb[i + 2] = ((rgb[i + 2] as f32) * (1.0 - a) + b as f32 * a) as u8;
}

/// Text fallback: stacked cards with Flip 3D offsets (no Kitty).
pub fn text_overlay_lines(
    cards: &[BufferCard],
    scroll: f32,
    selected: usize,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(""); height as usize];
    if cards.is_empty() || width < 10 || height < 4 {
        return lines;
    }
    let count = cards.len();
    let mut order: Vec<(usize, f32)> = (0..count)
        .map(|i| (i, slot_of(i, scroll, count)))
        .collect();
    order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (index, slot) in order {
        if slot < -0.5 || slot > 5.0 {
            continue;
        }
        let indent = ((slot.max(0.0)) * 4.0) as u16;
        let rise = ((slot.max(0.0)) * 1.2) as u16;
        let card_w = ((width as f32) * 0.55).max(20.0) as u16;
        let card_h = ((height as f32) * 0.45).max(6.0) as u16;
        let x0 = width
            .saturating_mul(55)
            .saturating_div(100)
            .saturating_sub(indent)
            .min(width.saturating_sub(card_w));
        let y0 = height
            .saturating_mul(40)
            .saturating_div(100)
            .saturating_sub(rise)
            .min(height.saturating_sub(card_h));
        let hot = index == selected;
        let border = if hot { Color::Yellow } else { Color::DarkGray };
        // ANSI colors only — host terminal remaps to the active scheme.
        let title_style = Style::default()
            .fg(if hot { Color::Yellow } else { Color::Cyan })
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        for dy in 0..card_h {
            let y = y0.saturating_add(dy) as usize;
            if y >= lines.len() {
                break;
            }
            let mut row = format!("{}┌{}┐", " ".repeat(x0 as usize), "─".repeat(card_w.saturating_sub(2) as usize));
            if dy == 0 {
                // top already set
            } else if dy + 1 == card_h {
                row = format!(
                    "{}└{}┘",
                    " ".repeat(x0 as usize),
                    "─".repeat(card_w.saturating_sub(2) as usize)
                );
            } else if dy == 1 {
                let title = truncate(&cards[index].title, card_w.saturating_sub(2) as usize);
                row = format!("{}│{title:<width$}│", " ".repeat(x0 as usize), width = card_w.saturating_sub(2) as usize);
                lines[y] = Line::from(Span::styled(row, title_style));
                continue;
            } else {
                let body = cards[index]
                    .lines
                    .get(dy as usize - 2)
                    .map(|s| truncate(s, card_w.saturating_sub(2) as usize))
                    .unwrap_or_default();
                row = format!(
                    "{}│{body:<width$}│",
                    " ".repeat(x0 as usize),
                    width = card_w.saturating_sub(2) as usize
                );
            }
            lines[y] = Line::from(Span::styled(row, Style::default().fg(border)));
        }
    }
    // Header hint — reverse video so the host theme paints the chip.
    if let Some(first) = lines.first_mut() {
        *first = Line::from(Span::styled(
            " hold Tab  ·  ←→ choose  ·  release to switch  ·  Esc cancel ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ));
    }
    lines
}

fn truncate(s: &str, width: usize) -> String {
    let mut out: String = s.chars().take(width).collect();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

pub fn paint_text_fallback(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    cards: &[BufferCard],
    scroll: f32,
    selected: usize,
) {
    // Clear to terminal default (no forced black) so Kitty/theme underlay matches.
    frame.render_widget(Clear, area);
    let lines = text_overlay_lines(cards, scroll, selected, area.width, area.height);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title("Buffers")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_of_wraps_like_shojiwm() {
        assert!((slot_of(0, 0.0, 3) - (-0.0)).abs() < 0.01 || slot_of(0, 0.0, 3).abs() < 0.6);
        let s1 = slot_of(1, 0.0, 3);
        assert!(s1 > 0.4 && s1 < 1.6, "{s1}");
    }

    #[test]
    fn open_keeps_current_buffer_until_arrows() {
        let mut sw = FlipSwitcher::default();
        sw.open_at(0, 3);
        assert!(sw.open);
        assert_eq!(sw.selected, 0);
        assert_eq!(sw.restore, 0);
        sw.flip(1, 3);
        assert_eq!(sw.selected, 1);
        sw.flip(1, 3);
        assert_eq!(sw.selected, 2);
        sw.flip(-1, 3);
        assert_eq!(sw.selected, 1);
    }

    #[test]
    fn scene_rgb_has_expected_size() {
        let cards = vec![
            BufferCard {
                title: "a.rs".into(),
                lines: vec!["fn main() {}".into()],
                active: true,
                preview: None,
            },
            BufferCard {
                title: "b.rs".into(),
                lines: vec!["let x = 1;".into()],
                active: false,
                preview: None,
            },
        ];
        let theme = SceneTheme::from_rgb((250, 245, 230), (70, 80, 55), true);
        let rgb = render_scene_rgb(&cards, 0.0, 0, 120, 80, theme);
        assert_eq!(rgb.len(), 120 * 80 * 3);
        // Light theme backdrop — not the old near-black (18,18,22).
        assert_eq!(&rgb[0..3], &[250, 245, 230]);
    }

    #[test]
    fn scene_samples_syntax_preview_rgb() {
        let mut rgb = vec![0u8; 20 * 10 * 3];
        // Paint a green pixel mid-frame so sampling must hit preview, not flat gray.
        let mid = ((5 * 20 + 10) * 3) as usize;
        rgb[mid] = 10;
        rgb[mid + 1] = 220;
        rgb[mid + 2] = 40;
        let cards = vec![BufferCard {
            title: "x.rs".into(),
            lines: vec!["***".into()],
            active: true,
            preview: Some(std::sync::Arc::new(PreviewImage {
                width: 20,
                height: 10,
                rgb,
            })),
        }];
        let theme = SceneTheme::from_rgb((30, 30, 36), (200, 200, 210), false);
        let scene = render_scene_rgb(&cards, 0.0, 0, 160, 100, theme);
        assert!(
            scene.chunks(3).any(|p| p[1] > 150 && p[0] < 80),
            "expected green preview samples in scene"
        );
    }

    #[test]
    fn scene_caps_resolution_for_fast_flip() {
        let theme = SceneTheme::from_rgb((250, 245, 230), (40, 40, 40), true);
        let rgb = render_scene_rgb(&[], 0.0, 0, 2000, 2000, theme);
        assert_eq!(
            rgb.len(),
            (SCENE_MAX_W * SCENE_MAX_H * 3) as usize,
            "oversized requests must clamp"
        );
    }

    #[test]
    fn tick_eases_toward_selection() {
        let mut sw = FlipSwitcher::default();
        sw.open_at(0, 4);
        sw.selected = 2;
        sw.scroll = 0.0;
        assert!(sw.tick(70.0));
        assert!(sw.scroll > 0.0 && sw.scroll < 2.0);
    }
}
