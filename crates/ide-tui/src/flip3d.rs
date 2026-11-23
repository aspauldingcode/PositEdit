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
const DIM: u8 = 40;

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
}

impl Default for FlipSwitcher {
    fn default() -> Self {
        Self {
            open: false,
            selected: 0,
            scroll: 0.0,
            restore: 0,
            dirty: false,
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
pub fn render_scene_rgb(
    cards: &[BufferCard],
    scroll: f32,
    selected: usize,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let w = width.max(2) as usize;
    let h = height.max(2) as usize;
    let mut rgb = vec![DIM; w * h * 3];
    // Dim backdrop
    for px in rgb.chunks_exact_mut(3) {
        px[0] = 18;
        px[1] = 18;
        px[2] = 22;
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
    let cols = (cw / 7.0).max(8.0) as usize;
    let rows = (ch / 12.0).max(4.0) as usize;

    for row in 0..rows {
        let v = row as f32 / rows.max(1) as f32;
        let row_scale = 1.0 - v * pitch;
        let row_w = cw * row_scale;
        let y = cy - ch * 0.5 + v * ch;
        let x0 = cx - row_w * 0.5 + (v - 0.5) * cw * shear;
        for col in 0..cols {
            let u = col as f32 / cols.max(1) as f32;
            let x = x0 + u * row_w;
            let (px, py) = (x.round() as isize, y.round() as isize);
            if px < 0 || py < 0 || px as usize >= w || py as usize >= h {
                continue;
            }
            let (r, g, b) = card_pixel(card, row, rows, col, cols, selected);
            put_blend(rgb, w, px as usize, py as usize, r, g, b, alpha);
            // Slight thickness for the 3D edge.
            if col + 1 == cols {
                put_blend(
                    rgb,
                    w,
                    (px as usize).saturating_add(1).min(w - 1),
                    py as usize,
                    r.saturating_sub(40),
                    g.saturating_sub(40),
                    b.saturating_sub(40),
                    alpha * 0.8,
                );
            }
        }
    }
}

fn card_pixel(
    card: &BufferCard,
    row: usize,
    rows: usize,
    col: usize,
    cols: usize,
    selected: bool,
) -> (u8, u8, u8) {
    let border = selected;
    if row == 0 || row + 1 == rows || col == 0 || col + 1 == cols {
        return if border {
            (230, 180, 60)
        } else {
            (90, 100, 120)
        };
    }
    if row == 1 {
        // Title bar.
        let title: Vec<char> = card.title.chars().collect();
        let i = col.saturating_sub(1);
        if i < title.len() {
            return (220, 220, 230);
        }
        return (45, 50, 65);
    }
    // Prefer pre-rendered syntax preview (minimap glyphs + TokenKind colors).
    if let Some(preview) = card.preview.as_ref() {
        if preview.width > 0 && preview.height > 0 && !preview.rgb.is_empty() {
            let body_rows = rows.saturating_sub(3).max(1);
            let body_cols = cols.saturating_sub(2).max(1);
            let br = row.saturating_sub(2).min(body_rows.saturating_sub(1));
            let bc = col.saturating_sub(1).min(body_cols.saturating_sub(1));
            let u = (bc as f32 + 0.5) / body_cols as f32;
            let v = (br as f32 + 0.5) / body_rows as f32;
            return sample_preview(preview, u, v);
        }
    }
    let line_i = row.saturating_sub(2);
    if let Some(line) = card.lines.get(line_i) {
        let chars: Vec<char> = line.chars().collect();
        let i = col.saturating_sub(1);
        if i < chars.len() && chars[i] != ' ' {
            return (200, 205, 215);
        }
    }
    (28, 30, 38)
}

fn sample_preview(preview: &PreviewImage, u: f32, v: f32) -> (u8, u8, u8) {
    let w = preview.width.max(1);
    let h = preview.height.max(1);
    let x = ((u.clamp(0.0, 1.0) * (w.saturating_sub(1) as f32)).round() as u32).min(w - 1);
    let y = ((v.clamp(0.0, 1.0) * (h.saturating_sub(1) as f32)).round() as u32).min(h - 1);
    let i = ((y * w + x) * 3) as usize;
    if i + 2 >= preview.rgb.len() {
        return (28, 30, 38);
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
        let border = if hot { Color::Yellow } else { Color::Gray };
        let title_style = Style::default()
            .fg(if hot { Color::Black } else { Color::White })
            .bg(if hot { Color::Yellow } else { Color::DarkGray })
            .add_modifier(Modifier::BOLD);
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
    // Header hint
    if let Some(first) = lines.first_mut() {
        *first = Line::from(Span::styled(
            " hold Tab  ·  ←→ choose  ·  release to switch  ·  Esc cancel ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
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
        let rgb = render_scene_rgb(&cards, 0.0, 0, 120, 80);
        assert_eq!(rgb.len(), 120 * 80 * 3);
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
        let scene = render_scene_rgb(&cards, 0.0, 0, 160, 100);
        assert!(
            scene.chunks(3).any(|p| p[1] > 150 && p[0] < 80),
            "expected green preview samples in scene"
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
