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
/// ShojiWM `TRANSITION_MS` — enter/leave morph duration.
pub const TRANSITION_MS: f32 = 450.0;
/// ShojiWM Flip `DIM_ALPHA` — desktop dim while the stack is shown.
pub const DIM_ALPHA: f32 = 0.45;
/// Cap Kitty scene pixels so flip frames stay cheap (PNG+base64 bound).
pub const SCENE_MAX_W: u32 = 560;
pub const SCENE_MAX_H: u32 = 360;
/// ShojiWM Flip `SCROLL_STEP_PX` — touchpad distance per window (logical px).
#[allow(dead_code)] // reserved for continuous touchpad remainder (arrows-only cycling).
pub const SCROLL_STEP_PX: f32 = 80.0;

/// ShojiWM window-switcher phase (`entering` / `shown` / `leaving`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlipPhase {
    #[default]
    Closed,
    Entering,
    Shown,
    Leaving,
}

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
    /// Mouse hover outline (purple) — never moves keyboard selection.
    pub border_hover: (u8, u8, u8),
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
            border_hot: (230, 160, 50), // orange — keyboard / click selection
            border_hover: (168, 85, 247), // purple — mouse outline only
        }
    }
}

/// How Flip was armed. Tab-hold is the only legal "open while key down" mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlipArm {
    #[default]
    None,
    /// Physical Tab is down. `open && !tab_held` is illegal → must close.
    Tab,
    /// gs / AltTab fallback (Enter commits; no Tab hold required).
    Chord,
}

#[derive(Clone, Debug)]
pub struct FlipSwitcher {
    pub open: bool,
    pub selected: usize,
    /// Eased scroll toward `selected` (ShojiWM `state.scroll`).
    pub scroll: f32,
    /// Buffer index that was focused when the switcher opened (Esc restores).
    pub restore: usize,
    /// Arm mode — Tab arm requires `tab_held` for the whole open lifetime.
    pub arm: FlipArm,
    /// True while the physical Tab that armed Flip is still down.
    pub tab_held: bool,
    /// Last Tab press/repeat while armed (silence ⇒ released when repeats stop).
    pub last_tab_pulse: Option<std::time::Instant>,
    /// Saw a Tab auto-repeat after open (enables silence-as-release).
    pub saw_tab_repeat: bool,
    /// Card under the pointer — purple outline only (does not move `selected`).
    pub hovered: Option<usize>,
    /// ShojiWM enter/shown/leave phase.
    pub phase: FlipPhase,
    /// Linear 0..1 progress of the current enter/leave (`TRANSITION_MS`).
    pub progress: f32,
    /// When leaving: `true` commits `leave_to`, `false` restores `restore`.
    pub leave_commit: bool,
    /// Focus target once leave finishes (selected on commit, restore on cancel).
    pub leave_to: usize,
    /// Pose of each card when leave started (ShojiWM `leaveFrom`).
    pub leave_from: Vec<Pose>,
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
            arm: FlipArm::None,
            tab_held: false,
            last_tab_pulse: None,
            saw_tab_repeat: false,
            hovered: None,
            phase: FlipPhase::Closed,
            progress: 0.0,
            leave_commit: true,
            leave_to: 0,
            leave_from: Vec::new(),
            dirty: false,
            placed: None,
            scene_png: Vec::new(),
            scene_rgb: Vec::new(),
        }
    }
}

impl FlipSwitcher {
    /// Open on the focused buffer.
    /// `tab_arm`: true = physical Tab hold (release must close); false = chord fallback.
    pub fn open_at(&mut self, focus: usize, count: usize, tab_arm: bool) {
        let count = count.max(1);
        self.open = true;
        self.restore = focus.min(count - 1);
        self.selected = self.restore;
        self.scroll = self.selected as f32;
        self.arm = if tab_arm { FlipArm::Tab } else { FlipArm::Chord };
        // Tab arm starts held; Chord does not claim a Tab key is down.
        self.tab_held = tab_arm;
        self.last_tab_pulse = if tab_arm {
            Some(std::time::Instant::now())
        } else {
            None
        };
        self.saw_tab_repeat = false;
        self.hovered = None;
        self.phase = FlipPhase::Entering;
        self.progress = 0.0;
        self.leave_commit = true;
        self.leave_to = self.selected;
        self.leave_from.clear();
        self.dirty = true;
    }

    /// Silence after Tab repeats before assuming a missing key-up.
    /// Must exceed slow OS key-repeat intervals (~2 Hz) and gaps while arrows
    /// steal the repeat stream (terminals only auto-repeat the last key).
    pub const TAB_HOLD_SILENCE_MS: u64 = 1_200;

    /// Tab press/repeat while open — keeps the hold alive.
    pub fn note_tab_pulse(&mut self) {
        if !self.open || self.arm != FlipArm::Tab {
            return;
        }
        if self.last_tab_pulse.is_some() {
            self.saw_tab_repeat = true;
        }
        self.last_tab_pulse = Some(std::time::Instant::now());
        self.tab_held = true;
    }

    /// Any Flip interaction while Tab is still armed (arrows / hover).
    /// Keeps the hold alive when Tab auto-repeat pauses for other keys.
    pub fn note_hold_activity(&mut self) {
        if !self.open || self.arm != FlipArm::Tab || !self.tab_held {
            return;
        }
        self.last_tab_pulse = Some(std::time::Instant::now());
    }

    /// Mark Tab up. Returns true when the switcher must close (commit).
    pub fn note_tab_release(&mut self) -> bool {
        if !self.open || self.arm != FlipArm::Tab {
            return false;
        }
        self.tab_held = false;
        true
    }

    /// Law mirror: Tab-armed Flip open without Tab held is illegal.
    /// Leaving already released Tab — not a violation (ShojiWM close morph).
    pub fn tab_arm_violated(&self) -> bool {
        self.open
            && self.arm == FlipArm::Tab
            && !self.tab_held
            && matches!(self.phase, FlipPhase::Entering | FlipPhase::Shown)
    }

    /// Silence after repeats ⇒ Tab was released without a key-up event.
    /// Only a Press-only-host fallback — real TabRelease always wins.
    pub fn tab_hold_timed_out(&self) -> bool {
        if !self.open || self.arm != FlipArm::Tab || !self.tab_held {
            return false;
        }
        if !self.saw_tab_repeat {
            return false;
        }
        self.last_tab_pulse.is_some_and(|at| {
            at.elapsed() >= std::time::Duration::from_millis(Self::TAB_HOLD_SILENCE_MS)
        })
    }

    pub fn flip(&mut self, steps: i32, count: usize) {
        // Arrows only while shown/entering — ignore during leave morph.
        if !self.open || count == 0 || steps == 0 || self.phase == FlipPhase::Leaving {
            return;
        }
        // Arrow keys usually stop Tab auto-repeat — treat navigation as hold proof.
        self.note_hold_activity();
        let count = count as i32;
        let next = (self.selected as i32 + steps).rem_euclid(count) as usize;
        self.selected = next;
        self.dirty = true;
    }

    pub fn tick(&mut self, dt_ms: f32) -> bool {
        if !self.open {
            return false;
        }
        let mut moving = false;
        if matches!(self.phase, FlipPhase::Entering | FlipPhase::Leaving) {
            self.progress = (self.progress + dt_ms / TRANSITION_MS).min(1.0);
            self.dirty = true;
            moving = true;
            if self.phase == FlipPhase::Entering && self.progress >= 1.0 {
                self.phase = FlipPhase::Shown;
                self.progress = 1.0;
            }
        }
        let distance = self.selected as f32 - self.scroll;
        if distance.abs() < 0.001 {
            self.scroll = self.selected as f32;
        } else {
            self.scroll += distance * (1.0 - (-dt_ms / SCROLL_TIME_CONSTANT_MS).exp());
            self.dirty = true;
            moving = true;
        }
        moving
    }

    /// True when the leave morph has finished and focus should hand off.
    pub fn leave_finished(&self) -> bool {
        self.open && self.phase == FlipPhase::Leaving && self.progress >= 1.0
    }

    pub fn settle(&mut self) {
        self.scroll = self.selected as f32;
    }

    /// Begin ShojiWM leave morph toward `to` (commit) or restore (cancel).
    pub fn begin_leave(&mut self, commit: bool, count: usize, scene_w: f32, scene_h: f32) {
        if !self.open || self.phase == FlipPhase::Leaving {
            return;
        }
        let count = count.max(1);
        self.settle();
        let eased = window_ease(self.progress);
        let card_w = (scene_w * FRONT_MAX_WIDTH).max(40.0);
        let card_h = (scene_h * FRONT_MAX_HEIGHT).max(24.0);
        // Capture the live pose (incl. mid-enter) — ShojiWM `leaveFrom`.
        self.leave_from = (0..count)
            .map(|i| {
                let shown = slot_pose(
                    slot_of(i, self.scroll, count),
                    count,
                    card_w,
                    card_h,
                    scene_w,
                    scene_h,
                );
                if self.phase == FlipPhase::Entering {
                    lerp_pose(
                        desktop_pose(i, self.restore, scene_w, scene_h),
                        shown,
                        eased,
                    )
                } else {
                    shown
                }
            })
            .collect();
        self.leave_commit = commit;
        self.leave_to = if commit {
            self.commit_focus(self.selected).min(count - 1)
        } else {
            self.cancel_focus(self.restore).min(count - 1)
        };
        self.phase = FlipPhase::Leaving;
        self.progress = 0.0;
        self.arm = FlipArm::None;
        self.tab_held = false;
        self.last_tab_pulse = None;
        self.saw_tab_repeat = false;
        self.hovered = None;
        self.dirty = true;
    }

    /// Synchronous close — clears hold tracking so a late key-up is a no-op.
    pub fn close(&mut self) {
        self.open = false;
        self.arm = FlipArm::None;
        self.tab_held = false;
        self.last_tab_pulse = None;
        self.saw_tab_repeat = false;
        self.hovered = None;
        self.phase = FlipPhase::Closed;
        self.progress = 0.0;
        self.leave_from.clear();
        self.dirty = true;
    }

    /// Pure commit target (Bend-mirrored): selected while open, else unchanged focus.
    pub fn commit_focus(&self, current_focus: usize) -> usize {
        if self.open {
            self.selected
        } else {
            current_focus
        }
    }

    /// Pure cancel target (Bend-mirrored): restore while open, else unchanged focus.
    pub fn cancel_focus(&self, current_focus: usize) -> usize {
        if self.open {
            self.restore
        } else {
            current_focus
        }
    }

    /// Pointer over a card: purple outline only — never moves orange selection.
    pub fn hover_outline(&mut self, index: usize, count: usize) {
        if !self.open || count == 0 || self.phase == FlipPhase::Leaving {
            return;
        }
        self.note_hold_activity();
        let next = index.min(count - 1);
        if self.hovered != Some(next) {
            self.hovered = Some(next);
            self.dirty = true;
        }
    }

    /// Clear hover highlight when the pointer leaves every card.
    pub fn hover_clear(&mut self) {
        if self.hovered.take().is_some() {
            self.dirty = true;
        }
    }

    /// Click selects (orange) then the caller begins leave/commit.
    pub fn click_select(&mut self, index: usize, count: usize) {
        if !self.open || count == 0 || self.phase == FlipPhase::Leaving {
            return;
        }
        self.selected = index.min(count - 1);
        self.hovered = None;
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

/// ShojiWM `WINDOW_EASE` = `cubic-bezier(0.1, 0.9, 0.2, 1)`.
pub fn window_ease(t: f32) -> f32 {
    cubic_bezier_ease(t.clamp(0.0, 1.0), 0.1, 0.9, 0.2, 1.0)
}

fn cubic_bezier_ease(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |t: f32| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f32| ((ay * t + by) * t + cy) * t;
    let sample_dx = |t: f32| (3.0 * ax * t + 2.0 * bx) * t + cx;
    let mut t = x;
    for _ in 0..8 {
        let err = sample_x(t) - x;
        if err.abs() < 1e-6 {
            return sample_y(t);
        }
        let d = sample_dx(t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= err / d;
    }
    let mut lo = 0.0;
    let mut hi = 1.0;
    t = x;
    for _ in 0..12 {
        let x_est = sample_x(t);
        if (x_est - x).abs() < 1e-6 {
            return sample_y(t);
        }
        if x > x_est {
            lo = t;
        } else {
            hi = t;
        }
        t = (hi + lo) * 0.5;
    }
    sample_y(t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp_pose(from: Pose, to: Pose, t: f32) -> Pose {
    Pose {
        x: lerp(from.x, to.x, t),
        y: lerp(from.y, to.y, t),
        z: lerp(from.z, to.z, t),
        rotate_x: lerp(from.rotate_x, to.rotate_x, t),
        rotate_y: lerp(from.rotate_y, to.rotate_y, t),
        scale: lerp(from.scale, to.scale, t),
        opacity: lerp(from.opacity, to.opacity, t),
    }
}

/// Approximate ShojiWM `realPose` for the editor: focused buffer fills the
/// scene flat; others fade in from behind.
pub fn desktop_pose(index: usize, focus: usize, scene_w: f32, scene_h: f32) -> Pose {
    let card_w = (scene_w * FRONT_MAX_WIDTH).max(40.0);
    let card_h = (scene_h * FRONT_MAX_HEIGHT).max(24.0);
    // Cover ~90% of the scene (editor viewport morph).
    let fit = (1.0_f32)
        .min((scene_w * 0.92) / card_w.max(1.0))
        .min((scene_h * 0.88) / card_h.max(1.0));
    if index == focus {
        Pose {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            rotate_x: 0.0,
            rotate_y: 0.0,
            scale: fit,
            opacity: 1.0,
        }
    } else {
        Pose {
            x: 0.0,
            y: 0.0,
            z: -scene_h * 0.3,
            rotate_x: 0.0,
            rotate_y: 0.0,
            scale: fit * 0.9,
            opacity: 0.0,
        }
    }
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

/// ShojiWM `pickPlane` / `cardAt` — frontmost card under a scene-local point.
///
/// Coordinates match `render_scene_rgb` / `blit_card` (origin top-left, +y down).
/// Returns the buffer index, or `None` for empty backdrop (click-outside).
pub fn hit_test(
    count: usize,
    scroll: f32,
    scene_w: f32,
    scene_h: f32,
    x: f32,
    y: f32,
) -> Option<usize> {
    if count == 0 || scene_w < 8.0 || scene_h < 6.0 {
        return None;
    }
    let card_w = (scene_w * FRONT_MAX_WIDTH).max(40.0);
    let card_h = (scene_h * FRONT_MAX_HEIGHT).max(24.0);
    // Front-to-back: smallest slot first (ShojiWM pickPlane order).
    let mut order: Vec<(usize, f32)> = (0..count)
        .map(|i| (i, slot_of(i, scroll, count)))
        .collect();
    order.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    for &(index, slot) in &order {
        if slot < -0.6 || slot > VISIBLE_WINDOWS as f32 {
            continue;
        }
        let pose = slot_pose(slot, count, card_w, card_h, scene_w, scene_h);
        if pose.opacity < 0.05 {
            continue;
        }
        if point_in_card(pose, card_w, card_h, scene_w, scene_h, x, y) {
            return Some(index);
        }
    }
    None
}

/// Same shear/pitch footprint as `blit_card` (terminal 2D projection of the plane).
fn point_in_card(
    pose: Pose,
    card_w: f32,
    card_h: f32,
    scene_w: f32,
    scene_h: f32,
    x: f32,
    y: f32,
) -> bool {
    let scale = pose.scale.max(0.15);
    let cw = (card_w * scale).max(8.0);
    let ch = (card_h * scale).max(8.0);
    let cx = scene_w * 0.5 + pose.x;
    let cy = scene_h * 0.5 - pose.y;
    let shear = (pose.rotate_y * PI / 180.0).tan() * 0.35;
    let pitch = (pose.rotate_x * PI / 180.0).sin() * 0.08;
    let top = cy - ch * 0.5;
    let bottom = cy + ch * 0.5;
    if y < top || y > bottom {
        return false;
    }
    let v = ((y - top) / ch.max(1.0)).clamp(0.0, 1.0);
    let row_scale = 1.0 - v * pitch;
    let row_w = cw * row_scale;
    let x0 = cx - row_w * 0.5 + (v - 0.5) * cw * shear;
    x >= x0 && x <= x0 + row_w
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

/// Settled Flip scene (no hover / morph) — test helper wrapping
/// [`render_scene_rgb_hover`]. Production paint always passes phase + hover.
#[cfg(test)]
pub fn render_scene_rgb(
    cards: &[BufferCard],
    scroll: f32,
    selected: usize,
    width: u32,
    height: u32,
    theme: SceneTheme,
) -> Vec<u8> {
    render_scene_rgb_hover(
        cards,
        scroll,
        selected,
        None,
        width,
        height,
        theme,
        FlipPhase::Shown,
        1.0,
        selected,
        selected,
        &[],
    )
}

/// Like `render_scene_rgb`, with hover outline + ShojiWM enter/leave morph.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_rgb_hover(
    cards: &[BufferCard],
    scroll: f32,
    selected: usize,
    hovered: Option<usize>,
    width: u32,
    height: u32,
    theme: SceneTheme,
    phase: FlipPhase,
    progress: f32,
    restore: usize,
    leave_to: usize,
    leave_from: &[Pose],
) -> Vec<u8> {
    let w = width.max(2).min(SCENE_MAX_W) as usize;
    let h = height.max(2).min(SCENE_MAX_H) as usize;
    let eased = window_ease(progress.clamp(0.0, 1.0));
    let dim = match phase {
        FlipPhase::Entering => DIM_ALPHA * eased,
        FlipPhase::Shown => DIM_ALPHA,
        FlipPhase::Leaving => DIM_ALPHA * (1.0 - eased),
        FlipPhase::Closed => 0.0,
    };
    let mut rgb = vec![0u8; w * h * 3];
    for px in rgb.chunks_exact_mut(3) {
        // Dim desktop behind the stack (ShojiWM dimAlpha).
        px[0] = ((theme.backdrop.0 as f32) * (1.0 - dim * 0.85)) as u8;
        px[1] = ((theme.backdrop.1 as f32) * (1.0 - dim * 0.85)) as u8;
        px[2] = ((theme.backdrop.2 as f32) * (1.0 - dim * 0.85)) as u8;
    }
    if cards.is_empty() {
        return rgb;
    }
    let count = cards.len();
    let card_w = ((w as f32) * FRONT_MAX_WIDTH).max(40.0);
    let card_h = ((h as f32) * FRONT_MAX_HEIGHT).max(24.0);
    // Paint back-to-front (largest slot / lowest opacity first).
    let mut order: Vec<(usize, f32)> = (0..count)
        .map(|i| (i, slot_of(i, scroll, count)))
        .collect();
    order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for &(index, slot) in &order {
        let shown = slot_pose(slot, count, card_w, card_h, w as f32, h as f32);
        let pose = match phase {
            FlipPhase::Entering => {
                let from = desktop_pose(index, restore, w as f32, h as f32);
                lerp_pose(from, shown, eased)
            }
            FlipPhase::Shown | FlipPhase::Closed => shown,
            FlipPhase::Leaving => {
                let from = leave_from.get(index).copied().unwrap_or(shown);
                let to = if index == leave_to {
                    desktop_pose(index, leave_to, w as f32, h as f32)
                } else {
                    Pose {
                        x: from.x,
                        y: from.y,
                        z: from.z - (h as f32) * 0.15,
                        rotate_x: from.rotate_x,
                        rotate_y: from.rotate_y,
                        scale: from.scale * 0.85,
                        opacity: 0.0,
                    }
                };
                lerp_pose(from, to, eased)
            }
        };
        if pose.opacity < 0.05 {
            continue;
        }
        // During leave/enter morph, skip the slot-visibility cull so desktop
        // poses (slot may be large) still draw.
        if matches!(phase, FlipPhase::Shown)
            && (slot < -0.6 || slot > VISIBLE_WINDOWS as f32)
        {
            continue;
        }
        let border = if index == selected {
            theme.border_hot
        } else if hovered == Some(index) {
            theme.border_hover
        } else {
            theme.border
        };
        blit_card(
            &mut rgb,
            w,
            h,
            &cards[index],
            pose,
            card_w,
            card_h,
            border,
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
    border: (u8, u8, u8),
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
    // Prefer the shared PreviewPool Arc (title bar is stamped into the RGB).
    let has_preview = card
        .preview
        .as_ref()
        .is_some_and(|p| p.width > 0 && p.height > 0 && !p.rgb.is_empty());

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
            } else if has_preview {
                // Zero-copy sample of the same Arc minimap glyph pipeline uses.
                sample_preview(card.preview.as_ref().unwrap(), u, v, theme.paper)
            } else if v < 0.14 {
                title_bar_pixel(&card.title, u, v / 0.14, theme)
            } else {
                let body_v = ((v - 0.14) / 0.86).clamp(0.0, 1.0);
                card_body_fallback(card, u, body_v, theme)
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

/// 5×7 glyph title bar (readable buffer name when PreviewPool is cold).
fn title_bar_pixel(title: &str, u: f32, v: f32, theme: SceneTheme) -> (u8, u8, u8) {
    const SCALE: usize = 2;
    const GW: usize = 5 * SCALE;
    const GH: usize = 7 * SCALE;
    const GAP: usize = SCALE;
    let chars: Vec<char> = title.chars().take(28).collect();
    if chars.is_empty() {
        return theme.title_bg;
    }
    let total_w = chars.len() * (GW + GAP) + GAP * 2;
    let x = ((u.clamp(0.0, 1.0) * total_w as f32) as usize).min(total_w.saturating_sub(1));
    let y = ((v.clamp(0.0, 1.0) * (GH + 4) as f32) as usize).min(GH + 3);
    if y < 2 || y >= GH + 2 {
        return theme.title_bg;
    }
    let gy = y - 2;
    if x < GAP {
        return theme.title_bg;
    }
    let local = x - GAP;
    let cell = GW + GAP;
    let ci = local / cell;
    let ox = local % cell;
    if ci >= chars.len() || ox >= GW {
        return theme.title_bg;
    }
    let rows = crate::minimap_gfx::glyph_rows(chars[ci]).unwrap_or(&[0x1F; 7]);
    let row = gy / SCALE;
    let bit = 4 - (ox / SCALE);
    if row < 7 && (rows[row] >> bit) & 1 == 1 {
        theme.title_fg
    } else {
        theme.title_bg
    }
}

fn card_body_fallback(card: &BufferCard, u: f32, v: f32, theme: SceneTheme) -> (u8, u8, u8) {
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
    // Bilinear — nearest-neighbour on sheared Flip cards looked like TV static.
    let fx = u.clamp(0.0, 1.0) * (w.saturating_sub(1) as f32);
    let fy = v.clamp(0.0, 1.0) * (h.saturating_sub(1) as f32);
    let x0 = fx.floor() as u32;
    let y0 = fy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let pix = |x: u32, y: u32| -> (f32, f32, f32) {
        let i = ((y * w + x) * 3) as usize;
        if i + 2 >= preview.rgb.len() {
            return (fallback.0 as f32, fallback.1 as f32, fallback.2 as f32);
        }
        (
            preview.rgb[i] as f32,
            preview.rgb[i + 1] as f32,
            preview.rgb[i + 2] as f32,
        )
    };
    let (r00, g00, b00) = pix(x0, y0);
    let (r10, g10, b10) = pix(x1, y0);
    let (r01, g01, b01) = pix(x0, y1);
    let (r11, g11, b11) = pix(x1, y1);
    let r0 = r00 + (r10 - r00) * tx;
    let g0 = g00 + (g10 - g00) * tx;
    let b0 = b00 + (b10 - b00) * tx;
    let r1 = r01 + (r11 - r01) * tx;
    let g1 = g01 + (g11 - g01) * tx;
    let b1 = b01 + (b11 - b01) * tx;
    (
        (r0 + (r1 - r0) * ty) as u8,
        (g0 + (g1 - g0) * ty) as u8,
        (b0 + (b1 - b0) * ty) as u8,
    )
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
            " hold Tab  ·  ←→ cycle  ·  hover outlines  ·  release/click switches  ·  Esc ",
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
        sw.open_at(0, 3, true);
        assert!(sw.open);
        assert_eq!(sw.arm, FlipArm::Tab);
        assert!(sw.tab_held);
        assert_eq!(sw.selected, 0);
        assert_eq!(sw.restore, 0);
        sw.flip(1, 3);
        assert_eq!(sw.selected, 1);
        sw.flip(1, 3);
        assert_eq!(sw.selected, 2);
        sw.flip(-1, 3);
        assert_eq!(sw.selected, 1);
        assert_eq!(sw.commit_focus(0), 1);
        sw.close();
        assert!(!sw.tab_held);
        assert!(!sw.open);
    }

    #[test]
    fn bend_flip_release_commits_selected() {
        // Mirrors bend/flip law release_commits_selected.
        let mut sw = FlipSwitcher::default();
        sw.open_at(0, 3, true);
        assert_eq!(sw.arm, FlipArm::Tab);
        assert!(sw.tab_held);
        sw.flip(1, 3);
        assert!(sw.note_tab_release());
        assert!(sw.tab_arm_violated());
        assert_eq!(sw.commit_focus(0), 1);
        sw.close();
        assert!(!sw.open);
        assert!(!sw.tab_arm_violated());
    }

    #[test]
    fn bend_flip_open_tab_holds_invariant() {
        // Mirrors bend/flip law open_tab_holds_invariant.
        let mut sw = FlipSwitcher::default();
        sw.open_at(2, 4, true);
        assert!(sw.open && sw.tab_held && sw.arm == FlipArm::Tab);
        assert!(!sw.tab_arm_violated());
        sw.tab_held = false;
        assert!(sw.tab_arm_violated(), "open without Tab held is illegal");
    }

    #[test]
    fn arrow_activity_refreshes_tab_hold_silence_clock() {
        // Holding Tab + pressing arrows stops Tab auto-repeat on most hosts.
        // Navigation must keep the hold alive so Flip does not auto-commit.
        let mut sw = FlipSwitcher::default();
        sw.open_at(0, 3, true);
        sw.note_tab_pulse(); // marks saw_tab_repeat
        assert!(sw.saw_tab_repeat);
        sw.last_tab_pulse = Some(
            std::time::Instant::now()
                - std::time::Duration::from_millis(FlipSwitcher::TAB_HOLD_SILENCE_MS + 50),
        );
        // Stale pulse alone would time out…
        assert!(sw.tab_hold_timed_out());
        // …but an arrow refreshes it.
        sw.flip(1, 3);
        assert!(!sw.tab_hold_timed_out());
        assert_eq!(sw.selected, 1);
        assert!(sw.tab_held);
    }

    #[test]
    fn tab_hold_silence_exceeds_slow_key_repeat() {
        assert!(
            FlipSwitcher::TAB_HOLD_SILENCE_MS >= 1_000,
            "200ms silence closed Flip between slow OS Tab repeats"
        );
    }

    #[test]
    fn hit_test_finds_front_card_and_misses_backdrop() {
        // Scene 160×100 — front card centres around mid-right (FRONT_X).
        let count = 3usize;
        let scroll = 0.0_f32;
        let hit = hit_test(count, scroll, 160.0, 100.0, 100.0, 55.0);
        assert_eq!(hit, Some(0), "front card under centre-right");
        assert_eq!(
            hit_test(count, scroll, 160.0, 100.0, 2.0, 2.0),
            None,
            "corner backdrop is outside every plane"
        );
    }

    #[test]
    fn hover_outline_does_not_move_selection() {
        let mut sw = FlipSwitcher::default();
        sw.open_at(0, 4, true);
        sw.phase = FlipPhase::Shown;
        sw.hover_outline(2, 4);
        assert_eq!(sw.selected, 0, "hover must not cycle selection");
        assert_eq!(sw.hovered, Some(2));
        sw.hover_clear();
        assert_eq!(sw.hovered, None);
        assert_eq!(sw.selected, 0, "clearing hover keeps selection");
    }

    #[test]
    fn enter_leave_morph_runs_transition_ms() {
        let mut sw = FlipSwitcher::default();
        sw.open_at(0, 3, true);
        assert_eq!(sw.phase, FlipPhase::Entering);
        assert!(sw.tick(TRANSITION_MS));
        assert_eq!(sw.phase, FlipPhase::Shown);
        sw.flip(1, 3);
        sw.begin_leave(true, 3, 320.0, 200.0);
        assert_eq!(sw.phase, FlipPhase::Leaving);
        assert_eq!(sw.leave_to, 1);
        assert!(!sw.leave_finished());
        let _ = sw.tick(TRANSITION_MS);
        assert!(sw.leave_finished());
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
        // Shown phase dims the desktop (ShojiWM DIM_ALPHA) — not the old near-black.
        let dim = 1.0 - DIM_ALPHA * 0.85;
        assert_eq!(rgb[0], (250.0 * dim) as u8);
        assert_eq!(rgb[1], (245.0 * dim) as u8);
        assert_eq!(rgb[2], (230.0 * dim) as u8);
    }

    #[test]
    fn title_bar_paints_readable_glyphs_not_dither() {
        let theme = SceneTheme::from_rgb((250, 245, 230), (70, 80, 55), true);
        let mut ink = 0usize;
        let mut bg = 0usize;
        for step in 0..64 {
            let u = step as f32 / 63.0;
            let px = title_bar_pixel("flake.nix", u, 0.5, theme);
            if px == theme.title_fg {
                ink += 1;
            } else if px == theme.title_bg {
                bg += 1;
            }
        }
        assert!(ink >= 8, "expected shaped glyph ink, got {ink}");
        assert!(bg >= 8, "expected title bar padding/gaps, got {bg}");
        // Dither would alternate nearly 50/50 every other sample — shaped glyphs
        // leave long runs of bg between letters.
        let mut runs = 0usize;
        let mut prev = title_bar_pixel("flake.nix", 0.0, 0.5, theme);
        for step in 1..64 {
            let u = step as f32 / 63.0;
            let px = title_bar_pixel("flake.nix", u, 0.5, theme);
            if px != prev {
                runs += 1;
                prev = px;
            }
        }
        assert!(
            runs < 40,
            "dither noise would flip constantly; shaped glyphs have fewer runs ({runs})"
        );
    }

    #[test]
    fn scene_reuses_preview_arc_bytes_without_copy() {
        let mut preview_rgb = vec![240u8; 32 * 24 * 3];
        // Distinct cyan body block (bilinear sampling must still read cyan).
        for y in 10..16 {
            for x in 14..20 {
                let i = ((y * 32 + x) * 3) as usize;
                preview_rgb[i] = 20;
                preview_rgb[i + 1] = 180;
                preview_rgb[i + 2] = 200;
            }
        }
        let preview = std::sync::Arc::new(PreviewImage {
            width: 32,
            height: 24,
            rgb: preview_rgb,
        });
        let cards = vec![BufferCard {
            title: "shared.rs".into(),
            lines: vec!["should not show".into()],
            active: true,
            preview: Some(std::sync::Arc::clone(&preview)),
        }];
        let theme = SceneTheme::from_rgb((250, 245, 230), (70, 80, 55), true);
        let rgb = render_scene_rgb(&cards, 0.0, 0, 160, 100, theme);
        // Card body must sample the Arc (cyan), not the greeked fallback text color.
        assert!(
            rgb.chunks(3).any(|p| p[0] < 40 && p[1] > 150 && p[2] > 170),
            "scene must sample shared PreviewImage Arc"
        );
        assert_eq!(std::sync::Arc::strong_count(&preview), 2);
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
        sw.open_at(0, 4, true);
        sw.selected = 2;
        sw.scroll = 0.0;
        assert!(sw.tick(70.0));
        assert!(sw.scroll > 0.0 && sw.scroll < 2.0);
    }
}
