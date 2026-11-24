//! edocview-style markdown document preview for Kitty/Ghostty.
//!
//! Mirrors https://github.com/aspauldingcode/edocview.nvim:
//! 1. pandoc (+ typst) compiles Markdown → PDF (cached by content SHA).
//! 2. pymupdf rasterizes pages at `cols * cell_w` (capped; width-cached).
//! 3. Transmit once (`a=t`); scroll with crop placements (`a=p` + `x,y,w,h`).
//! 4. Never `U=1` / never destination `c,r` — those stretched Help on Ghostty.
//!
//! **Never blocks the UI thread on compile/raster.** Heavy work runs on a
//! background thread; first page arrives ASAP, full document follows.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use serde::Deserialize;

use crate::canvas;
use crate::graphics::{GraphicsArbiter, GraphicsBackend};

const PAGE_GAP_ROWS: usize = 1;
/// Preview never needs full-retina billboards — caps pandoc/pymupdf cost.
pub const MAX_PREVIEW_WIDTH_PX: u32 = 960;
/// Debounce before kicking a raster after open/edit.
pub const RASTER_DEBOUNCE: Duration = Duration::from_millis(40);

#[derive(Clone, Debug)]
pub struct RetainedPage {
    pub path: PathBuf,
    #[allow(dead_code)]
    pub hash: String,
    pub px_w: u32,
    pub px_h: u32,
    pub row: usize,
    pub height_rows: usize,
    pub image_id: u32,
    pub transmitted: bool,
}

/// Retained multi-page document (Help / Markdown Canvas).
#[derive(Clone, Debug, Default)]
pub struct RetainedDoc {
    pub pages: Vec<RetainedPage>,
    pub total_rows: usize,
    pub cols: u16,
    pub cell_w: u32,
    pub cell_h: u32,
    pub content_key: u64,
    pub base_image_id: u32,
    /// True when only the first page has arrived; full raster still inflight.
    #[allow(dead_code)]
    pub partial: bool,
    /// (scroll_rows, viewport_rows, screen_x, screen_y, cols, page_index, src_y)
    pub placed: Option<(usize, u16, u16, u16, u16, usize, u32)>,
}

impl RetainedDoc {
    #[allow(dead_code)]
    pub fn needs_raster(&self, content_key: u64, cols: u16, cell_w: u32, cell_h: u32) -> bool {
        self.pages.is_empty()
            || self.content_key != content_key
            || self.cols != cols
            || self.cell_w != cell_w
            || self.cell_h != cell_h
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct PageJson {
    pub path: String,
    pub hash: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewSlot {
    Help,
    Markdown,
}

#[derive(Debug)]
struct Inflight {
    gen: u64,
    slot: PreviewSlot,
    key: u64,
    cols: u16,
    cell_w: u32,
    cell_h: u32,
    base_image_id: u32,
    rx: Receiver<WorkerEvent>,
}

#[derive(Debug)]
enum WorkerEvent {
    Pages {
        gen: u64,
        pages: Vec<PageJson>,
        partial: bool,
    },
    Failed {
        gen: u64,
        error: String,
    },
}

/// Background preview engine — one inflight job, generation-cancelled.
#[derive(Debug, Default)]
pub struct PreviewEngine {
    next_gen: u64,
    inflight: Option<Inflight>,
    pub last_error: Option<String>,
    /// Slot that should start after debounce.
    pending: Option<PendingRequest>,
}

#[derive(Clone, Debug)]
struct PendingRequest {
    slot: PreviewSlot,
    source: String,
    resource_dir: PathBuf,
    cols: u16,
    cell_w: u32,
    cell_h: u32,
    base_image_id: u32,
    key: u64,
    due: Instant,
}

impl PreviewEngine {
    pub fn busy(&self) -> bool {
        self.inflight.is_some() || self.pending.is_some()
    }

    /// Schedule a raster (debounced). Never runs work on the caller thread.
    pub fn request(
        &mut self,
        slot: PreviewSlot,
        source: &str,
        resource_dir: &Path,
        cols: u16,
        cell_w: u32,
        cell_h: u32,
        base_image_id: u32,
        debounce: Duration,
    ) {
        let width_px = preview_width_px(cols, cell_w);
        let key = content_key(source, width_px);
        // Already rendering this exact fingerprint.
        if self
            .inflight
            .as_ref()
            .is_some_and(|job| job.key == key && job.slot == slot)
        {
            self.pending = None;
            return;
        }
        if self.pending.as_ref().is_some_and(|p| p.key == key && p.slot == slot) {
            return;
        }
        self.pending = Some(PendingRequest {
            slot,
            source: source.to_string(),
            resource_dir: resource_dir.to_path_buf(),
            cols,
            cell_w,
            cell_h,
            base_image_id,
            key,
            due: Instant::now() + debounce,
        });
    }

    /// Cancel pending/inflight for a slot (e.g. canvas closed).
    pub fn cancel_slot(&mut self, slot: PreviewSlot) {
        if self.pending.as_ref().is_some_and(|p| p.slot == slot) {
            self.pending = None;
        }
        if self.inflight.as_ref().is_some_and(|j| j.slot == slot) {
            // Bump gen so the result is ignored.
            self.next_gen = self.next_gen.saturating_add(1);
            self.inflight = None;
        }
    }

    /// Start due jobs and apply finished pages into `help` / `markdown` docs.
    /// Returns true when UI should present again.
    pub fn poll(
        &mut self,
        help: &mut Option<RetainedDoc>,
        markdown: &mut Option<RetainedDoc>,
    ) -> bool {
        let mut dirty = false;
        if self.inflight.is_none() {
            if let Some(pending) = self.pending.take() {
                if pending.due <= Instant::now() {
                    self.spawn(pending);
                    dirty = true;
                } else {
                    self.pending = Some(pending);
                }
            }
        }
        // Drain all ready worker messages without blocking.
        loop {
            let Some(job) = self.inflight.as_ref() else {
                break;
            };
            let gen = job.gen;
            let slot = job.slot;
            let key = job.key;
            let cols = job.cols;
            let cell_w = job.cell_w;
            let cell_h = job.cell_h;
            let base = job.base_image_id;
            let event = match job.rx.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.inflight = None;
                    dirty = true;
                    break;
                }
            };
            match event {
                WorkerEvent::Pages {
                    gen: eg,
                    pages,
                    partial,
                } if eg == gen => {
                    match slot {
                        PreviewSlot::Help => {
                            apply_pages(help, pages, key, cols, cell_w, cell_h, base, partial)
                        }
                        PreviewSlot::Markdown => {
                            apply_pages(markdown, pages, key, cols, cell_w, cell_h, base, partial)
                        }
                    }
                    if !partial {
                        self.inflight = None;
                    }
                    dirty = true;
                }
                WorkerEvent::Failed { gen: eg, error } if eg == gen => {
                    self.last_error = Some(error);
                    self.inflight = None;
                    dirty = true;
                }
                _ => {}
            }
        }
        dirty
    }

    fn spawn(&mut self, pending: PendingRequest) {
        self.next_gen = self.next_gen.saturating_add(1);
        let gen = self.next_gen;
        let (tx, rx) = mpsc::channel();
        let width_px = preview_width_px(pending.cols, pending.cell_w);
        let source = pending.source.clone();
        let resource = pending.resource_dir.clone();
        let key = pending.key;
        thread::Builder::new()
            .name("posit-edocview".into())
            .spawn(move || {
                // Phase 1: first page ASAP so Canvas paints without waiting on
                // the full document.
                match rasterize_preview(&source, width_px, &resource, 1) {
                    Ok(pages) if !pages.is_empty() => {
                        let _ = tx.send(WorkerEvent::Pages {
                            gen,
                            pages,
                            partial: true,
                        });
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = tx.send(WorkerEvent::Failed { gen, error });
                        return;
                    }
                }
                // Phase 2: remaining pages (cached PDF — usually much faster).
                match rasterize_preview(&source, width_px, &resource, 0) {
                    Ok(pages) if !pages.is_empty() => {
                        let _ = tx.send(WorkerEvent::Pages {
                            gen,
                            pages,
                            partial: false,
                        });
                    }
                    Ok(_) => {
                        let _ = tx.send(WorkerEvent::Failed {
                            gen,
                            error: "empty pages".into(),
                        });
                    }
                    Err(error) => {
                        let _ = tx.send(WorkerEvent::Failed { gen, error });
                    }
                }
                let _ = key; // fingerprint already enforced by caller
            })
            .ok();
        self.inflight = Some(Inflight {
            gen,
            slot: pending.slot,
            key: pending.key,
            cols: pending.cols,
            cell_w: pending.cell_w,
            cell_h: pending.cell_h,
            base_image_id: pending.base_image_id,
            rx,
        });
    }
}

fn apply_pages(
    doc: &mut Option<RetainedDoc>,
    pages: Vec<PageJson>,
    key: u64,
    cols: u16,
    cell_w: u32,
    cell_h: u32,
    base_image_id: u32,
    partial: bool,
) {
    let cell_h = cell_h.max(1);
    let mut retained = Vec::with_capacity(pages.len());
    let mut row = 0usize;
    for (index, page) in pages.into_iter().enumerate() {
        let height_rows = ((page.height as usize) + cell_h as usize - 1) / cell_h as usize;
        let height_rows = height_rows.max(1);
        // Preserve transmitted flag when the same path/id is refreshed.
        let transmitted = doc
            .as_ref()
            .and_then(|d| d.pages.get(index))
            .is_some_and(|p| p.path == PathBuf::from(&page.path) && p.transmitted);
        retained.push(RetainedPage {
            path: PathBuf::from(page.path),
            hash: page.hash,
            px_w: page.width,
            px_h: page.height,
            row,
            height_rows,
            image_id: base_image_id.saturating_add(index as u32).max(1),
            transmitted,
        });
        row = row.saturating_add(height_rows).saturating_add(PAGE_GAP_ROWS);
    }
    let total_rows = row.saturating_sub(PAGE_GAP_ROWS).max(1);
    *doc = Some(RetainedDoc {
        pages: retained,
        total_rows,
        cols,
        cell_w,
        cell_h,
        content_key: key,
        base_image_id,
        partial,
        placed: None,
    });
}

pub fn preview_width_px(cols: u16, cell_w: u32) -> u32 {
    (cols as u32)
        .saturating_mul(cell_w.max(1))
        .min(MAX_PREVIEW_WIDTH_PX)
        .max(1)
}

/// True when Kitty + edocview render toolchain (script/python/pandoc/pymupdf) work.
pub fn kitty_doc_available() -> bool {
    crate::minimap_gfx::host_is_kitty() && toolchain_ok()
}

fn toolchain_ok() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let script = render_script();
        if !script.is_file() {
            return false;
        }
        Command::new("python3")
            .args(["-c", "import fitz"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

fn render_script() -> PathBuf {
    if let Ok(path) = std::env::var("POSIT_EDOCVIEW_RENDER") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/edocview_render.py")
}

fn cache_root() -> PathBuf {
    if let Ok(path) = std::env::var("POSIT_EDOCVIEW_CACHE") {
        return PathBuf::from(path);
    }
    std::env::temp_dir().join("posit-edocview-cache")
}

/// Fingerprint of markdown source + render width so we retransmit only when needed.
pub fn content_key(source: &str, width_px: u32) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in source.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= u64::from(width_px);
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// Exact cell pixel size (edocview / image.nvim). Prefers TIOCGWINSZ.
pub fn cell_pixels(
    term_cols: u16,
    term_rows: u16,
    app_pixels: Option<(u16, u16)>,
) -> (u32, u32) {
    #[cfg(unix)]
    {
        let mut wsz: libc::winsize = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut wsz) };
        if rc == 0 && wsz.ws_col > 0 && wsz.ws_row > 0 && wsz.ws_xpixel > 0 && wsz.ws_ypixel > 0
        {
            let cw = (wsz.ws_xpixel as u32 / wsz.ws_col as u32).max(1);
            let ch = (wsz.ws_ypixel as u32 / wsz.ws_row as u32).max(1);
            return (cw, ch);
        }
    }
    if let Some((pw, ph)) = app_pixels {
        let cols = term_cols.max(1) as u32;
        let rows = term_rows.max(1) as u32;
        if pw > 0 && ph > 0 {
            return ((pw as u32 / cols).max(1), (ph as u32 / rows).max(1));
        }
    }
    (9, 18)
}

/// Background-safe raster via cached `preview` command. `max_pages` 0 = all.
pub fn rasterize_preview(
    source: &str,
    width_px: u32,
    resource_dir: &Path,
    max_pages: u32,
) -> Result<Vec<PageJson>, String> {
    let script = render_script();
    if !script.is_file() {
        return Err(format!("missing render script {}", script.display()));
    }
    let cache = cache_root();
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let stamp = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        source.hash(&mut h);
        width_px.hash(&mut h);
        max_pages.hash(&mut h);
        h.finish()
    };
    let staging = cache.join(format!("staging-{stamp}"));
    let _ = std::fs::create_dir_all(&staging);
    let md = staging.join("document.md");
    std::fs::write(&md, source).map_err(|e| e.to_string())?;

    let width_px = width_px.min(MAX_PREVIEW_WIDTH_PX).max(1);
    let output = Command::new("python3")
        .arg(&script)
        .arg("preview")
        .arg(&md)
        .arg(&cache)
        .arg(resource_dir)
        .arg(width_px.to_string())
        .arg(max_pages.to_string())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(err.trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| format!("pages json: {e}"))
}

/// Sync helper kept for tests / fallback tooling.
#[allow(dead_code)]
pub fn rasterize_markdown_pages(
    source: &str,
    width_px: u32,
    resource_dir: &Path,
) -> Result<Vec<PageJson>, String> {
    rasterize_preview(source, width_px, resource_dir, 0)
}

/// Transmit PNG bytes without placing (`a=t`).
pub fn transmit_png(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    png: &[u8],
    image_id: u32,
) -> io::Result<bool> {
    if png.is_empty() || image_id == 0 || arbiter.backend() != GraphicsBackend::Kitty {
        return Ok(false);
    }
    write!(out, "\x1b_Ga=d,d=i,i={image_id},q=2\x1b\\")?;
    let b64 = canvas_base64(png);
    let mut first = true;
    let mut rest = b64.as_str();
    while !rest.is_empty() {
        let take = rest.len().min(4096);
        let (chunk, next) = rest.split_at(take);
        let more = if next.is_empty() { 0 } else { 1 };
        if first {
            write!(
                out,
                "\x1b_Ga=t,f=100,t=d,i={image_id},q=2,m={more};{chunk}\x1b\\"
            )?;
            first = false;
        } else {
            write!(out, "\x1b_Gm={more};{chunk}\x1b\\")?;
        }
        rest = next;
    }
    out.flush()?;
    Ok(true)
}

/// Place a crop at a screen cell — source pixel crop only (no `c`/`r`).
pub fn place_crop(
    out: &mut impl Write,
    screen_col: u16,
    screen_row: u16,
    image_id: u32,
    placement_id: u32,
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
) -> io::Result<()> {
    if image_id == 0 || src_w == 0 || src_h == 0 {
        return Ok(());
    }
    write!(
        out,
        "\x1b[{};{}H\x1b_Ga=p,i={image_id},p={placement_id},x={src_x},y={src_y},w={src_w},h={src_h},C=1,z=-1,q=2\x1b\\",
        screen_row.saturating_add(1),
        screen_col.saturating_add(1),
    )?;
    out.flush()?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentStatus {
    /// Kitty crop placed (or already placed).
    Ready,
    /// Raster still running / not started — keep text fallback visible.
    Pending,
    /// Toolchain missing or hard failure.
    Unavailable,
}

/// Place retained pages only — **never** compiles/rasterizes on this thread.
pub fn present_markdown(
    out: &mut impl Write,
    arbiter: &mut GraphicsArbiter,
    doc: &mut Option<RetainedDoc>,
    source: &str,
    _resource_dir: &Path,
    inner: Rect,
    scroll_rows: usize,
    cell_w: u32,
    cell_h: u32,
    base_image_id: u32,
) -> io::Result<PresentStatus> {
    if arbiter.backend() != GraphicsBackend::Kitty || !toolchain_ok() {
        return Ok(PresentStatus::Unavailable);
    }
    if inner.width < 4 || inner.height < 2 || cell_w == 0 || cell_h == 0 {
        return Ok(PresentStatus::Unavailable);
    }
    let cols = inner.width;
    let rows = inner.height;
    let width_px = preview_width_px(cols, cell_w);
    let key = content_key(source, width_px);
    let ready = doc.as_ref().is_some_and(|d| {
        !d.pages.is_empty()
            && d.content_key == key
            && d.cols == cols
            && d.cell_w == cell_w
            && d.cell_h == cell_h
            && d.base_image_id == base_image_id
    });
    if !ready {
        return Ok(PresentStatus::Pending);
    }

    let Some(doc) = doc.as_mut() else {
        return Ok(PresentStatus::Pending);
    };
    let max_scroll = doc.total_rows.saturating_sub(rows as usize);
    let scroll = scroll_rows.min(max_scroll);
    let selected = doc
        .pages
        .iter()
        .position(|page| {
            scroll < page.row.saturating_add(page.height_rows).saturating_add(PAGE_GAP_ROWS)
        })
        .unwrap_or(doc.pages.len().saturating_sub(1));
    let page = &doc.pages[selected];
    let offset_rows = scroll
        .saturating_sub(page.row)
        .min(page.height_rows.saturating_sub(1));
    let src_y = (offset_rows as u32).saturating_mul(doc.cell_h);
    let place_key = (scroll, rows, inner.x, inner.y, cols, selected, src_y);
    if doc.placed == Some(place_key) && page.transmitted {
        return Ok(PresentStatus::Ready);
    }

    if !doc.pages[selected].transmitted {
        let png = match std::fs::read(&doc.pages[selected].path) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            _ => return Ok(PresentStatus::Pending),
        };
        let id = doc.pages[selected].image_id;
        if !transmit_png(out, arbiter, &png, id)? {
            return Ok(PresentStatus::Unavailable);
        }
        doc.pages[selected].transmitted = true;
    }

    let page = &doc.pages[selected];
    let viewport_px_h = (rows as u32).saturating_mul(doc.cell_h).max(1);
    let src_h = viewport_px_h.min(page.px_h.saturating_sub(src_y)).max(1);
    let src_w = (cols as u32)
        .saturating_mul(doc.cell_w)
        .min(page.px_w)
        .max(1);
    place_crop(
        out,
        inner.x,
        inner.y,
        page.image_id,
        1,
        0,
        src_y,
        src_w,
        src_h,
    )?;
    doc.placed = Some(place_key);
    Ok(PresentStatus::Ready)
}

pub fn clear_doc(out: &mut impl Write, doc: &mut Option<RetainedDoc>) -> io::Result<()> {
    if let Some(d) = doc.take() {
        for page in d.pages {
            canvas::delete_kitty_image(out, page.image_id)?;
        }
    }
    Ok(())
}

fn canvas_base64(bytes: &[u8]) -> String {
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
    fn crop_placement_is_not_unicode_placeholder() {
        let mut out = Vec::new();
        place_crop(&mut out, 2, 3, 43, 1, 0, 32, 800, 400).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("a=p"), "{s}");
        assert!(s.contains("i=43"), "{s}");
        assert!(s.contains("y=32"), "{s}");
        assert!(s.contains("C=1"), "{s}");
        assert!(!s.contains(",c="), "{s}");
        assert!(!s.contains(",r="), "{s}");
        assert!(!s.contains("U=1"), "{s}");
    }

    #[test]
    fn content_key_changes_with_width() {
        assert_ne!(content_key("# hi", 800), content_key("# hi", 900));
        assert_eq!(content_key("# hi", 800), content_key("# hi", 800));
    }

    #[test]
    fn preview_width_is_capped() {
        assert_eq!(preview_width_px(200, 20), MAX_PREVIEW_WIDTH_PX);
        assert_eq!(preview_width_px(40, 10), 400);
    }

    #[test]
    fn cell_pixels_falls_back_when_ioctl_unavailable() {
        let (w, h) = cell_pixels(80, 24, Some((800, 480)));
        assert_eq!(w, 10);
        assert_eq!(h, 20);
        let (w, h) = cell_pixels(80, 24, None);
        assert!(w >= 1 && h >= 1);
    }

    #[test]
    fn render_script_is_vendored() {
        let script = render_script();
        assert!(
            script.is_file(),
            "expected {} (vendored edocview render)",
            script.display()
        );
    }

    #[test]
    fn present_without_doc_is_pending_not_blocking() {
        let mut arbiter = GraphicsArbiter::default();
        arbiter.prefer_kitty_from_env();
        let mut doc = None;
        let mut out = Vec::new();
        let status = present_markdown(
            &mut out,
            &mut arbiter,
            &mut doc,
            "# hi\n",
            Path::new("."),
            Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
            0,
            10,
            20,
            80,
        )
        .unwrap();
        // Either Pending (kitty+toolchain) or Unavailable (no kitty in test).
        assert!(
            matches!(
                status,
                PresentStatus::Pending | PresentStatus::Unavailable
            ),
            "{status:?}"
        );
        assert!(doc.is_none(), "must not sync-raster into doc");
    }

    #[test]
    fn engine_request_does_not_block_and_polls() {
        let mut engine = PreviewEngine::default();
        engine.request(
            PreviewSlot::Markdown,
            "# async\n\nhello\n",
            Path::new("."),
            40,
            10,
            20,
            80,
            Duration::ZERO,
        );
        assert!(engine.busy());
        // Polling without waiting must be instant.
        let mut help = None;
        let mut md = None;
        let _ = engine.poll(&mut help, &mut md);
        assert!(md.is_none() || md.as_ref().is_some_and(|d| !d.pages.is_empty()) || engine.busy());
    }

    #[test]
    fn markdown_pipeline_smoke_when_toolchain_present() {
        if !toolchain_ok() {
            eprintln!("skip: pymupdf/python toolchain missing");
            return;
        }
        let dir = std::env::temp_dir().join("posit-edocview-test-res");
        let _ = std::fs::create_dir_all(&dir);
        let source = "# Hello\n\nThis is **bold**.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        match rasterize_preview(source, 600, &dir, 1) {
            Ok(pages) => {
                assert_eq!(pages.len(), 1);
                assert!(Path::new(&pages[0].path).is_file());
            }
            Err(err) => eprintln!("skip compile: {err}"),
        }
    }
}
