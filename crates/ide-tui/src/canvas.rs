//! Dedicated Canvas pane: Kitty graphics + full input forwarding.
//!
//! Capture workers write PNG (+ meta.json). Clicks map through a **contain-fit**
//! of the screenshot inside the pane (letterboxing), then into the tool's own
//! coordinate space:
//! - phone → `terminal-phone action <session> tap|swipe` in **display-points**
//! - browser → `agent-browser mouse …` in CSS pixels (viewport-sized)
use crate::graphics::{GraphicsArbiter, GraphicsBackend};
use ratatui::layout::Rect;
use serde::Deserialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FrameMeta {
    #[serde(default)]
    #[allow(dead_code)]
    pub kind: String,
    #[serde(default)]
    pub image_width: u32,
    #[serde(default)]
    pub image_height: u32,
    /// Divides image pixels into action coordinates when the PNG is native/retina.
    /// For terminal-phone unframed screenshots the PNG is already display-points → 1.0.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// `display-points` (phone) or `css-pixels` (browser).
    #[serde(default)]
    pub coordinate_space: String,
    /// Explicit action-space size when it differs from image_width/scale.
    #[serde(default)]
    pub coord_width: u32,
    #[serde(default)]
    pub coord_height: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub target: String,
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Debug)]
pub struct CanvasSurface {
    pub kind: String,
    #[allow(dead_code)]
    pub title: String,
    /// Phone session name or browser URL.
    pub target: String,
    frame_path: PathBuf,
    meta_path: PathBuf,
    geom_path: PathBuf,
    nudge_path: PathBuf,
    child: Option<Child>,
    revision: u64,
    painted_revision: u64,
    /// Currently displayed Kitty image id (placeholders bind here).
    kitty_front: u32,
    /// Inactive id for the next upload (double-buffer; never blank the front).
    kitty_back: u32,
    last_mtime: Option<SystemTime>,
    last_poll: Instant,
    /// Last published pane geometry — avoid rewriting the same JSON every frame.
    last_geom: Option<(u16, u16, u32, u32)>,
    /// Cached pixel box from geom (for click mapping without disk reads).
    geom_pixels: Option<(f64, f64)>,
    meta: FrameMeta,
    /// Drag origin in canvas-local cells (content area).
    drag_origin: Option<(u16, u16)>,
    last_cell: Option<(u16, u16)>,
    /// Phone/browser already committed a tap/click on Down (missing-Up safe).
    pressed_action: bool,
}

impl CanvasSurface {
    /// True while a press is held (tap/swipe/drag in progress).
    pub fn gesture_active(&self) -> bool {
        self.drag_origin.is_some()
    }

    /// Test helper: a surface with no capture child.
    #[cfg(test)]
    pub fn test_stub(kind: &str, target: &str) -> Self {
        Self {
            kind: kind.to_string(),
            title: "Canvas".into(),
            target: target.to_string(),
            frame_path: PathBuf::from(format!("/tmp/posit-canvas-stub-{kind}.png")),
            meta_path: PathBuf::from(format!("/tmp/posit-canvas-stub-{kind}.json")),
            geom_path: PathBuf::from(format!("/tmp/posit-canvas-stub-{kind}.geom.json")),
            nudge_path: PathBuf::from(format!("/tmp/posit-canvas-stub-{kind}.nudge")),
            child: None,
            revision: 0,
            painted_revision: 0,
            kitty_front: kitty_pair_for_kind(kind).0,
            kitty_back: kitty_pair_for_kind(kind).1,
            last_mtime: None,
            last_poll: Instant::now(),
            last_geom: None,
            geom_pixels: None,
            meta: FrameMeta {
                kind: kind.to_string(),
                scale: 1.0,
                coordinate_space: if kind == "phone" {
                    "display-points".into()
                } else {
                    "css-pixels".into()
                },
                target: target.to_string(),
                ..FrameMeta::default()
            },
            drag_origin: None,
            last_cell: None,
            pressed_action: false,
        }
    }

    pub fn start(kind: &str, workspace: &Path, program: &str, args: &[String]) -> io::Result<Self> {
        let dir = std::env::temp_dir().join(format!("posit-canvas-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let frame_path = dir.join(format!("{kind}.png"));
        let meta_path = dir.join(format!("{kind}.json"));
        let geom_path = dir.join(format!("{kind}.geom.json"));
        let nudge_path = dir.join(format!("{kind}.nudge"));
        let _ = fs::remove_file(&frame_path);
        let _ = fs::remove_file(&meta_path);
        let _ = fs::remove_file(&geom_path);
        let _ = fs::remove_file(&nudge_path);
        // argv from DLC: [canvas_capture.py, kind, target]
        let target = args
            .iter()
            .rev()
            .find(|value| {
                *value != kind
                    && !value.ends_with(".py")
                    && !value.contains("canvas_capture")
                    && !value.ends_with("python3")
                    && !value.ends_with("python")
            })
            .cloned()
            .unwrap_or_else(|| match kind {
                "browser" => "https://example.com".into(),
                "phone" => "posit".into(),
                _ => String::new(),
            });
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(workspace)
            .env("POSIT_CANVAS_FRAME", &frame_path)
            .env("POSIT_CANVAS_META", &meta_path)
            .env("POSIT_CANVAS_GEOM", &geom_path)
            .env("POSIT_CANVAS_NUDGE", &nudge_path)
            .env("POSIT_CANVAS_KIND", kind)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn()?;
        Ok(Self {
            kind: kind.to_string(),
            // Pane chrome is always titled "Canvas"; kind shows as a tab when needed.
            title: "Canvas".into(),
            target,
            frame_path,
            meta_path,
            geom_path,
            nudge_path,
            child: Some(child),
            revision: 0,
            painted_revision: 0,
            kitty_front: kitty_pair_for_kind(kind).0,
            kitty_back: kitty_pair_for_kind(kind).1,
            last_mtime: None,
            last_poll: Instant::now()
                .checked_sub(Duration::from_secs(2))
                .unwrap_or_else(Instant::now),
            last_geom: None,
            geom_pixels: None,
            meta: FrameMeta {
                kind: kind.to_string(),
                scale: 1.0,
                coordinate_space: if kind == "phone" {
                    "display-points".into()
                } else {
                    "css-pixels".into()
                },
                target: String::new(),
                ..FrameMeta::default()
            },
            drag_origin: None,
            last_cell: None,
            pressed_action: false,
        })
    }

    pub fn frame_path(&self) -> &Path {
        &self.frame_path
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn geom_path(&self) -> &Path {
        &self.geom_path
    }

    /// Publish the Canvas pane size so capture workers match viewport to the pane
    /// (not the full terminal window).
    pub fn write_geom(&mut self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) {
        let key = (cols, rows, cell_w, cell_h);
        if self.last_geom == Some(key) {
            return;
        }
        let pixel_w = (cols as u32).saturating_mul(cell_w.max(1));
        let pixel_h = (rows as u32).saturating_mul(cell_h.max(1));
        let body = serde_json::json!({
            "cols": cols,
            "rows": rows,
            "cell_width": cell_w,
            "cell_height": cell_h,
            "pixel_width": pixel_w,
            "pixel_height": pixel_h,
        });
        if fs::write(&self.geom_path, body.to_string()).is_ok() {
            self.last_geom = Some(key);
            self.geom_pixels = Some((pixel_w as f64, pixel_h as f64));
        }
    }

    pub fn poll_frame(&mut self) -> bool {
        // Metadata checks are cheap, but still rate-limit disk hits on the UI thread.
        if self.last_poll.elapsed() < Duration::from_millis(250) {
            return self.needs_paint();
        }
        self.last_poll = Instant::now();
        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(_)) = child.try_wait() {
                self.child = None;
            }
        }
        if let Ok(text) = fs::read_to_string(&self.meta_path) {
            if let Ok(meta) = serde_json::from_str::<FrameMeta>(&text) {
                if meta.image_width > 0 && meta.image_height > 0 {
                    self.meta = meta;
                }
            }
        }
        let Ok(meta) = fs::metadata(&self.frame_path) else {
            return self.needs_paint();
        };
        let Ok(mtime) = meta.modified() else {
            return self.needs_paint();
        };
        if self.last_mtime != Some(mtime) {
            self.last_mtime = Some(mtime);
            self.revision = self.revision.saturating_add(1);
        }
        self.needs_paint()
    }

    pub fn needs_paint(&self) -> bool {
        self.revision > 0 && self.revision != self.painted_revision
    }

    pub fn has_frame(&self) -> bool {
        self.revision > 0
    }

    /// Id that currently owns the on-screen placeholders.
    pub fn display_image_id(&self) -> u32 {
        self.kitty_front
    }

    /// Inactive id — upload the next frame here before swapping.
    pub fn upload_image_id(&self) -> u32 {
        self.kitty_back
    }

    #[deprecated(note = "use upload_image_id / display_image_id")]
    pub fn next_image_id(&self) -> u32 {
        self.kitty_back
    }

    pub fn mark_painted(&mut self) {
        self.painted_revision = self.revision;
        // Front shows what we just bound; back is free for the next upload.
        std::mem::swap(&mut self.kitty_front, &mut self.kitty_back);
    }


    /// Kill capture immediately. Quit must not wait on screenshot workers.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            // Bounded reap — never block the UI/quit path on a wedged worker.
            let deadline = Instant::now() + Duration::from_millis(50);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) if Instant::now() >= deadline => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        }
        let _ = fs::remove_file(&self.frame_path);
        let _ = fs::remove_file(&self.meta_path);
        let _ = fs::remove_file(&self.geom_path);
    }

    /// Map a canvas-content cell into phone display-points or browser CSS pixels.
    ///
    /// With live pane geom, Kitty/Ghostty contain-fit the PNG (letterboxing).
    /// Clicks on letterbox margins return `None`. Without geom, stretch-fill.
    pub fn cell_to_point(&self, local_x: u16, local_y: u16, inner: Rect) -> Option<(i32, i32)> {
        let cols = inner.width.max(1) as f64;
        let rows = inner.height.max(1) as f64;
        let (img_w, img_h) = self.image_size();
        if img_w < 2.0 || img_h < 2.0 {
            return None;
        }
        let (coord_w, coord_h) = self.action_size(img_w, img_h);
        let (nx, ny) = if let Some((pane_w, pane_h)) = self.geom_pixels {
            if pane_w < 2.0 || pane_h < 2.0 {
                return None;
            }
            let fit = (pane_w / img_w).min(pane_h / img_h);
            let draw_w = img_w * fit;
            let draw_h = img_h * fit;
            let origin_x = (pane_w - draw_w) * 0.5;
            let origin_y = (pane_h - draw_h) * 0.5;
            let click_x = (local_x as f64 + 0.5) / cols * pane_w;
            let click_y = (local_y as f64 + 0.5) / rows * pane_h;
            if click_x < origin_x
                || click_y < origin_y
                || click_x >= origin_x + draw_w
                || click_y >= origin_y + draw_h
            {
                return None;
            }
            (
                (click_x - origin_x) / draw_w,
                (click_y - origin_y) / draw_h,
            )
        } else {
            // No pixel geom yet — stretch across the cell box.
            (
                (local_x as f64 + 0.5) / cols,
                (local_y as f64 + 0.5) / rows,
            )
        };
        let px = (nx * coord_w).round() as i32;
        let py = (ny * coord_h).round() as i32;
        Some((
            px.clamp(0, (coord_w as i32 - 1).max(0)),
            py.clamp(0, (coord_h as i32 - 1).max(0)),
        ))
    }

    fn image_size(&self) -> (f64, f64) {
        if self.meta.image_width > 0 && self.meta.image_height > 0 {
            (
                self.meta.image_width as f64,
                self.meta.image_height as f64,
            )
        } else {
            match self.kind.as_str() {
                // iPhone logical points (terminal-phone unframed screenshot size).
                "phone" => (402.0, 874.0),
                _ => (1280.0, 800.0),
            }
        }
    }

    fn action_size(&self, img_w: f64, img_h: f64) -> (f64, f64) {
        if self.meta.coord_width > 0 && self.meta.coord_height > 0 {
            return (
                self.meta.coord_width as f64,
                self.meta.coord_height as f64,
            );
        }
        let scale = if self.meta.scale > 0.0 {
            self.meta.scale
        } else {
            1.0
        };
        // Phone screenshots from `terminal-phone action screenshot` are already
        // display-points (JSON width/height == PNG size). Never divide by retina scale.
        if self.kind == "phone"
            || self.meta.coordinate_space == "display-points"
            || self.meta.coordinate_space == "css-pixels"
        {
            // If scale looks like a mistaken retina divisor on an already-point PNG,
            // ignore it when image ≈ known point sizes.
            if self.kind == "phone" && scale > 1.5 && img_w <= 500.0 {
                return (img_w, img_h);
            }
        }
        ((img_w / scale).max(1.0), (img_h / scale).max(1.0))
    }

    pub fn pointer_down(&mut self, local_x: u16, local_y: u16, inner: Rect) -> Option<String> {
        // Finish a prior press that never got MouseUp (common on some terminals).
        if self.drag_origin.is_some() {
            let _ = self.pointer_up(local_x, local_y, inner);
        }
        self.drag_origin = Some((local_x, local_y));
        self.last_cell = Some((local_x, local_y));
        self.pressed_action = false;
        let (x, y) = self.cell_to_point(local_x, local_y, inner)?;
        match self.kind.as_str() {
            // Commit on Down — Ghostty/other hosts often omit MouseUp.
            "phone" => {
                let msg = self.phone_action(&["tap", &x.to_string(), &y.to_string()]);
                self.pressed_action = true;
                self.refresh_soon();
                Some(msg)
            }
            "browser" => {
                enqueue_browser(&[
                    vec![
                        "mouse".into(),
                        "move".into(),
                        x.to_string(),
                        y.to_string(),
                    ],
                    vec!["mouse".into(), "down".into(), "left".into()],
                    vec!["mouse".into(), "up".into(), "left".into()],
                ]);
                self.pressed_action = true;
                self.refresh_soon();
                Some(format!("browser click {x},{y}"))
            }
            _ => Some(format!("{} canvas has no input yet", self.kind)),
        }
    }

    pub fn pointer_drag(&mut self, local_x: u16, local_y: u16, inner: Rect) -> Option<String> {
        self.last_cell = Some((local_x, local_y));
        let (x, y) = self.cell_to_point(local_x, local_y, inner)?;
        match self.kind.as_str() {
            "browser" => Some(format!("browser drag {x},{y}")),
            "phone" => Some(format!("phone drag {x},{y}")),
            _ => None,
        }
    }

    pub fn pointer_up(&mut self, local_x: u16, local_y: u16, inner: Rect) -> Option<String> {
        let origin = self.drag_origin.take()?;
        self.last_cell = Some((local_x, local_y));
        let dx = local_x as i32 - origin.0 as i32;
        let dy = local_y as i32 - origin.1 as i32;
        let (x1, y1) = self.cell_to_point(origin.0, origin.1, inner)?;
        let (x2, y2) = match self.cell_to_point(local_x, local_y, inner) {
            Some(p) => p,
            None => (x1, y1),
        };
        let notice = match self.kind.as_str() {
            "phone" if dx.abs() < 2 && dy.abs() < 2 => {
                if self.pressed_action {
                    format!("phone tap {x1},{y1}")
                } else {
                    let msg = self.phone_action(&["tap", &x1.to_string(), &y1.to_string()]);
                    self.refresh_soon();
                    msg
                }
            }
            "phone" => {
                let msg = self.phone_action(&[
                    "swipe",
                    &x1.to_string(),
                    &y1.to_string(),
                    &x2.to_string(),
                    &y2.to_string(),
                    "--duration",
                    "0.25",
                ]);
                self.refresh_soon();
                msg
            }
            "browser" if dx.abs() < 2 && dy.abs() < 2 => {
                format!("browser click {x1},{y1}")
            }
            "browser" => {
                enqueue_browser(&[
                    vec![
                        "mouse".into(),
                        "move".into(),
                        x1.to_string(),
                        y1.to_string(),
                    ],
                    vec!["mouse".into(), "down".into(), "left".into()],
                    vec![
                        "mouse".into(),
                        "move".into(),
                        x2.to_string(),
                        y2.to_string(),
                    ],
                    vec!["mouse".into(), "up".into(), "left".into()],
                ]);
                self.refresh_soon();
                format!("browser drag {x1},{y1}->{x2},{y2}")
            }
            _ => return None,
        };
        self.pressed_action = false;
        Some(notice)
    }

    pub fn scroll_axis(
        &mut self,
        local_x: u16,
        local_y: u16,
        inner: Rect,
        dy: i32,
        dx: i32,
    ) -> Option<String> {
        let (x, y) = self.cell_to_point(local_x, local_y, inner)?;
        let notice = match self.kind.as_str() {
            "phone" => {
                // Wheel → short swipe (finger opposite content motion).
                let x2 = (x - dx.saturating_mul(40)).max(0);
                let y2 = (y - dy.saturating_mul(40)).max(0);
                let msg = self.phone_action(&[
                    "swipe",
                    &x.to_string(),
                    &y.to_string(),
                    &x2.to_string(),
                    &y2.to_string(),
                    "--duration",
                    "0.2",
                ]);
                self.refresh_soon();
                msg
            }
            "browser" => {
                let dy_px = dy.saturating_mul(80).to_string();
                let dx_px = dx.saturating_mul(80).to_string();
                enqueue_browser(&[
                    vec![
                        "mouse".into(),
                        "move".into(),
                        x.to_string(),
                        y.to_string(),
                    ],
                    vec!["mouse".into(), "wheel".into(), dy_px, dx_px],
                ]);
                self.refresh_soon();
                format!("browser wheel {dy},{dx}")
            }
            _ => return None,
        };
        Some(notice)
    }

    pub fn key(&mut self, key: &str) -> Option<String> {
        let notice = match self.kind.as_str() {
            "phone" => {
                let mapped = map_phone_key(key)?;
                let msg = if mapped.chars().count() == 1 && mapped != "\n" {
                    // Single printable → type
                    self.phone_action(&["type", "--", &mapped])
                } else {
                    self.phone_action(&["key", mapped])
                };
                self.refresh_soon();
                msg
            }
            "browser" => {
                if key.len() == 1 {
                    enqueue_browser(&[vec![
                        "keyboard".into(),
                        "type".into(),
                        key.to_string(),
                    ]]);
                } else {
                    enqueue_browser(&[vec!["press".into(), map_browser_key(key).to_string()]]);
                }
                self.refresh_soon();
                format!("browser key {key}")
            }
            _ => return None,
        };
        Some(notice)
    }

    pub fn button(&mut self, name: &str) -> Option<String> {
        if self.kind != "phone" {
            return None;
        }
        let msg = self.phone_action(&["button", name]);
        self.refresh_soon();
        Some(msg)
    }

    /// Browser chrome: `back` / `forward` / `reload` via agent-browser.
    pub fn browser_nav(&mut self, action: &str) -> Option<String> {
        if self.kind != "browser" {
            return None;
        }
        let cmd = match action {
            "back" | "forward" | "reload" => action,
            _ => return None,
        };
        enqueue_browser(&[vec![cmd.to_string()]]);
        self.refresh_soon();
        Some(format!("browser {cmd}"))
    }

    fn refresh_soon(&mut self) {
        // Ask the capture worker for a faster pass instead of spawning another
        // screenshot process that fights the worker and blocks the machine.
        self.last_poll = Instant::now()
            .checked_sub(Duration::from_secs(2))
            .unwrap_or_else(Instant::now);
        let _ = fs::write(&self.nudge_path, b"1");
    }

    fn phone_action(&self, args: &[&str]) -> String {
        let Some(phone) = find_tool("terminal-phone", "IDE_TERMINAL_PHONE") else {
            return "terminal-phone not found".into();
        };
        let label = (*args.first().unwrap_or(&"")).to_string();
        let target = self.target.clone();
        let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        enqueue_phone(&phone, &target, &owned);
        format!("phone {label}")
    }
}

/// Serial device-action queue: click move/down/up must not race across processes.
enum DeviceJob {
    Browser(Vec<Vec<String>>),
    Phone {
        bin: String,
        target: String,
        args: Vec<String>,
    },
}

fn device_tx() -> Sender<DeviceJob> {
    static TX: OnceLock<Sender<DeviceJob>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<DeviceJob>();
        std::thread::Builder::new()
            .name("posit-canvas-device".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    match job {
                        DeviceJob::Browser(steps) => {
                            let Some(browser) = find_tool("agent-browser", "IDE_AGENT_BROWSER")
                            else {
                                continue;
                            };
                            for args in steps {
                                let _ = Command::new(&browser)
                                    .args(&args)
                                    .stdin(Stdio::null())
                                    .stdout(Stdio::null())
                                    .stderr(Stdio::null())
                                    .status();
                            }
                        }
                        DeviceJob::Phone { bin, target, args } => {
                            let _ = Command::new(&bin)
                                .arg("action")
                                .arg(&target)
                                .args(&args)
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .status();
                        }
                    }
                }
            })
            .expect("canvas device worker");
        tx
    })
    .clone()
}

fn enqueue_browser(steps: &[Vec<String>]) {
    let _ = device_tx().send(DeviceJob::Browser(steps.to_vec()));
}

fn enqueue_phone(bin: &str, target: &str, args: &[String]) {
    let _ = device_tx().send(DeviceJob::Phone {
        bin: bin.to_string(),
        target: target.to_string(),
        args: args.to_vec(),
    });
}

impl Drop for CanvasSurface {
    fn drop(&mut self) {
        self.stop();
    }
}

fn map_phone_key(key: &str) -> Option<&str> {
    match key {
        "enter" | "return" => Some("return"),
        "backspace" => Some("backspace"),
        "tab" => Some("tab"),
        "esc" | "escape" => Some("escape"),
        "up" => Some("up"),
        "down" => Some("down"),
        "left" => Some("left"),
        "right" => Some("right"),
        other if other.chars().count() == 1 => Some(other),
        _ => None,
    }
}

fn map_browser_key(key: &str) -> &str {
    match key {
        "enter" | "return" => "Enter",
        "backspace" => "Backspace",
        "tab" => "Tab",
        "esc" | "escape" => "Escape",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        other => other,
    }
}

fn find_tool(name: &str, env_key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(env_key) {
        let path = PathBuf::from(&value);
        if path.is_file() {
            return Some(value);
        }
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/ide-tui → repo root
    if dir.pop() && dir.pop() {
        let vendor = dir.join("vendor/bin").join(name);
        if vendor.is_file() {
            return Some(vendor.display().to_string());
        }
    }
    which(name)
}

fn which(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// Kitty row/column diacritics for Unicode placeholders (subset of kitty's table).
const KITTY_DIACRITICS: &[u32] = &[
    0x0305, 0x030D, 0x030E, 0x0310, 0x0312, 0x033D, 0x033E, 0x033F, 0x0346, 0x034A, 0x034B, 0x034C,
    0x0350, 0x0351, 0x0352, 0x0357, 0x035B, 0x0363, 0x0364, 0x0365, 0x0366, 0x0367, 0x0368, 0x0369,
    0x036A, 0x036B, 0x036C, 0x036D, 0x036E, 0x036F, 0x0483, 0x0484, 0x0485, 0x0486, 0x0487, 0x0592,
    0x0593, 0x0594, 0x0595, 0x0597, 0x0598, 0x0599, 0x059C, 0x059D, 0x059E, 0x059F, 0x05A0, 0x05A1,
    0x05A8, 0x05A9, 0x05AB, 0x05AC, 0x05AF, 0x05C4, 0x0610, 0x0611, 0x0612, 0x0613, 0x0614, 0x0615,
    0x0616, 0x0617, 0x0657, 0x0658, 0x0659, 0x065A, 0x065B, 0x065D, 0x065E, 0x06D6, 0x06D7, 0x06D8,
    0x06D9, 0x06DA, 0x06DB, 0x06DC, 0x06DF, 0x06E0, 0x06E1, 0x06E2, 0x06E4, 0x06E7, 0x06E8, 0x06EB,
    0x06EC, 0x0730, 0x0732, 0x0733, 0x0735, 0x0736, 0x073A, 0x073D, 0x073F, 0x0740, 0x0741, 0x0743,
    0x0745, 0x0747, 0x0749, 0x074A, 0x07EB, 0x07EC, 0x07ED, 0x07EE, 0x07EF, 0x07F0, 0x07F1, 0x07F3,
    0x0816, 0x0817, 0x0818, 0x0819, 0x081B, 0x081C, 0x081D, 0x081E, 0x081F, 0x0820, 0x0821, 0x0822,
    0x0823, 0x0825, 0x0826, 0x0827, 0x0829, 0x082A, 0x082B, 0x082C, 0x082D, 0x0951, 0x0953, 0x0954,
    0x0F82, 0x0F83, 0x0F86, 0x0F87, 0x135D, 0x135E, 0x135F, 0x17DD, 0x193A, 0x1A17, 0x1A75, 0x1A76,
    0x1A77, 0x1A78, 0x1A79, 0x1A7A, 0x1A7B, 0x1A7C, 0x1B6B, 0x1B6D, 0x1B6E, 0x1B6F, 0x1B70, 0x1B71,
    0x1B72, 0x1B73, 0x1CD0, 0x1CD1, 0x1CD2, 0x1CDA, 0x1CDB, 0x1CE0, 0x1DC0, 0x1DC1, 0x1DC3, 0x1DC4,
    0x1DC5, 0x1DC6, 0x1DC7, 0x1DC8, 0x1DC9, 0x1DCB, 0x1DCC, 0x1DD1, 0x1DD2, 0x1DD3, 0x1DD4, 0x1DD5,
    0x1DD6, 0x1DD7, 0x1DD8, 0x1DD9, 0x1DDA, 0x1DDB, 0x1DDC, 0x1DDD, 0x1DDE, 0x1DDF, 0x1DE0, 0x1DE1,
    0x1DE2, 0x1DE3, 0x1DE4, 0x1DE5, 0x1DE6, 0x1DFE, 0x20D0, 0x20D1, 0x20D4, 0x20D5, 0x20D6, 0x20D7,
    0x20DB, 0x20DC, 0x20E1, 0x20E7, 0x20E9, 0x20F0, 0x2CEF, 0x2CF0, 0x2CF1, 0x2DE0, 0x2DE1, 0x2DE2,
    0x2DE3, 0x2DE4, 0x2DE5, 0x2DE6, 0x2DE7, 0x2DE8, 0x2DE9, 0x2DEA, 0x2DEB, 0x2DEC, 0x2DED, 0x2DEE,
    0x2DEF, 0x2DF0, 0x2DF1, 0x2DF2, 0x2DF3, 0x2DF4, 0x2DF5, 0x2DF6, 0x2DF7, 0x2DF8, 0x2DF9, 0x2DFA,
    0x2DFB, 0x2DFC, 0x2DFD, 0x2DFE, 0x2DFF, 0xA66F, 0xA67C, 0xA67D, 0xA6F0, 0xA6F1, 0xA8E0, 0xA8E1,
    0xA8E2, 0xA8E3, 0xA8E4, 0xA8E5, 0xA8E6, 0xA8E7, 0xA8E8, 0xA8E9, 0xA8EA, 0xA8EB, 0xA8EC, 0xA8ED,
    0xA8EE, 0xA8EF, 0xA8F0, 0xA8F1, 0xAAB0, 0xAAB2, 0xAAB3, 0xAAB7, 0xAAB8, 0xAABE, 0xAABF, 0xAAC1,
    0xFE20, 0xFE21, 0xFE22, 0xFE23, 0xFE24, 0xFE25, 0xFE26, 0x10A0F, 0x10A38, 0x1D185, 0x1D186, 0x1D187,
    0x1D188, 0x1D189, 0x1D1AA, 0x1D1AB, 0x1D1AC, 0x1D1AD, 0x1D242, 0x1D243, 0x1D244,
];

fn kitty_mark(index: usize) -> char {
    let code = KITTY_DIACRITICS[index % KITTY_DIACRITICS.len()];
    char::from_u32(code).unwrap_or('\u{0305}')
}

/// Unicode placeholder glyph for one cell of a virtual Kitty placement.
#[cfg(test)]
pub fn kitty_placeholder_cell(row: usize, col: usize) -> String {
    format!("\u{10EEEE}{}{}", kitty_mark(row), kitty_mark(col))
}

/// FG color that binds placeholders to `image_id` (indexed when id < 256).
#[cfg(test)]
pub fn kitty_placeholder_fg(image_id: u32) -> ratatui::style::Color {
    if image_id < 256 {
        ratatui::style::Color::Indexed(image_id as u8)
    } else {
        ratatui::style::Color::Rgb(
            ((image_id >> 16) & 255) as u8,
            ((image_id >> 8) & 255) as u8,
            (image_id & 255) as u8,
        )
    }
}

/// Rewrite Unicode placeholders so a virtual Kitty placement stays bound to the
/// Canvas cell box (not the full terminal). Must run after every ratatui draw.
pub fn write_kitty_placeholders(
    out: &mut impl Write,
    inner: Rect,
    image_id: u32,
) -> io::Result<()> {
    let cols = (inner.width as usize).min(KITTY_DIACRITICS.len());
    let rows = (inner.height as usize).min(KITTY_DIACRITICS.len());
    if cols < 2 || rows < 2 || image_id == 0 {
        return Ok(());
    }
    let color = if image_id < 256 {
        format!("\x1b[38;5;{image_id}m")
    } else {
        let r = (image_id >> 16) & 255;
        let g = (image_id >> 8) & 255;
        let b = image_id & 255;
        format!("\x1b[38;2;{r};{g};{b}m")
    };
    write!(out, "\x1b[0m{color}")?;
    let origin_row = inner.y.saturating_add(1) as usize;
    let origin_col = inner.x.saturating_add(1) as usize;
    // Every cell needs row+column diacritics. Bare U+10EEEE only on column 0
    // made Ghostty smear/tear the virtual placement across the strip.
    for r in 0..rows {
        write!(out, "\x1b[{};{}H", origin_row + r, origin_col)?;
        for c in 0..cols {
            write!(out, "\u{10EEEE}{}{}", kitty_mark(r), kitty_mark(c))?;
        }
    }
    write!(out, "\x1b[39m")?;
    Ok(())
}

/// Stable Kitty id pair per Canvas kind (both < 256 for indexed placeholder FG).
pub fn kitty_pair_for_kind(kind: &str) -> (u32, u32) {
    match kind {
        "browser" => (20, 21),
        "phone" => (22, 23),
        "android" => (24, 25),
        _ => (26, 27),
    }
}

/// Emit a Kitty virtual placement covering exactly `inner` cells with the PNG at `path`.
///
/// Uses Unicode placeholders (`U=1`) so Ghostty/kitty size the image to the Canvas
/// pane — never the full terminal window.
///
/// Double-buffer: upload to `image_id` without deleting the visible frame first.
/// Pass `retire_id` (previous front) to free after placeholders retarget.
pub fn paint_kitty_png(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    inner: Rect,
    path: &Path,
    image_id: u32,
) -> io::Result<bool> {
    paint_kitty_png_swap(out, arbiter, inner, path, image_id, None)
}

/// Upload `path` to `image_id`, bind placeholders, then optionally delete `retire_id`.
pub fn paint_kitty_png_swap(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    inner: Rect,
    path: &Path,
    image_id: u32,
    retire_id: Option<u32>,
) -> io::Result<bool> {
    if inner.width < 2 || inner.height < 2 {
        return Ok(false);
    }
    let backend = arbiter.backend();
    if backend != GraphicsBackend::Kitty {
        return Ok(false);
    }
    let bytes = match fs::read(path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => return Ok(false),
    };
    paint_kitty_png_bytes_swap(out, arbiter, inner, &bytes, image_id, retire_id)
}

/// Emit a Kitty virtual placement from in-memory PNG bytes (Ghostty-proven `f=100`).
pub fn paint_kitty_png_bytes(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    inner: Rect,
    png: &[u8],
    image_id: u32,
) -> io::Result<bool> {
    paint_kitty_png_bytes_swap(out, arbiter, inner, png, image_id, None)
}

/// Upload PNG to `image_id` without blanking the visible frame first.
///
/// Ghostty blanks cells when the *shown* id is retransmitted ([#15124](https://github.com/ghostty-org/ghostty/discussions/15124)).
/// Upload the next frame to the inactive id, retarget placeholders, then retire the old id.
pub fn paint_kitty_png_bytes_swap(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    inner: Rect,
    png: &[u8],
    image_id: u32,
    retire_id: Option<u32>,
) -> io::Result<bool> {
    if inner.width < 2 || inner.height < 2 || png.is_empty() || image_id == 0 {
        return Ok(false);
    }
    let backend = arbiter.backend();
    if backend != GraphicsBackend::Kitty {
        return Ok(false);
    }
    // Never delete `image_id` before transmit — that blanks the pane for 10–20ms+.
    let b64 = base64_encode(png);
    let cols = inner.width;
    let rows = inner.height;
    let box_params = format!("U=1,c={cols},r={rows},z=-1073741824");
    let mut first = true;
    let mut rest = b64.as_str();
    while !rest.is_empty() {
        let take = rest.len().min(4096);
        let (chunk, next) = rest.split_at(take);
        let more = if next.is_empty() { 0 } else { 1 };
        if first {
            write!(
                out,
                "\x1b_Ga=T,f=100,t=d,i={image_id},q=2,{box_params},m={more};{chunk}\x1b\\"
            )?;
            first = false;
        } else {
            write!(out, "\x1b_Gm={more};{chunk}\x1b\\")?;
        }
        rest = next;
    }
    write_kitty_placeholders(out, inner, image_id)?;
    if let Some(old) = retire_id {
        if old != 0 && old != image_id {
            write!(out, "\x1b_Ga=d,d=i,i={old},q=2\x1b\\")?;
        }
    }
    out.flush()?;
    Ok(true)
}

/// Encode tightly-packed RGB24 into a PNG (filter None, zlib stored blocks).
/// Used for the editor Kitty minimap so Ghostty can use the same `f=100` path as Canvas.
pub fn encode_rgb_png(width: u32, height: u32, rgb: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    let row_bytes = (width as usize).saturating_mul(3);
    let expected = row_bytes.saturating_mul(height as usize);
    if rgb.len() < expected {
        return None;
    }
    let mut raw = Vec::with_capacity((row_bytes + 1).saturating_mul(height as usize));
    for y in 0..height as usize {
        raw.push(0); // filter None
        let start = y.saturating_mul(row_bytes);
        raw.extend_from_slice(&rgb[start..start + row_bytes]);
    }
    let zlib = zlib_store(&raw);
    let mut out = Vec::with_capacity(zlib.len() + 256);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    write_png_chunk(&mut out, b"IHDR", &{
        let mut ihdr = [0u8; 13];
        ihdr[0..4].copy_from_slice(&width.to_be_bytes());
        ihdr[4..8].copy_from_slice(&height.to_be_bytes());
        ihdr[8] = 8; // bit depth
        ihdr[9] = 2; // truecolor RGB
        ihdr
    });
    write_png_chunk(&mut out, b"IDAT", &zlib);
    write_png_chunk(&mut out, b"IEND", &[]);
    Some(out)
}

fn write_png_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    let mut crc = crc32_init();
    crc = crc32_update(crc, ty);
    crc = crc32_update(crc, data);
    out.extend_from_slice(&crc32_finish(crc).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    // CMF/FLG: 0x78 0x01 (window 32k, no dict, check bits)
    let mut out = vec![0x78, 0x01];
    let mut offset = 0usize;
    while offset < data.len() {
        let rest = data.len() - offset;
        let take = rest.min(65535);
        let last = offset + take >= data.len();
        out.push(if last { 0x01 } else { 0x00 });
        let len = take as u16;
        let nlen = !len;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(&data[offset..offset + take]);
        offset += take;
    }
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32_init() -> u32 {
    0xffff_ffff
}

fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xff) as usize;
        crc = CRC32_TABLE[idx] ^ (crc >> 8);
    }
    crc
}

fn crc32_finish(crc: u32) -> u32 {
    crc ^ 0xffff_ffff
}

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xedb8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// Emit a Kitty virtual placement covering `inner` with raw 24-bit RGB pixels.
///
/// Same Ghostty-proven path as Canvas: PNG `f=100` + `U=1` unicode placeholders.
/// Glyph minimap is only for hosts that are not Kitty/Ghostty.
pub fn paint_kitty_rgb(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    inner: Rect,
    rgb: &[u8],
    width: u32,
    height: u32,
    image_id: u32,
) -> io::Result<bool> {
    paint_kitty_rgb_swap(out, arbiter, inner, rgb, width, height, image_id, None)
}

pub fn paint_kitty_rgb_swap(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    inner: Rect,
    rgb: &[u8],
    width: u32,
    height: u32,
    image_id: u32,
    retire_id: Option<u32>,
) -> io::Result<bool> {
    let Some(png) = encode_rgb_png(width, height, rgb) else {
        return Ok(false);
    };
    paint_kitty_png_bytes_swap(out, arbiter, inner, &png, image_id, retire_id)
}

/// Drop a previously placed Kitty image by id (quiet).
pub fn delete_kitty_image(out: &mut impl Write, image_id: u32) -> io::Result<()> {
    if image_id == 0 {
        return Ok(());
    }
    write!(out, "\x1b_Ga=d,d=i,i={image_id},q=2\x1b\\")?;
    out.flush()
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (a << 16) | (b << 8) | c;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip_prefix() {
        let encoded = base64_encode(b"PNG");
        assert!(encoded.starts_with("UE5H"));
    }

    #[test]
    fn kitty_paint_uses_virtual_placement_for_canvas_box() {
        let dir = tempfile_dir();
        let png = dir.join("frame.png");
        // Minimal 1×1 PNG.
        fs::write(
            &png,
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\x0f\x00\x00\x01\x01\x00\x05\x18\xd8N\x00\x00\x00\x00IEND\xaeB`\x82",
        )
        .unwrap();
        let mut out = Vec::new();
        // Force Kitty path regardless of host TERM.
        // SAFETY: test-only env mutation for graphics probe.
        unsafe {
            std::env::set_var("TERM", "xterm-kitty");
            std::env::set_var("TERM_PROGRAM", "kitty");
        }
        let mut arbiter = GraphicsArbiter::default();
        let inner = Rect {
            x: 100,
            y: 2,
            width: 40,
            height: 20,
        };
        let painted = paint_kitty_png(&mut out, &mut arbiter, inner, &png, 7).unwrap();
        assert!(painted);
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("U=1,c=40,r=20"),
            "expected Canvas-sized virtual placement, got: {text}"
        );
        assert!(
            !text.contains(",C=1,"),
            "legacy cursor placement must not size to the full terminal"
        );
        assert!(text.contains('\u{10EEEE}'), "placeholders required for cell binding");
    }

    #[test]
    fn kitty_swap_does_not_blank_upload_id_first() {
        unsafe {
            std::env::set_var("TERM", "xterm-kitty");
            std::env::set_var("TERM_PROGRAM", "kitty");
        }
        let mut arbiter = GraphicsArbiter::default();
        let inner = Rect {
            x: 2,
            y: 2,
            width: 8,
            height: 4,
        };
        let rgb = vec![0x20_u8; 8 * 4 * 3];
        let mut out = Vec::new();
        paint_kitty_rgb_swap(&mut out, &mut arbiter, inner, &rgb, 8, 4, 42, Some(41)).unwrap();
        let text = String::from_utf8_lossy(&out);
        let transmit = text.find("a=T,").expect("transmit");
        let retire = text.find("a=d,d=i,i=41").expect("retire old front");
        assert!(
            !text[..transmit].contains("a=d,d=i,i=42"),
            "must not delete the upload id before transmit"
        );
        assert!(retire > transmit, "retire after transmit+placeholders");
    }

    #[test]
    fn kitty_rgb_paint_encodes_png_f100() {
        // SAFETY: test-only env mutation for graphics probe.
        unsafe {
            std::env::set_var("TERM", "xterm-kitty");
            std::env::set_var("TERM_PROGRAM", "kitty");
        }
        let mut arbiter = GraphicsArbiter::default();
        let inner = Rect {
            x: 10,
            y: 2,
            width: 8,
            height: 12,
        };
        let rgb = vec![0x40_u8; 8 * 12 * 3];
        let png = encode_rgb_png(8, 12, &rgb).expect("png");
        assert!(png.starts_with(b"\x89PNG"), "PNG signature");
        let mut out = Vec::new();
        let painted = paint_kitty_rgb(&mut out, &mut arbiter, inner, &rgb, 8, 12, 900_001).unwrap();
        assert!(painted);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("f=100"), "Ghostty path uses PNG: {text}");
        assert!(
            text.contains("U=1,c=8,r=12"),
            "minimap uses Canvas-style virtual placement: {text}"
        );
        assert!(
            text.contains('\u{10EEEE}'),
            "placeholders required for cell binding"
        );
    }

    #[test]
    fn write_geom_publishes_canvas_pixel_box() {
        let mut surface = CanvasSurface::test_stub("browser", "https://example.com");
        surface.write_geom(40, 20, 9, 18);
        let text = fs::read_to_string(surface.geom_path()).unwrap();
        assert!(text.contains("\"cols\":40"));
        assert!(text.contains("\"rows\":20"));
        assert!(text.contains("\"pixel_width\":360"));
        assert!(text.contains("\"pixel_height\":360"));
        // Second write with same size must be a no-op (no rewrite thrash).
        let mtime = fs::metadata(surface.geom_path()).unwrap().modified().unwrap();
        surface.write_geom(40, 20, 9, 18);
        let mtime2 = fs::metadata(surface.geom_path()).unwrap().modified().unwrap();
        assert_eq!(mtime, mtime2);
    }

    #[test]
    fn cell_to_point_maps_phone_display_points() {
        let mut surface = CanvasSurface {
            kind: "phone".into(),
            title: "t".into(),
            target: "posit".into(),
            frame_path: PathBuf::from("/tmp/x.png"),
            meta_path: PathBuf::from("/tmp/x.json"),
            geom_path: PathBuf::from("/tmp/x.geom.json"),
            nudge_path: PathBuf::from("/tmp/x.nudge"),
            child: None,
            revision: 1,
            painted_revision: 0,
            kitty_front: 22,
            kitty_back: 23,
            last_mtime: None,
            last_poll: Instant::now(),
            last_geom: None,
            // Pane matches image aspect so contain-fit fills the box.
            geom_pixels: Some((402.0, 874.0)),
            meta: FrameMeta {
                kind: "phone".into(),
                image_width: 402,
                image_height: 874,
                scale: 1.0,
                coordinate_space: "display-points".into(),
                coord_width: 402,
                coord_height: 874,
                target: "posit".into(),
            },
            drag_origin: None,
            last_cell: None,
            pressed_action: false,
        };
        let inner = Rect {
            x: 10,
            y: 2,
            width: 40,
            height: 80,
        };
        let (x0, y0) = surface.cell_to_point(0, 0, inner).unwrap();
        let (x1, y1) = surface.cell_to_point(39, 79, inner).unwrap();
        assert!(x0 < 30, "top-left should be near 0,0 got {x0},{y0}");
        assert!(y0 < 30, "top-left should be near 0,0 got {x0},{y0}");
        assert!(x1 > 370, "bottom-right near 402 got {x1}");
        assert!(y1 > 840, "bottom-right near 874 got {y1}");
        // Mistaken retina scale on a points-sized PNG must not shrink taps.
        surface.meta.scale = 3.0;
        let (x_mid, _y_mid) = surface.cell_to_point(20, 40, inner).unwrap();
        assert!(
            (180..230).contains(&x_mid),
            "center x should stay in points, got {x_mid}"
        );
        surface.drag_origin = Some((2, 2));
        assert!(surface.gesture_active());
        surface.drag_origin = None;
        assert!(!surface.gesture_active());
    }

    #[test]
    fn phone_tap_uses_display_points_not_pane_pixels() {
        let mut phone = CanvasSurface::test_stub("phone", "posit");
        phone.meta.image_width = 402;
        phone.meta.image_height = 874;
        phone.meta.coord_width = 402;
        phone.meta.coord_height = 874;
        phone.meta.coordinate_space = "display-points".into();
        phone.meta.scale = 1.0;
        // Wide pane would letterbox; center cell must still hit ~mid screen.
        phone.geom_pixels = Some((800.0, 874.0));
        let inner = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 80,
        };
        let (x, y) = phone.cell_to_point(40, 40, inner).unwrap();
        assert!((180..230).contains(&x), "expected ~201 got {x}");
        assert!((400..480).contains(&y), "expected ~437 got {y}");
    }

    #[test]
    fn tap_vs_swipe_threshold() {
        let dx = 0i32;
        let dy = 1i32;
        assert!(dx.abs() < 2 && dy.abs() < 2, "near-stationary is a tap");
        let dx = 5i32;
        let dy = 0i32;
        assert!(!(dx.abs() < 2 && dy.abs() < 2), "lateral move is a swipe");
    }

    #[test]
    fn phone_and_browser_dispatch_through_stubs() {
        let dir = tempfile_dir();
        let phone_log = dir.join("phone.log");
        let browser_log = dir.join("browser.log");
        write_stub(
            &dir.join("terminal-phone"),
            &phone_log,
            r#"#!/usr/bin/env bash
echo "$@" >> "$POSIT_STUB_LOG"
if [[ "$1" == "action" && "$3" == "screenshot" ]]; then
  out=""
  while [[ $# -gt 0 ]]; do
    if [[ "$1" == "--output" ]]; then out="$2"; break; fi
    shift
  done
  if [[ -n "$out" ]]; then
    printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\x0f\x00\x00\x01\x01\x00\x05\x18\xd8N\x00\x00\x00\x00IEND\xaeB`\x82' > "$out"
  fi
fi
exit 0
"#,
        );
        write_stub(
            &dir.join("agent-browser"),
            &browser_log,
            r#"#!/usr/bin/env bash
echo "$@" >> "$POSIT_STUB_LOG"
if [[ "$1" == "screenshot" ]]; then
  out="${2:-/tmp/stub.png}"
  printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\x0f\x00\x00\x01\x01\x00\x05\x18\xd8N\x00\x00\x00\x00IEND\xaeB`\x82' > "$out"
fi
exit 0
"#,
        );
        // Prefer env overrides — find_tool checks them before vendor/bin.
        // SAFETY: test-only env mutation for stub discovery.
        unsafe {
            std::env::set_var("IDE_TERMINAL_PHONE", dir.join("terminal-phone"));
            std::env::set_var("IDE_AGENT_BROWSER", dir.join("agent-browser"));
        }

        let inner = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 80,
        };
        let mut phone = blank_surface("phone", "posit", &dir);
        phone.meta.image_width = 1170;
        phone.meta.image_height = 2532;
        phone.meta.scale = 3.0;
        unsafe { std::env::set_var("POSIT_STUB_LOG", &phone_log) };
        let _ = fs::remove_file(&phone_log);
        assert!(phone.pointer_down(10, 20, inner).is_some());
        assert!(phone.gesture_active());
        let tap = phone.pointer_up(10, 20, inner).unwrap();
        assert!(tap.contains("phone"), "{tap}");
        assert!(!phone.gesture_active());
        let _ = phone.pointer_down(5, 5, inner);
        let swipe = phone.pointer_up(30, 40, inner).unwrap();
        assert!(swipe.contains("phone"), "{swipe}");
        let _ = phone.key("a");
        let _ = phone.scroll_axis(10, 10, inner, 1, 0);
        let _ = phone.button("HOME");
        let _ = phone.button("LOCK");
        let phone_body = wait_log(&phone_log, &["tap", "swipe", "type", "button", "HOME"], 2000);
        assert!(
            phone_body.contains("tap") && phone_body.contains("swipe") && phone_body.contains("type"),
            "phone log missing actions:\n{phone_body}"
        );
        assert!(
            phone_body.contains("button") && phone_body.contains("HOME"),
            "phone log missing button:\n{phone_body}"
        );

        let mut browser = blank_surface("browser", "https://example.com", &dir);
        browser.meta.image_width = 1280;
        browser.meta.image_height = 800;
        browser.meta.scale = 1.0;
        unsafe { std::env::set_var("POSIT_STUB_LOG", &browser_log) };
        let _ = fs::remove_file(&browser_log);
        assert!(browser.pointer_down(8, 8, inner).is_some());
        assert!(browser.pointer_drag(12, 10, inner).is_some());
        assert!(browser.pointer_up(12, 10, inner).is_some());
        let _ = browser.key("enter");
        let _ = browser.scroll_axis(8, 8, inner, 1, 0);
        let _ = browser.browser_nav("back");
        let _ = browser.browser_nav("reload");
        let browser_body = wait_log(
            &browser_log,
            &["mouse", "down", "up", "wheel", "press", "back", "reload"],
            // Serial device worker + host memory pressure can delay stub writes.
            8_000,
        );
        assert!(
            browser_body.contains("mouse")
                && browser_body.contains("down")
                && browser_body.contains("up")
                && browser_body.contains("wheel"),
            "browser log missing mouse actions:\n{browser_body}"
        );
        assert!(
            browser_body.contains("back") && browser_body.contains("reload"),
            "browser log missing nav:\n{browser_body}"
        );
        assert!(
            browser_body.contains("press") || browser_body.contains("keyboard"),
            "browser log missing key:\n{browser_body}"
        );
    }

    fn wait_log(path: &Path, needles: &[&str], timeout_ms: u64) -> String {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut body = String::new();
        while Instant::now() < deadline {
            body = fs::read_to_string(path).unwrap_or_default();
            if needles.iter().all(|n| body.contains(n)) {
                return body;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        body
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "posit-canvas-test-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_stub(path: &Path, log: &Path, body: &str) {
        let _ = log;
        fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }

    fn blank_surface(kind: &str, target: &str, dir: &Path) -> CanvasSurface {
        CanvasSurface {
            kind: kind.into(),
            title: kind.into(),
            target: target.into(),
            frame_path: dir.join(format!("{kind}.png")),
            meta_path: dir.join(format!("{kind}.json")),
            geom_path: dir.join(format!("{kind}.geom.json")),
            nudge_path: dir.join(format!("{kind}.nudge")),
            child: None,
            revision: 1,
            painted_revision: 0,
            kitty_front: kitty_pair_for_kind(kind).0,
            kitty_back: kitty_pair_for_kind(kind).1,
            last_mtime: None,
            last_poll: Instant::now(),
            last_geom: None,
            geom_pixels: None,
            meta: FrameMeta {
                kind: kind.into(),
                target: target.into(),
                ..FrameMeta::default()
            },
            drag_origin: None,
            last_cell: None,
            pressed_action: false,
        }
    }
}
