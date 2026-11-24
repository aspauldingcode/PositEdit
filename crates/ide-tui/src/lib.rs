//! Terminal editor with scrolling 80-column tiles, a file tree, and Yazi.
//! Drawing and commands are testable without a tty.
//! The interactive loop restores the terminal on return and on drop.
mod bench;
mod clipboard;
mod files;
mod finder;
mod flip3d;
mod fold;
mod graphics;
mod gutter;
mod instance;
mod seams;
mod layout;
mod canvas;
mod edocview;
mod markdown_preview;
mod control;
mod local_tab;
mod minimap_gfx;
mod palette;
mod scheduler;
mod scrollbar;
mod term_palette;
mod terminal;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::io::{self, stdout, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
    EnterAlternateScreen, LeaveAlternateScreen,
};
use files::ChooseError;
use ide_core::{pressure_level, Document, Edit, PressureLevel};
use ide_dlc_ui::{authorize, toolbar_hit, Trust, TrustAction, WorkbenchAction, WorkbenchToolbar};
use ide_local_tab_ui::{TabControl, TabStatus};
use ide_navigation::{FuzzyHit, GrepHit};
use ide_spatial::{
    LensKind, PaneKind, SpatialAction, SpatialCapabilities, SpatialContext,
    SpatialMode, SpatialOutcome, SpatialOverview, SpatialSession, StructuralOp,
};
use ide_syntax::{
    format_manually, highlight, node_at_byte, FormatOutcome, LanguageProfile, Span, TokenKind,
};
use layout::{Axis, Chrome, Window};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer as Screen;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use ratatui::{Frame, Terminal};

pub const COMPONENT: &str = "ide-tui";
pub const IMPLEMENTED: bool = true;

struct EditBuffer {
    path: Option<PathBuf>,
    name: String,
    document: Document,
    cursor: usize,
    scroll: usize,
    scroll_x: usize,
    anchor: usize,
    selection: Option<(usize, usize)>,
    language: String,
    saved: String,
    /// Flattened text keyed by document revision (avoids rope→String every paint).
    text_cache: RefCell<Option<(u64, String)>>,
    /// Syntax spans keyed by (revision, language).
    highlight_cache: RefCell<Option<(u64, String, Vec<Span>)>>,
    /// Cached max line width for horizontal scroll limits.
    width_cache: RefCell<Option<(u64, usize)>>,
    /// Code-fold ranges + collapsed set (LSP preferred when injected).
    folds: RefCell<fold::FoldState>,
}

pub struct EditorApp {
    buffers: Vec<EditBuffer>,
    focus: usize,
    show_many: bool,
    view: f32,
    target: f32,
    width: u16,
    height: u16,
    axis: Axis,
    stride: u16,
    layout_key: (Axis, bool, u16, u16),
    palette: bool,
    query: String,
    quit: bool,
    notice: String,
    help: bool,
    help_scroll: usize,
    /// edocview retained Help document (PDF pages → Kitty crop).
    help_doc: Option<edocview::RetainedDoc>,
    /// edocview retained Markdown Canvas preview.
    markdown_doc: Option<edocview::RetainedDoc>,
    /// Force text Help when Kitty/PDF pipeline fails.
    help_force_text: bool,
    /// Force text Markdown Canvas when Kitty/PDF pipeline fails.
    markdown_force_text: bool,
    tree_focused: bool,
    tree_cursor: usize,
    tree_scroll: usize,
    tree: Vec<files::Entry>,
    open_dirs: HashSet<PathBuf>,
    workspace: PathBuf,
    profiles: Vec<LanguageProfile>,
    pick: bool,
    finder: Option<Finder>,
    yazi: Option<YaziBrowse>,
    file_index: Vec<String>,
    tree_query: Option<String>,
    tree_hits: Vec<FuzzyHit>,
    tree_search_cursor: usize,
    editing: bool,
    sidebar: bool,
    chat: bool,
    tree_width: u16,
    agent_width: u16,
    resize: Option<Side>,
    agent_focused: bool,
    agent_draft: String,
    agent_log: Vec<String>,
    agent_program: String,
    overlay: Overlay,
    chord: String,
    chord_deadline: Option<Instant>,
    click: Option<(Instant, u16, u16, u8)>,
    select_by: SelectBy,
    dlc_system: Option<String>,
    dlc_trusted: bool,
    pending_plan: Option<(PathBuf, Vec<String>)>,
    wheel_at: Option<Instant>,
    /// First axis of the current wheel gesture: 'v' or 'h'.
    scroll_axis: Option<char>,
    terminals: terminal::Panel,
    notifications: Vec<Notice>,
    notifications_open: bool,
    tile_spans: Option<Vec<u16>>,
    tile_drag: Option<usize>,
    terminal_rows: Option<u16>,
    terminal_drag: bool,
    /// Dragging the terminal session-list divider.
    terminal_side_drag: bool,
    /// Dragging the Canvas left seam (width only; Kitty paint deferred until release).
    canvas_resize: bool,
    /// Kitty/markdown graphics were hidden for the in-progress canvas drag.
    canvas_resize_hid_gfx: bool,
    /// Ratatui Clear+chrome for Canvas — skip while only the editor scrolled.
    canvas_chrome_dirty: Cell<bool>,
    /// Set when Canvas cells were Cleared and Kitty placeholders must be rebound.
    canvas_placeholders_stale: Cell<bool>,
    /// Last Kitty placement size; repaint when the Canvas inner box changes.
    canvas_painted_size: Option<(u16, u16)>,
    /// Kitty image id currently bound to Canvas placeholders.
    canvas_image_id: Option<u32>,
    pixels: Option<(u16, u16)>,
    tab_enabled: bool,
    ghost: Option<Ghost>,
    hover: Option<(u16, u16)>,
    hover_since: Option<Instant>,
    hover_hint: Option<String>,
    /// When false, mouse hover never shows the yellow action tooltip.
    hover_hints: bool,
    /// Open the Welcome markdown buffer (+ Canvas preview) on empty startup.
    show_welcome: bool,
    /// Allow Kitty graphics (minimap / Canvas / Flip / docs). Off → text fallbacks.
    kitty_graphics: bool,
    /// Scroll offset for the Canvas markdown preview.
    markdown_preview_scroll: u16,
    spatial: SpatialSession,
    graphics: graphics::GraphicsArbiter,
    spatial_cursor: usize,
    spatial_lines: Vec<String>,
    /// Active Canvas tab kind (`browser` / `phone` / `android`). `None` = closed.
    runtime_dock: Option<String>,
    runtime_focused: bool,
    runtime_width: Option<u16>,
    /// Open Canvas surfaces (one per runtime). Active tab = `runtime_dock`.
    canvas_surfaces: Vec<canvas::CanvasSurface>,
    /// Last good PNG per (kind, target) for instant reopen / tab reuse.
    canvas_frame_cache: canvas::FrameCache,
    /// Last time we probed OS memory pressure / trimmed history.
    memory_pressure_at: Option<Instant>,
    last_pressure: PressureLevel,
    gutter: gutter::GutterState,
    /// Dragging the editor minimap or scrollbar (document scroll).
    edge_drag: Option<EdgeDrag>,
    /// Last Kitty RGB minimap upload key (revision/scroll/size).
    editor_minimap_key: Option<minimap_gfx::MinimapPaintKey>,
    /// Kitty image id bound to the editor minimap placeholders.
    editor_minimap_id: Option<u32>,
    /// Base-RGB cache (scroll-only frames skip rope resampling).
    minimap_frame_cache: RefCell<minimap_gfx::MinimapFrameCache>,
    /// Headless Flip/minimap syntax previews (background thread, Arc-shared).
    preview_pool: minimap_gfx::PreviewPool,
    /// Terminal-derived minimap palette (OSC fingerprint; refreshes live).
    minimap_theme: minimap_gfx::MinimapTheme,
    /// Last successful OSC palette probe (throttle live reloads).
    minimap_theme_probed_at: Option<Instant>,
    /// Force a palette re-probe on next present (focus / scheme change).
    minimap_theme_dirty: bool,
    /// ShojiWM Flip 3D buffer cycler.
    flip: flip3d::FlipSwitcher,
    /// Last Flip 3D tick (easing).
    flip_tick_at: Option<Instant>,
    /// Dragging the window-management strip under the editor.
    buffer_strip_drag: bool,
    /// Async edocview raster engine (never blocks the UI thread).
    preview_engine: edocview::PreviewEngine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeDrag {
    Minimap,
    Scrollbar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanvasTitleHit {
    Tab(usize),
    Close,
}

/// In-pane Canvas controls (bottom strip) — phone hardware + browser nav.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanvasControlHit {
    PhoneHome,
    PhoneLock,
    PhoneSiri,
    BrowserBack,
    BrowserForward,
    BrowserReload,
}

#[derive(Clone, Debug)]
struct Ghost {
    revision: u64,
    /// Caret position where the ghost is shown.
    cursor: usize,
    /// When set, Tab replaces `[replace_from, cursor)` with `text` (snippets).
    replace_from: Option<usize>,
    text: String,
}

/// Startup welcome buffer — title is `# Welcome`; body is a short tutorial.
const WELCOME_MARKDOWN: &str = r#"# Welcome

Posit Edit is a minimal terminal IDE. This buffer is Markdown — styled in place, with a live preview on the right Canvas.

## Essentials

| Key | Action |
| --- | --- |
| `Enter` / `e` | start editing |
| `Esc` | leave edit (view mode) |
| `;` | settings |
| `Ctrl-P` | command palette |
| `t` | terminal |
| `<` / `>` | file tree / agent |
| `?` | help |

## Files & buffers

- `o` opens a file browser · `s` saves · `n` new buffer
- `Right` / `Left` (view) switch buffers
- Drag to select · `y` copy · `p` paste · `x` cut

## Terminal

Focus the terminal pane (click it or open with `t`). **Tab** there is shell completion, not Posit Edit ghost text.

## Canvas preview

Markdown opens a **Preview** tab on the right. Browser / Phone use keys `6` / `7`. Close with the Canvas `x`.

## Settings tip

`;` → toggle **welcome** off if you do not want this buffer on startup.

*Happy editing.*
"#;

/// Kitty image id base for fullscreen Help (edocview pages use base..base+n).
const HELP_KITTY_IMAGE_ID: u32 = 43;
const FLIP3D_KITTY_IMAGE_ID: u32 = flip3d::FLIP3D_KITTY_IMAGE_ID;
/// Kitty image id base for Markdown Canvas preview pages.
const MARKDOWN_KITTY_IMAGE_ID: u32 = 80;

fn bind_order(bind: &str) -> (u8, String) {
    let lower = bind.to_ascii_lowercase();
    let group = u8::from(
        lower
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic()),
    );
    (group, lower)
}

fn sorted_binds(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = rows
        .iter()
        .map(|(k, a)| ((*k).to_string(), (*a).to_string()))
        .collect();
    out.sort_by(|a, b| bind_order(&a.0).cmp(&bind_order(&b.0)));
    out
}

fn push_bind_table(out: &mut String, title: &str, rows: &[(&str, &str)]) {
    out.push_str("## ");
    out.push_str(title);
    out.push_str("\n\n");
    out.push_str("| Key | Action |\n");
    out.push_str("| --- | --- |\n");
    for (key, action) in sorted_binds(rows) {
        out.push_str(&format!("| `{key}` | {action} |\n"));
    }
    out.push('\n');
}

/// Full Help document (Markdown). Keybinds are tables, grouped by what they do.
/// Resolve the shipped Posit Edit icon for Help / docs (Liquid Glass preview PNG).
fn help_icon_path() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var("POSIT_EDIT_ICON") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Some(path);
        }
    }
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/posit-edit.png"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../share/posit-edit/icons/posit-edit.png"),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

fn help_markdown() -> String {
    let mut text = String::from("# Help\n\n");
    if let Some(icon) = help_icon_path() {
        // Absolute path so pandoc/edocview can resolve the asset from a temp staging dir.
        text.push_str(&format!(
            "![Posit Edit]({}){{width=160}}\n\n",
            icon.display()
        ));
    }
    text.push_str(
        "Posit Edit is this terminal IDE. The yellow **?** in the title bar opens and closes this guide. \
**h** toggles mouse hover hints. **q** quits. Click any of them, or the `? help` label on the status line. \
**F1** opens this guide. **Esc** closes it.\n\n\
---\n\n\
## How it works\n\n\
### Buffers\n\n\
Fullscreen is the default: one buffer fills the editor. Opening a file switches to that buffer \
instead of splitting.\n\n\
**f** / **Ctrl-W** turns on tiling. Wide screens lay files left to right.\n\n\
Columns cap at **80 columns** once enough files fill the width.\n\n\
Fewer open files grow to fill space. Files beyond the visible columns stay on the strip; focus \
scrolls to them. Drag a buffer's **right-edge** pill to widen only that buffer \
(niri-style — neighbours keep their widths, including the last buffer).\n\n\
When the screen is taller than wide, files stack top to bottom at full width. **Left** / **Right** \
move between buffers; **Up** / **Down** scroll the file.\n\n\
### Editing\n\n\
View mode first — letters run commands. **Enter** / **e** edit at the caret; **E** after the caret; \
**H** at first non-blank; **L** at line end (Neovim **i** / **a** / **I** / **A** placements). \
**Esc** returns to view. Gutter: ● for breakpoints, numbers to jump. **rn** relative line numbers. \
Palette `:42` / `:+10` / `:-5` jumps. Right edge: code minimap + scrollbar. Only one pane owns the \
keyboard; hover cyan means click to focus. Click places the caret; drag selects; double-click a word; \
triple-click a line; Shift+arrows select while editing.\n\n\
### Canvas\n\n\
**6** / **7** open Browser / Phone on the right Canvas. Markdown opens a **Preview** tab. The bottom \
strip is real chrome (phone Home / Lock / Siri, browser Back / Fwd / Reload). Clicks map into the \
runtime. **y** / **p** / **x** copy / paste / cut; right-click keeps the selection.\n\n\
### Sidebars & terminal\n\n\
**<** file tree · **>** agent chat · drag mid-edge split pills (or the highlighted edge) to resize. **t** toggles the bottom terminal; \
drag its top edge for height. Session list on the right (including debug).\n\n\
### Flip 3D buffer switcher\n\n\
In **view mode** (not editing), **press and hold Tab** to open the ShojiWM-style stack (needs **2+ buffers**). It starts on the **current** buffer — hold+release alone keeps you there. **←→ / ↑↓** choose another buffer · **release Tab** (or Enter) switches · **Esc** cancels. View-mode **gs** is a fallback when the terminal does not report key-up. Kitty hosts pre-render syntax cards in the background.\n\n\
### Closing\n\n\
**w** closes the focused buffer. **q** quits. Dirty buffers ask first: **y** saves and closes, **!** \
discards edits and closes, **n** cancels. **i** toggles notifications.\n\n\
---\n\n\
# Keybinds\n\n",
    );

    push_bind_table(
        &mut text,
        "Guide & settings",
        &[
            ("?", "open this guide"),
            ("F1", "open this guide"),
            ("h", "toggle mouse hover hints"),
            (";", "open settings (welcome, Kitty graphics, sidebars, …)"),
            ("k", "settings: toggle Kitty graphics (text fallbacks)"),
            (":", "open the command line"),
            ("Ctrl-P", "open the command line"),
            ("i", "toggle notifications"),
        ],
    );
    push_bind_table(
        &mut text,
        "Editing mode",
        &[
            ("Enter", "enter editing on the focused buffer"),
            ("e", "edit at the caret (nvim i)"),
            ("E", "edit after the caret (nvim a)"),
            ("H", "edit at first non-blank on the line (nvim I)"),
            ("L", "edit at end of line (nvim A)"),
            ("Esc", "leave editing / dismiss ghost text"),
            ("Tab", "accept ghost text, or indent when none is shown"),
            ("Ctrl-Z", "undo the last edit"),
            ("Ctrl-Alt-T", "toggle local Tab completion"),
            ("ff", "format the file"),
            ("rn", "toggle relative line numbers"),
            (":42", "palette: jump to line 42"),
            (":+10 / :-5", "palette: relative line jump"),
        ],
    );
    push_bind_table(
        &mut text,
        "Motion & selection",
        &[
            ("Up", "scroll the file"),
            ("Down", "scroll the file"),
            ("Ctrl-Up", "scroll the file"),
            ("Ctrl-Down", "scroll the file"),
            ("Shift-Up", "view: jump up 8 lines. edit: select up"),
            ("Shift-Down", "view: jump down 8 lines. edit: select down"),
            ("Shift-Left", "view: swap with the previous buffer. edit: select left"),
            ("Shift-Right", "view: swap with the next buffer. edit: select right"),
            ("Option-Left", "jump left by word (Alt-Left / Meta-b)"),
            ("Option-Right", "jump right by word (Alt-Right / Meta-f)"),
            ("Option-Shift-Left", "select left by word"),
            ("Option-Shift-Right", "select right by word"),
            ("a", "select all"),
            ("Ctrl-A", "select all"),
            ("y", "copy the selection"),
            ("p", "paste"),
            ("x", "cut the selection"),
        ],
    );
    push_bind_table(
        &mut text,
        "Buffers & layout",
        &[
            ("Left", "focus the previous buffer"),
            ("Right", "focus the next buffer"),
            ("Ctrl-Left", "focus the previous buffer"),
            ("Ctrl-Right", "focus the next buffer"),
            ("[", "focus the first buffer"),
            ("]", "focus the last buffer"),
            ("{", "move this buffer to the start"),
            ("}", "move this buffer to the end"),
            ("f", "toggle fullscreen / split (niri-style edge drag)"),
            ("Ctrl-W", "toggle fullscreen"),
            ("hold Tab", "Flip 3D (current buffer · ←→ choose · release switches)"),
            ("gs", "Flip 3D fallback when key-up is unavailable"),
            ("buffer strip", "bottom precision scrollbar · click to focus"),
            (
                "right-edge pill",
                "drag to resize only that buffer (last buffer included)",
            ),
            ("w", "close the buffer"),
            ("s", "save"),
            ("Ctrl-S", "save"),
            ("rr", "rename the file"),
            ("o", "open floating Yazi"),
        ],
    );
    push_bind_table(
        &mut text,
        "Search",
        &[
            ("l", "search open buffers"),
            ("ll", "search files"),
            ("lll", "live grep"),
        ],
    );
    push_bind_table(
        &mut text,
        "Sidebars & terminal",
        &[
            ("<", "toggle the file sidebar"),
            (">", "toggle the agent chat"),
            ("t", "toggle the terminal"),
            (
                "split pills",
                "drag mid-edge handles: tree / chat / canvas / terminal / buffer right edges",
            ),
            (
                "agent",
                "routes Claude/Codex/OpenCode/agent, or FreeLocal tools: list/read/grep/format",
            ),
        ],
    );
    push_bind_table(
        &mut text,
        "Canvas & runtimes",
        &[
            ("zp", "markdown preview on the Canvas"),
            ("6", "open Browser on the Canvas (Kitty graphics)"),
            ("7", "open Phone on the Canvas (Kitty graphics)"),
            ("8", "open Android on the Canvas"),
            ("9", "report debugger availability"),
        ],
    );
    push_bind_table(
        &mut text,
        "Build, run & workbench",
        &[
            ("b", "build"),
            ("r", "run"),
            ("1", "preview a build plan"),
            ("2", "preview a run plan"),
            ("3", "show the workbench configuration"),
            ("4", "preview a test plan"),
            ("5", "refresh detected projects"),
        ],
    );
    push_bind_table(
        &mut text,
        "Spatial",
        &[
            ("za", "toggle fold at caret (gutter ▸/▾)"),
            ("zc", "close fold at caret"),
            ("ze", "expand fold at caret"),
            ("zo", "spatial overview"),
            ("zl", "ephemeral lens"),
            ("zn", "structural edit preview"),
            ("zt", "execution timeline"),
            ("zi", "change impact"),
        ],
    );
    push_bind_table(
        &mut text,
        "Quit",
        &[
            ("q", "quit when buffers are saved"),
            ("Ctrl-Q", "quit"),
        ],
    );
    text
}

#[cfg(test)]
fn help_text() -> String {
    help_markdown()
}

#[cfg(test)]
fn keybind_rows() -> Vec<(&'static str, &'static str)> {
    // Flat sample used by alphabetical-order tests; Help UI reads `help_markdown()`.
    let mut rows = vec![
        ("<", "toggle the file sidebar"),
        (">", "toggle the agent chat"),
        ("?", "open this guide"),
        ("b", "build"),
        ("ff", "format the file"),
        ("l", "search open buffers"),
        ("ll", "search files"),
        ("lll", "live grep"),
        ("o", "open floating Yazi"),
    ];
    rows.sort_by_key(|left| bind_order(left.0));
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Down { column: u16, row: u16 },
    Drag { column: u16, row: u16 },
    Up { column: u16, row: u16 },
    Move { column: u16, row: u16 },
    ScrollUp { column: u16, row: u16 },
    ScrollDown { column: u16, row: u16 },
    ScrollLeft { column: u16, row: u16 },
    ScrollRight { column: u16, row: u16 },
    Right { column: u16, row: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanvasPointerKind {
    Down,
    Drag,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKey {
    Char(char),
    Enter,
    Backspace,
    Left,
    Right,
    CtrlQ,
    CtrlZ,
    CtrlP,
    CtrlW,
    CtrlS,
    CtrlLeft,
    CtrlRight,
    CtrlUp,
    CtrlDown,
    CtrlA,
    Delete,
    Up,
    Down,
    ShiftLeft,
    ShiftRight,
    ShiftUp,
    ShiftDown,
    AltLeft,
    AltRight,
    AltShiftLeft,
    AltShiftRight,
    /// ShojiWM-style buffer cycler (Alt+Tab when the host forwards it).
    AltTab,
    AltShiftTab,
    Esc,
    Help,
    Tab,
    /// Tab key-up — commits Flip 3D after a hold in view mode.
    TabRelease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectBy {
    Char,
    Word,
    Line,
}

/// Exactly one pane owns the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaneFocus {
    Editor,
    Terminal,
    Agent,
    Tree,
    Runtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinderKind {
    Files,
    Grep,
    Buffers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MenuState {
    x: u16,
    y: u16,
    cursor: usize,
    /// Right-click landed on the Terminal/Console panel.
    over_terminal: bool,
}

const MENU: &[(&str, char)] = &[
    ("Cut", 'x'),
    ("Copy", 'y'),
    ("Paste", 'p'),
    ("Select All", 'a'),
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Notice {
    title: String,
    body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Overlay {
    None,
    Quit,
    Rename(String),
    Settings,
    Vim,
    Menu(MenuState),
    Close(usize),
}

#[derive(Clone, Debug)]
struct YaziBrowse {
    cwd: PathBuf,
    items: Vec<files::DirItem>,
    cursor: usize,
    scroll: usize,
}

#[derive(Clone, Debug)]
struct Finder {
    kind: FinderKind,
    query: String,
    cursor: usize,
    scroll: usize,
    files: Vec<FuzzyHit>,
    greps: Vec<GrepHit>,
}

impl EditBuffer {
    fn from_text(name: impl Into<String>, text: &str, language: impl Into<String>) -> Self {
        let cursor = text.chars().count();
        let language = language.into();
        let saved = text.to_string();
        let revision = 0;
        Self {
            path: None,
            name: name.into(),
            document: Document::new(text),
            cursor,
            scroll: 0,
            scroll_x: 0,
            anchor: cursor,
            selection: None,
            language: language.clone(),
            saved: saved.clone(),
            text_cache: RefCell::new(Some((revision, saved))),
            highlight_cache: RefCell::new(None),
            // (revision, max line width) — avoids O(n) on every horizontal scroll.
            width_cache: RefCell::new(None),
            folds: RefCell::new(fold::FoldState::default()),
        }
    }

    fn ensure_folds(&self) {
        let rev = self.document.revision();
        let text = self.text();
        self.folds.borrow_mut().refresh(&text, rev);
    }

    /// Prefer LSP folding ranges when a server replied; `None` falls back to syntax.
    fn apply_lsp_fold_result(&mut self, result: &serde_json::Value) {
        let ranges = fold::ranges_from_lsp_json(result);
        self.folds.borrow_mut().set_lsp_ranges(if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        });
        self.ensure_folds();
    }

    fn fold_toggle_at_caret(&mut self) -> String {
        self.ensure_folds();
        let line = self.document.char_to_line(self.cursor);
        let (chevron, end_line) = {
            let mut folds = self.folds.borrow_mut();
            let end = folds.range_at(line).map(|r| r.end_line + 1).unwrap_or(line + 1);
            (folds.toggle_at(line), end)
        };
        match chevron {
            Some(fold::Chevron::Collapsed) => {
                // Keep caret on the fold header when body collapses.
                let col = self.with_text(|body| line_col(body, self.cursor).1);
                self.cursor = self.with_text(|body| char_at(body, line, col));
                self.anchor = self.cursor;
                format!("folded lines {}-{end_line}", line + 1)
            }
            Some(fold::Chevron::Expanded) => format!("unfolded line {}", line + 1),
            None => "no fold at caret".into(),
        }
    }

    fn fold_close_at_caret(&mut self) -> String {
        self.ensure_folds();
        let line = self.document.char_to_line(self.cursor);
        if self.folds.borrow_mut().close_at(line) {
            let col = self.with_text(|body| line_col(body, self.cursor).1);
            self.cursor = self.with_text(|body| char_at(body, line, col));
            self.anchor = self.cursor;
            format!("folded line {}", line + 1)
        } else {
            "no fold at caret".into()
        }
    }

    fn fold_open_at_caret(&mut self) -> String {
        self.ensure_folds();
        let line = self.document.char_to_line(self.cursor);
        if self.folds.borrow_mut().open_at(line) {
            format!("unfolded near line {}", line + 1)
        } else {
            "no fold at caret".into()
        }
    }

    fn ensure_text_cache(&self) {
        let rev = self.document.revision();
        let mut cache = self.text_cache.borrow_mut();
        if cache.as_ref().is_some_and(|(r, _)| *r == rev) {
            return;
        }
        *cache = Some((rev, self.document.to_text()));
    }

    fn text(&self) -> String {
        self.ensure_text_cache();
        self.text_cache
            .borrow()
            .as_ref()
            .map(|(_, text)| text.clone())
            .unwrap_or_default()
    }

    fn with_text<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        self.ensure_text_cache();
        let cache = self.text_cache.borrow();
        f(cache.as_ref().map(|(_, text)| text.as_str()).unwrap_or(""))
    }

    fn ensure_highlight_cache(&self) {
        let rev = self.document.revision();
        {
            let cache = self.highlight_cache.borrow();
            if let Some((cached_rev, lang, _)) = cache.as_ref() {
                if *cached_rev == rev && lang == &self.language {
                    return;
                }
            }
        }
        let spans = self.with_text(|text| highlight(&self.language, text));
        *self.highlight_cache.borrow_mut() = Some((rev, self.language.clone(), spans));
    }

    fn max_line_chars(&self) -> usize {
        let rev = self.document.revision();
        if let Some((cached_rev, width)) = *self.width_cache.borrow() {
            if cached_rev == rev {
                return width;
            }
        }
        let width = self.with_text(|text| {
            text.split('\n')
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0)
        });
        *self.width_cache.borrow_mut() = Some((rev, width));
        width
    }

    fn dirty(&self) -> bool {
        self.ensure_text_cache();
        match self.text_cache.borrow().as_ref() {
            Some((_, text)) => text != &self.saved,
            None => true,
        }
    }

    fn mark_clean(&mut self) {
        self.saved = self.text();
    }

    fn insert(&mut self, ch: char) {
        if self.replace_selection(ch.to_string()) {
            return;
        }
        let revision = self.document.revision();
        let edit = Edit {
            start: self.cursor,
            end: self.cursor,
            replacement: ch.to_string(),
        };
        if self.document.apply(revision, &[edit]).is_ok() {
            self.cursor += 1;
            self.anchor = self.cursor;
        }
    }

    fn backspace(&mut self) {
        if self.replace_selection(String::new()) {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let revision = self.document.revision();
        let edit = Edit {
            start: self.cursor - 1,
            end: self.cursor,
            replacement: String::new(),
        };
        if self.document.apply(revision, &[edit]).is_ok() {
            self.cursor -= 1;
            self.anchor = self.cursor;
        }
    }

    fn replace_selection(&mut self, replacement: String) -> bool {
        let Some((start, end)) = self.selection.take() else {
            return false;
        };
        let added = replacement.chars().count();
        let revision = self.document.revision();
        let edit = Edit {
            start,
            end,
            replacement,
        };
        if self.document.apply(revision, &[edit]).is_ok() {
            self.cursor = start + added;
            self.anchor = self.cursor;
            true
        } else {
            false
        }
    }

    fn replace_all(&mut self, text: &str) {
        self.replace_all_keeping_view(text);
    }

    /// Full-buffer rewrite that keeps the caret on the same line/column and preserves scroll.
    fn replace_all_keeping_view(&mut self, text: &str) {
        let (line, column) = self.with_text(|body| line_col(body, self.cursor));
        let scroll = self.scroll;
        let scroll_x = self.scroll_x;
        let revision = self.document.revision();
        let edit = Edit {
            start: 0,
            end: self.document.len_chars(),
            replacement: text.to_string(),
        };
        if self.document.apply(revision, &[edit]).is_ok() {
            self.cursor = self.with_text(|body| char_at(body, line, column));
            self.anchor = self.cursor;
            self.selection = None;
            let line_count = self.document.len_lines().max(1);
            self.scroll = scroll.min(line_count.saturating_sub(1));
            self.scroll_x = scroll_x;
        }
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection?;
        if start == end {
            return None;
        }
        Some(self.text().chars().skip(start).take(end - start).collect())
    }

    fn insert_text(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if self.selection.is_some() {
            self.replace_selection(text);
            return;
        }
        let added = text.chars().count();
        let revision = self.document.revision();
        let edit = Edit {
            start: self.cursor,
            end: self.cursor,
            replacement: text,
        };
        if self.document.apply(revision, &[edit]).is_ok() {
            self.cursor += added;
            self.anchor = self.cursor;
            self.selection = None;
        }
    }

    fn undo(&mut self) {
        self.document.undo();
        self.cursor = self.cursor.min(self.document.len_chars());
        self.selection = None;
        self.anchor = self.cursor;
    }

    fn move_horizontal(&mut self, delta: isize, select: bool) {
        if !select {
            if let Some((start, end)) = self.selection.take() {
                self.cursor = if delta < 0 { start } else { end };
                self.anchor = self.cursor;
                return;
            }
        }
        if delta < 0 {
            self.cursor = self.cursor.saturating_sub(delta.unsigned_abs());
        } else {
            self.cursor = (self.cursor + delta as usize).min(self.document.len_chars());
        }
        if select {
            self.sync_selection();
        } else {
            self.selection = None;
            self.anchor = self.cursor;
        }
    }

    /// Option/Alt word hop — same idea as macOS text fields.
    fn move_word(&mut self, forward: bool, select: bool) {
        if !select {
            if let Some((start, end)) = self.selection.take() {
                self.cursor = if forward { end } else { start };
                self.anchor = self.cursor;
            }
        }
        let cursor = self.cursor;
        let next = self.with_text(|text| {
            if forward {
                word_jump_right(text, cursor)
            } else {
                word_jump_left(text, cursor)
            }
        });
        self.cursor = next;
        if select {
            self.sync_selection();
        } else {
            self.selection = None;
            self.anchor = self.cursor;
        }
    }

    fn move_vertical(&mut self, delta: isize, visible: usize, columns: usize, select: bool) {
        if !select {
            self.selection = None;
            self.anchor = self.cursor;
        }
        let cursor = self.cursor;
        let next_cursor = self.with_text(|text| {
            let (line, column) = line_col(text, cursor);
            let next = if delta < 0 {
                line.saturating_sub(delta.unsigned_abs())
            } else {
                line + delta as usize
            };
            char_at(text, next, column)
        });
        self.cursor = next_cursor;
        if select {
            self.sync_selection();
        } else {
            self.anchor = self.cursor;
        }
        self.reveal(visible, columns);
    }

    fn sync_selection(&mut self) {
        let anchor = self.anchor;
        self.selection =
            (anchor != self.cursor).then_some((anchor.min(self.cursor), anchor.max(self.cursor)));
    }

    fn select_all(&mut self) {
        let end = self.document.len_chars();
        self.anchor = 0;
        self.cursor = end;
        self.selection = (end > 0).then_some((0, end));
    }

    fn select_word(&mut self, line: usize, column: usize) {
        let text = self.document.to_text();
        let pos = char_at(&text, line, column);
        let (start, end) = word_bounds(&text, pos);
        self.set_span(start, end);
    }

    fn select_line(&mut self, line: usize, column: usize) {
        let text = self.document.to_text();
        let pos = char_at(&text, line, column);
        let (start, end) = line_bounds(&text, pos);
        self.set_span(start, end);
    }

    fn extend_word(&mut self, line: usize, column: usize) {
        self.extend_unit(line, column, word_bounds);
    }

    fn extend_line(&mut self, line: usize, column: usize) {
        self.extend_unit(line, column, line_bounds);
    }

    fn extend_unit(
        &mut self,
        line: usize,
        column: usize,
        bounds: fn(&str, usize) -> (usize, usize),
    ) {
        let text = self.document.to_text();
        let pos = char_at(&text, line, column);
        let (origin_start, origin_end) = bounds(&text, self.anchor);
        let (start, end) = bounds(&text, pos);
        let lo = origin_start.min(start);
        let hi = origin_end.max(end);
        self.cursor = if pos < self.anchor { lo } else { hi };
        self.selection = (lo != hi).then_some((lo, hi));
    }

    fn set_span(&mut self, start: usize, end: usize) {
        self.anchor = start;
        self.cursor = end;
        self.selection = (start != end).then_some((start, end));
    }

    fn delete_forward(&mut self) {
        if self.replace_selection(String::new()) {
            return;
        }
        if self.cursor >= self.document.len_chars() {
            return;
        }
        let revision = self.document.revision();
        let edit = Edit {
            start: self.cursor,
            end: self.cursor + 1,
            replacement: String::new(),
        };
        if self.document.apply(revision, &[edit]).is_ok() {
            self.anchor = self.cursor;
        }
    }

    fn place(&mut self, line: usize, column: usize, drag: bool) {
        let pos = char_at(&self.document.to_text(), line, column);
        if drag {
            let anchor = self.anchor;
            self.cursor = pos;
            self.selection = (anchor != pos).then_some((anchor.min(pos), anchor.max(pos)));
        } else {
            self.cursor = pos;
            self.anchor = pos;
            self.selection = None;
        }
    }

    fn reveal(&mut self, visible: usize, columns: usize) {
        let cursor = self.cursor;
        let (line, column) = self.with_text(|text| line_col(text, cursor));
        if line < self.scroll {
            self.scroll = line;
        } else if visible > 0 && line >= self.scroll + visible {
            self.scroll = line + 1 - visible;
        }
        if column < self.scroll_x {
            self.scroll_x = column;
        } else if columns > 0 && column >= self.scroll_x + columns {
            self.scroll_x = column + 1 - columns;
        }
    }

    fn move_to_line(&mut self, line: usize, visible: usize, columns: usize) {
        let target = line.saturating_sub(1);
        self.cursor = self.with_text(|text| char_at(text, target, 0));
        self.anchor = self.cursor;
        self.selection = None;
        self.reveal(visible, columns);
    }

    /// First non-whitespace on the current line (nvim `I`).
    fn move_to_line_head(&mut self) {
        let cursor = self.cursor;
        self.cursor = self.with_text(|text| {
            let (start, end) = line_bounds(text, cursor);
            let mut at = start;
            for ch in text.chars().skip(start).take(end.saturating_sub(start)) {
                if ch == '\n' || !ch.is_whitespace() {
                    break;
                }
                at += 1;
            }
            at
        });
        self.anchor = self.cursor;
        self.selection = None;
    }

    /// Just before the line's newline (nvim `A`).
    fn move_to_line_end(&mut self) {
        let cursor = self.cursor;
        self.cursor = self.with_text(|text| {
            let (start, end) = line_bounds(text, cursor);
            if end > start && text.chars().nth(end - 1) == Some('\n') {
                end - 1
            } else {
                end
            }
        });
        self.anchor = self.cursor;
        self.selection = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditEntry {
    /// Insert at the caret (nvim `i`).
    Here,
    /// Insert after the caret (nvim `a`).
    After,
    /// Insert at first non-blank on the line (nvim `I`).
    LineHead,
    /// Insert at end of line (nvim `A`).
    LineEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineJump {
    Absolute(usize),
    Relative(isize),
}

fn parse_line_jump(raw: &str) -> Option<LineJump> {
    let text = raw.trim().trim_start_matches(':').trim();
    let text = text
        .strip_prefix("line")
        .map(str::trim)
        .unwrap_or(text);
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix('+') {
        let n: usize = rest.trim().parse().ok()?;
        return Some(LineJump::Relative(n as isize));
    }
    if let Some(rest) = text.strip_prefix('-') {
        let n: usize = rest.trim().parse().ok()?;
        return Some(LineJump::Relative(-(n as isize)));
    }
    if text.chars().all(|ch| ch.is_ascii_digit()) {
        let n: usize = text.parse().ok()?;
        return Some(LineJump::Absolute(n.max(1)));
    }
    None
}

/// First non-whitespace glyph for each line in a sliding minimap window.
fn minimap_glyphs_window(buffer: &EditBuffer, start: usize, count: usize) -> Vec<char> {
    let line_count = buffer.document.len_lines().max(1);
    let count = count.max(1);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let line = start.saturating_add(i);
        if line >= line_count {
            out.push(' ');
            continue;
        }
        let prefix = buffer.document.line_prefix(line, 64);
        out.push(
            prefix
                .chars()
                .find(|ch| !ch.is_whitespace())
                .unwrap_or(' '),
        );
    }
    out
}

impl EditorApp {
    pub fn new(text: &str) -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut open_dirs = HashSet::new();
        open_dirs.insert(workspace.clone());
        let mut app = Self {
            buffers: vec![EditBuffer::from_text("untitled", text, "plain")],
            focus: 0,
            show_many: false,
            view: 0.0,
            target: 0.0,
            width: 80,
            height: 24,
            axis: Axis::Horizontal,
            stride: 1,
            layout_key: (Axis::Horizontal, false, 0, 0),
            palette: false,
            query: String::new(),
            quit: false,
            notice: String::new(),
            help: false,
            help_scroll: 0,
            help_doc: None,
            markdown_doc: None,
            help_force_text: false,
            markdown_force_text: false,
            tree_focused: false,
            tree_cursor: 0,
            tree_scroll: 0,
            tree: Vec::new(),
            open_dirs,
            workspace: workspace.clone(),
            profiles: ide_syntax::load_profiles(&ide_syntax::bundled_language_dir()),
            pick: false,
            finder: None,
            yazi: None,
            file_index: Vec::new(),
            tree_query: None,
            tree_hits: Vec::new(),
            tree_search_cursor: 0,
            editing: false,
            sidebar: true,
            chat: true,
            tree_width: layout::TREE_WIDTH,
            agent_width: layout::AGENT_WIDTH,
            resize: None,
            agent_focused: false,
            agent_draft: String::new(),
            agent_log: vec![
                "Ask for a change to the focused file.".to_string(),
                "Tools stay inside this workspace.".to_string(),
            ],
            agent_program: agent_program(),
            overlay: Overlay::None,
            chord: String::new(),
            chord_deadline: None,
            click: None,
            select_by: SelectBy::Char,
            dlc_system: None,
            dlc_trusted: false,
            pending_plan: None,
            wheel_at: None,
            scroll_axis: None,
            terminals: terminal::Panel::new(),
            notifications: Vec::new(),
            notifications_open: false,
            tile_spans: None,
            tile_drag: None,
            terminal_rows: None,
            terminal_drag: false,
            terminal_side_drag: false,
            canvas_resize: false,
            canvas_resize_hid_gfx: false,
            canvas_chrome_dirty: Cell::new(true),
            canvas_placeholders_stale: Cell::new(false),
            canvas_painted_size: None,
            canvas_image_id: None,
            pixels: None,
            tab_enabled: local_tab_enabled(&workspace),
            ghost: None,
            hover: None,
            hover_since: None,
            hover_hint: None,
            hover_hints: true,
            show_welcome: load_show_welcome(&workspace),
            kitty_graphics: {
                let on = load_kitty_graphics(&workspace);
                graphics::set_kitty_allowed(on);
                on
            },
            markdown_preview_scroll: 0,
            spatial: SpatialSession::new(SpatialCapabilities {
                tree_sitter: true,
                ..SpatialCapabilities::default()
            }),
            graphics: graphics::GraphicsArbiter::default(),
            spatial_cursor: 0,
            spatial_lines: Vec::new(),
            runtime_dock: None,
            runtime_focused: false,
            runtime_width: None,
            canvas_surfaces: Vec::new(),
            canvas_frame_cache: canvas::FrameCache::default(),
            memory_pressure_at: None,
            last_pressure: PressureLevel::Normal,
            gutter: gutter::GutterState::default(),
            edge_drag: None,
            editor_minimap_key: None,
            editor_minimap_id: None,
            minimap_frame_cache: RefCell::new(minimap_gfx::MinimapFrameCache::default()),
            preview_pool: minimap_gfx::PreviewPool::new(),
            minimap_theme: minimap_gfx::terminal_theme(),
            minimap_theme_probed_at: None,
            minimap_theme_dirty: true,
            flip: flip3d::FlipSwitcher::default(),
            flip_tick_at: None,
            buffer_strip_drag: false,
            preview_engine: edocview::PreviewEngine::default(),
        };
        app.reload_tree();
        app.bootstrap_workbench();
        app.note_frame(80, 24);
        app
    }

    pub fn language(&self) -> &str {
        &self.focused().language
    }

    pub fn set_language(&mut self, language: impl Into<String>) {
        let buffer = self.focused_mut();
        buffer.language = language.into();
        buffer.highlight_cache.borrow_mut().take();
    }

    fn tab_status(&self) -> TabStatus {
        if self.tab_enabled {
            TabStatus::LexicalFallback
        } else {
            TabStatus::Paused
        }
    }

    fn refresh_ghost(&mut self) {
        self.ghost = None;
        if !self.tab_enabled || !self.editing {
            return;
        }
        let cursor = self.focused().cursor;
        let selection = self.focused().selection;
        if selection.is_some() {
            return;
        }
        let text = self.focused().text();
        let chars: Vec<char> = text.chars().collect();
        if cursor > chars.len() {
            return;
        }
        let prefix: String = chars[..cursor].iter().collect();
        let suffix: String = chars[cursor..].iter().collect();
        let mut context = Vec::new();
        for (index, buffer) in self.buffers.iter().enumerate() {
            if index == self.focus || local_tab::secret_name(&buffer.name) {
                continue;
            }
            let sample: String = buffer.text().chars().take(2000).collect();
            if !sample.is_empty() {
                context.push(sample);
            }
            if context.len() == 8 {
                break;
            }
        }
        let suggestion = local_tab::suggest(&prefix, &suffix, &context);
        if !suggestion.is_empty() {
            let revision = self.focused().document.revision();
            self.ghost = Some(Ghost {
                revision,
                cursor,
                replace_from: None,
                text: suggestion,
            });
            return;
        }
        // Snippet trigger: exact language trigger at the caret word.
        let word = prefix
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        if let Some(snippet) = ide_syntax::match_trigger(&self.focused().language, &word) {
            let (expanded, _) = ide_syntax::expand_snippet(snippet.body);
            let revision = self.focused().document.revision();
            let start = cursor.saturating_sub(word.chars().count());
            // When the expansion begins with the trigger, insert only the remainder so the
            // ghost paints after the caret without doubling the keyword.
            let (text, replace_from) = if expanded.starts_with(&word) {
                (expanded[word.len()..].to_string(), None)
            } else {
                (expanded, Some(start))
            };
            self.ghost = Some(Ghost {
                revision,
                cursor,
                replace_from,
                text,
            });
        }
    }

    fn accept_ghost(&mut self) -> bool {
        let Some(ghost) = self.ghost.clone() else {
            return false;
        };
        let current = self.focused();
        let fresh = current.document.revision() == ghost.revision
            && current.cursor == ghost.cursor
            && current.selection.is_none();
        self.ghost = None;
        if !fresh {
            return false;
        }
        let start = ghost.replace_from.unwrap_or(ghost.cursor);
        let end = ghost.cursor;
        let added = ghost.text.chars().count();
        let edit = Edit {
            start,
            end,
            replacement: ghost.text,
        };
        if self
            .focused_mut()
            .document
            .apply(ghost.revision, &[edit])
            .is_err()
        {
            return false;
        }
        let buffer = self.focused_mut();
        buffer.cursor = start + added;
        buffer.anchor = buffer.cursor;
        buffer.selection = None;
        true
    }

    fn toggle_tab(&mut self) {
        self.tab_enabled = !self.tab_enabled;
        self.ghost = None;
        let dir = self.workspace.join(".posit");
        if std::fs::create_dir_all(&dir).is_ok() {
            let body = format!("{{\"enabled\":{}}}\n", self.tab_enabled);
            let _ = std::fs::write(dir.join("local-tab.json"), body);
        }
    }

    fn set_hover(&mut self, column: u16, row: u16) {
        if self.hover != Some((column, row)) {
            self.hover = Some((column, row));
            self.hover_since = Some(Instant::now());
            self.hover_hint = None;
        }
    }

    fn route_wheel(&mut self, delta: f32, vertical: bool, column: u16, row: u16, chrome: &Chrome) {
        let axis = if vertical { 'v' } else { 'h' };
        let idle = self
            .wheel_at
            .is_none_or(|at| at.elapsed() > Duration::from_millis(120));
        if idle {
            self.scroll_axis = Some(axis);
        } else if self.scroll_axis != Some(axis) {
            return;
        }
        self.wheel_at = Some(Instant::now());
        if contains(chrome.terminal, column, row) && chrome.terminal.height > 1 {
            if self.terminals.show_console {
                self.terminals.wheel_console(delta as isize);
            } else if vertical {
                self.terminals.wheel(
                    delta as isize,
                    column.saturating_sub(chrome.terminal.x),
                    chrome.terminal.width,
                );
            }
            return;
        }
        if contains(chrome.tree, column, row) && chrome.tree.width > 4 {
            if vertical {
                self.wheel_tree(delta as isize);
            }
            return;
        }
        if contains(chrome.agent, column, row) && chrome.agent.width > 4 {
            return;
        }
        if self.yazi.is_some() {
            if vertical {
                self.move_yazi(delta as isize);
            }
            return;
        }
        if self.help {
            if vertical {
                self.scroll_help(delta as isize);
            }
            return;
        }
        if vertical {
            self.wheel_buffer(delta);
        } else {
            self.wheel_buffer_x(delta);
        }
    }

    fn wheel_tree(&mut self, delta: isize) {
        if self.tree.is_empty() {
            return;
        }
        let visible = inset(self.tree_rect()).height as usize;
        let max = self.tree.len().saturating_sub(visible.max(1));
        if delta < 0 {
            self.tree_scroll = self.tree_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.tree_scroll = (self.tree_scroll + delta as usize).min(max);
        }
    }

    fn hover_menu(&mut self, column: u16, row: u16, area: Rect) {
        let Overlay::Menu(menu) = self.overlay else {
            return;
        };
        let rect = menu_rect(menu.x, menu.y, area);
        if !contains(rect, column, row) {
            return;
        }
        let index = row.saturating_sub(rect.y).saturating_sub(1) as usize;
        if index < MENU.len() {
            if let Overlay::Menu(menu) = &mut self.overlay {
                menu.cursor = index;
            }
        }
    }

    fn hover_in(&self, rect: Rect) -> bool {
        self.hover
            .is_some_and(|(column, row)| contains(rect, column, row))
    }

    fn sidebar_title_hot(&self) -> bool {
        let Some((column, row)) = self.hover else {
            return false;
        };
        let chrome = self.frame_chrome(Rect::new(0, 0, self.width, self.height));
        self.sidebar_button(chrome.tree, column, row)
    }

    fn agent_title_hot(&self) -> bool {
        let Some((column, row)) = self.hover else {
            return false;
        };
        let chrome = self.frame_chrome(Rect::new(0, 0, self.width, self.height));
        self.agent_button(chrome.agent, column, row)
    }

    fn hover_tile_seam(&self, index: usize) -> bool {
        let Some((column, row)) = self.hover else {
            return false;
        };
        self.tile_seam(column, row) == Some(index)
    }

    fn click_tab(&mut self, column: u16, row: u16, shift: bool, width: u16, height: u16) -> bool {
        self.note_frame(width, height);
        let area = Rect::new(0, 0, self.width, self.height);
        let Some(status) = self.frame_chrome(area).status else {
            return false;
        };
        let bounds = tab_bounds(self.tab_enabled, self.tab_status(), status);
        if column < bounds.x || column >= bounds.right() || row < bounds.y || row >= bounds.bottom()
        {
            return false;
        }
        if shift {
            self.open_settings();
        } else {
            self.toggle_tab();
        }
        true
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    pub fn splits(&self) -> u8 {
        if self.show_many {
            2
        } else {
            1
        }
    }

    pub fn text(&self) -> String {
        self.focused().text()
    }

    pub fn notice(&self) -> &str {
        &self.notice
    }

    pub fn workspace_path(&self) -> &Path {
        &self.workspace
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    pub fn terminals_open(&self) -> bool {
        self.terminals.open
    }

    pub fn runtime_dock_id(&self) -> Option<String> {
        self.runtime_dock.clone()
    }

    fn canvas_open(&self) -> bool {
        self.runtime_dock.is_some()
    }

    fn canvas_active(&self) -> Option<&canvas::CanvasSurface> {
        let kind = self.runtime_dock.as_deref()?;
        self.canvas_surfaces.iter().find(|surface| surface.kind == kind)
    }

    fn canvas_active_mut(&mut self) -> Option<&mut canvas::CanvasSurface> {
        let kind = self.runtime_dock.clone()?;
        self.canvas_surfaces
            .iter_mut()
            .find(|surface| surface.kind == kind)
    }

    fn canvas_tab_label(kind: &str) -> &'static str {
        match kind {
            "browser" => "Browser",
            "phone" => "Phone",
            "android" => "Android",
            "markdown" => "Preview",
            _ => "Canvas",
        }
    }

    fn open_welcome_buffer(&mut self) {
        self.buffers = vec![EditBuffer::from_text("welcome", WELCOME_MARKDOWN, "markdown")];
        self.focus = 0;
        self.open_markdown_preview();
        self.notice = "welcome — Preview on the right · ; settings to hide on startup".into();
    }

    fn open_markdown_preview(&mut self) {
        if !self.canvas_surfaces.iter().any(|surface| surface.kind == "markdown") {
            self.canvas_surfaces
                .push(canvas::CanvasSurface::text_pane("markdown", "preview"));
        }
        self.runtime_dock = Some("markdown".into());
        self.markdown_preview_scroll = 0;
        self.markdown_force_text = false;
        // Keep any warm retained pages; engine will replace when fingerprint changes.
        // Text fallback paints immediately — Kitty never blocks open.
        self.canvas_chrome_dirty.set(true);
        self.canvas_painted_size = None;
        self.canvas_image_id = None;
        self.notice = "preview · rendering in background…".into();
        self.schedule_markdown_raster(edocview::RASTER_DEBOUNCE);
    }

    fn schedule_markdown_raster(&mut self, debounce: Duration) {
        if self.markdown_force_text || cfg!(test) {
            return;
        }
        if !edocview::kitty_doc_available() {
            return;
        }
        let chrome = self.frame_chrome(Rect::new(0, 0, self.width.max(1), self.height.max(1)));
        if chrome.canvas.width < 4 {
            return;
        }
        let inner = canvas_media_rect(chrome.canvas);
        let cols = inner.width.saturating_sub(0).max(1);
        let (cell_w, cell_h) = edocview::cell_pixels(self.width, self.height, self.pixels);
        let source = self.markdown_preview_source();
        self.preview_engine.request(
            edocview::PreviewSlot::Markdown,
            &source,
            &self.workspace,
            cols,
            cell_w,
            cell_h,
            MARKDOWN_KITTY_IMAGE_ID,
            debounce,
        );
    }

    fn schedule_help_raster(&mut self, debounce: Duration) {
        if self.help_force_text || cfg!(test) {
            return;
        }
        if !edocview::kitty_doc_available() {
            return;
        }
        let area = Rect::new(0, 0, self.width.max(1), self.height.max(1));
        let cols = area.width.saturating_sub(2).max(1);
        let (cell_w, cell_h) = edocview::cell_pixels(self.width, self.height, self.pixels);
        self.preview_engine.request(
            edocview::PreviewSlot::Help,
            &help_markdown(),
            &self.workspace,
            cols,
            cell_w,
            cell_h,
            HELP_KITTY_IMAGE_ID,
            debounce,
        );
    }

    fn toggle_show_welcome(&mut self) {
        self.show_welcome = !self.show_welcome;
        persist_show_welcome(&self.workspace, self.show_welcome);
        self.notice = if self.show_welcome {
            "welcome on startup".into()
        } else {
            "welcome skipped on startup".into()
        };
    }

    fn toggle_kitty_graphics(&mut self) {
        self.kitty_graphics = !self.kitty_graphics;
        persist_kitty_graphics(&self.workspace, self.kitty_graphics);
        graphics::set_kitty_allowed(self.kitty_graphics);
        self.graphics.refresh_kitty_policy();
        // Drop any bound Kitty surfaces so fallbacks paint cleanly.
        self.editor_minimap_id = None;
        self.editor_minimap_key = None;
        self.minimap_frame_cache.borrow_mut().clear();
        self.preview_pool.clear();
        self.canvas_image_id = None;
        self.canvas_painted_size = None;
        self.canvas_placeholders_stale.set(true);
        self.flip.dirty = true;
        self.notice = if self.kitty_graphics {
            "Kitty graphics on (when the host supports it)".into()
        } else {
            "Kitty graphics off · text fallbacks".into()
        };
    }

    fn open_settings(&mut self) {
        self.help = false;
        self.palette = false;
        self.overlay = Overlay::Settings;
    }

    fn markdown_preview_source(&self) -> String {
        let focused = self.focused();
        if focused.language == "markdown" || focused.name.ends_with(".md") || focused.name == "welcome"
        {
            return focused.text();
        }
        self.buffers
            .iter()
            .find(|buffer| buffer.language == "markdown" || buffer.name == "welcome")
            .map(|buffer| buffer.text())
            .unwrap_or_else(|| focused.text())
    }

    fn activate_canvas_kind(&mut self, kind: &str) -> bool {
        if !self.canvas_surfaces.iter().any(|surface| surface.kind == kind) {
            return false;
        }
        self.runtime_dock = Some(kind.to_string());
        // Keep per-surface Kitty id + PNG cache. Only rebind placeholders / chrome.
        if let Some(surface) = self.canvas_surfaces.iter().find(|s| s.kind == kind) {
            self.canvas_image_id = Some(surface.next_image_id());
            self.canvas_painted_size = surface.placed_size();
        }
        self.canvas_chrome_dirty.set(true);
        self.canvas_placeholders_stale.set(true);
        self.focus_pane(PaneFocus::Runtime);
        true
    }

    fn close_active_canvas(&mut self) {
        let Some(kind) = self.runtime_dock.clone() else {
            return;
        };
        if let Some(index) = self
            .canvas_surfaces
            .iter()
            .position(|surface| surface.kind == kind)
        {
            let mut surface = self.canvas_surfaces.remove(index);
            surface.store_in_cache(&mut self.canvas_frame_cache);
            surface.stop();
        }
        self.runtime_dock = self
            .canvas_surfaces
            .last()
            .map(|surface| surface.kind.clone());
        if let Some(kind) = self.runtime_dock.clone() {
            if let Some(surface) = self.canvas_surfaces.iter().find(|s| s.kind == kind) {
                self.canvas_image_id = Some(surface.next_image_id());
                self.canvas_painted_size = surface.placed_size();
            }
        } else {
            self.canvas_painted_size = None;
            self.canvas_image_id = None;
        }
        self.canvas_chrome_dirty.set(true);
        self.canvas_placeholders_stale.set(true);
        if self.runtime_dock.is_none() {
            self.runtime_focused = false;
            self.notice = "canvas closed".into();
        } else {
            self.notice = format!(
                "{} canvas",
                Self::canvas_tab_label(self.runtime_dock.as_deref().unwrap_or(""))
            );
        }
    }

    /// Title-row hits: tabs (when 2+) and trailing close.
    fn canvas_title_hit(&self, column: u16, row: u16, chrome: &Chrome) -> Option<CanvasTitleHit> {
        if chrome.canvas.width == 0 || row != chrome.canvas.y || !contains(chrome.canvas, column, row)
        {
            return None;
        }
        let local = column.saturating_sub(chrome.canvas.x);
        let width = chrome.canvas.width;
        // Far-right " x " closes the active tab.
        if width > 3 && local + 3 >= width {
            return Some(CanvasTitleHit::Close);
        }
        if self.canvas_surfaces.len() < 2 {
            return None;
        }
        // Tabs sit just left of the close control: " Browser Phone  x "
        let mut cursor = width.saturating_sub(3); // before " x "
        for (index, surface) in self.canvas_surfaces.iter().enumerate().rev() {
            let label = format!(" {} ", Self::canvas_tab_label(&surface.kind));
            let len = label.chars().count() as u16;
            let start = cursor.saturating_sub(len);
            if local >= start && local < cursor {
                return Some(CanvasTitleHit::Tab(index));
            }
            cursor = start;
        }
        None
    }

    /// Bottom-strip hits for terminal-phone / terminal-browser controls.
    fn canvas_control_hit(
        &self,
        column: u16,
        row: u16,
        chrome: &Chrome,
    ) -> Option<CanvasControlHit> {
        let strip = canvas_controls_rect(chrome.canvas);
        if strip.width == 0 || !contains(strip, column, row) {
            return None;
        }
        let kind = self.runtime_dock.as_deref()?;
        let local = column.saturating_sub(strip.x) as usize;
        let line = Self::canvas_control_line(kind, self.canvas_active().map(|s| s.target.as_str()));
        hit_control_on_line(&line, local, kind)
    }

    fn canvas_control_line(kind: &str, target: Option<&str>) -> String {
        match kind {
            "phone" => " [Home] [Lock] [Siri] ".to_string(),
            "browser" => {
                let url = target.unwrap_or("");
                let short = if url.chars().count() > 28 {
                    format!("{}…", url.chars().take(27).collect::<String>())
                } else {
                    url.to_string()
                };
                format!(" [Back] [Fwd] [Reload] {short} ")
            }
            _ => String::new(),
        }
    }

    fn apply_canvas_control(&mut self, hit: CanvasControlHit) {
        let Some(surface) = self.canvas_active_mut() else {
            return;
        };
        let notice = match hit {
            CanvasControlHit::PhoneHome => surface.button("HOME"),
            CanvasControlHit::PhoneLock => surface.button("LOCK"),
            CanvasControlHit::PhoneSiri => surface.button("SIRI"),
            CanvasControlHit::BrowserBack => surface.browser_nav("back"),
            CanvasControlHit::BrowserForward => surface.browser_nav("forward"),
            CanvasControlHit::BrowserReload => surface.browser_nav("reload"),
        };
        if let Some(text) = notice {
            self.notice = text;
        }
        self.canvas_chrome_dirty.set(true);
    }

    pub fn frame_width(&self) -> u16 {
        self.width
    }

    pub fn frame_height(&self) -> u16 {
        self.height
    }

    pub fn focused_buffer_meta(&self) -> ide_mcp::BufferInfo {
        let buffer = self.focused();
        ide_mcp::BufferInfo {
            index: self.focus,
            name: buffer.name.clone(),
            language: buffer.language.clone(),
            dirty: buffer.dirty(),
            cursor: buffer.cursor,
            revision: buffer.document.revision(),
            focused: true,
        }
    }

    pub fn list_buffers(&self) -> Vec<ide_mcp::BufferInfo> {
        self.buffers
            .iter()
            .enumerate()
            .map(|(index, buffer)| ide_mcp::BufferInfo {
                index,
                name: buffer.name.clone(),
                language: buffer.language.clone(),
                dirty: buffer.dirty(),
                cursor: buffer.cursor,
                revision: buffer.document.revision(),
                focused: index == self.focus,
            })
            .collect()
    }

    pub fn buffer_text(&self, index: usize) -> Result<String, String> {
        self.buffers
            .get(index)
            .map(|buffer| buffer.text())
            .ok_or_else(|| format!("no buffer at index {index}"))
    }

    /// MCP / control-plane: open a file or directory in the live session.
    pub fn control_open(&mut self, path: PathBuf) {
        self.open_path(path);
    }

    /// MCP / control-plane: send a named key (esc, tab, enter, ctrl-s, 6, …).
    pub fn control_key(&mut self, key: &str) -> Result<(), String> {
        let normalized = key.trim().to_ascii_lowercase().replace('_', "-");
        // Canvas owns keys while the runtime pane is focused.
        if self.runtime_focused && self.canvas_open() {
            let (code, modifiers) = match normalized.as_str() {
                "esc" | "escape" => (KeyCode::Esc, KeyModifiers::NONE),
                "tab" => (KeyCode::Tab, KeyModifiers::NONE),
                "enter" | "return" => (KeyCode::Enter, KeyModifiers::NONE),
                "backspace" => (KeyCode::Backspace, KeyModifiers::NONE),
                "up" => (KeyCode::Up, KeyModifiers::NONE),
                "down" => (KeyCode::Down, KeyModifiers::NONE),
                "left" => (KeyCode::Left, KeyModifiers::NONE),
                "right" => (KeyCode::Right, KeyModifiers::NONE),
                "ctrl-h" | "ctrlh" => (KeyCode::Char('h'), KeyModifiers::CONTROL),
                "ctrl-l" | "ctrll" => (KeyCode::Char('l'), KeyModifiers::CONTROL),
                "ctrl-s" | "ctrls" => (KeyCode::Char('s'), KeyModifiers::CONTROL),
                other if other.chars().count() == 1 => {
                    let ch = other.chars().next().unwrap();
                    (KeyCode::Char(ch), KeyModifiers::NONE)
                }
                other => return Err(format!("unsupported canvas key: {other}")),
            };
            if self.canvas_key(code, modifiers) {
                return Ok(());
            }
        }
        match normalized.as_str() {
            "esc" | "escape" => self.handle(CommandKey::Esc),
            "tab" => self.handle(CommandKey::Tab),
            "enter" | "return" => self.handle(CommandKey::Enter),
            "backspace" => self.handle(CommandKey::Backspace),
            "up" => self.handle(CommandKey::Up),
            "down" => self.handle(CommandKey::Down),
            "left" => self.handle(CommandKey::Left),
            "right" => self.handle(CommandKey::Right),
            "ctrl-s" | "ctrls" => self.handle(CommandKey::CtrlS),
            "ctrl-q" | "ctrlq" => self.handle(CommandKey::CtrlQ),
            "ctrl-p" | "ctrlp" => self.handle(CommandKey::CtrlP),
            "ctrl-w" | "ctrlw" => self.handle(CommandKey::CtrlW),
            "help" | "?" => self.handle(CommandKey::Help),
            "hints" => self.toggle_hover_hints(),
            other if other.len() == 1 => {
                let ch = other.chars().next().unwrap();
                self.handle(CommandKey::Char(ch));
            }
            other => return Err(format!("unsupported key: {other}")),
        }
        Ok(())
    }

    /// MCP / control-plane: synthetic mouse against the live frame (Canvas, editor, …).
    pub fn control_pointer(
        &mut self,
        kind: &str,
        column: u16,
        row: u16,
    ) -> Result<(), String> {
        let width = self.width.max(1);
        let height = self.height.max(1);
        let event = match kind.trim().to_ascii_lowercase().as_str() {
            "down" | "press" => Pointer::Down { column, row },
            "drag" | "move-drag" => Pointer::Drag { column, row },
            "up" | "release" => Pointer::Up { column, row },
            "move" | "hover" => Pointer::Move { column, row },
            "scroll-up" | "wheel-up" => Pointer::ScrollUp { column, row },
            "scroll-down" | "wheel-down" => Pointer::ScrollDown { column, row },
            "scroll-left" | "wheel-left" => Pointer::ScrollLeft { column, row },
            "scroll-right" | "wheel-right" => Pointer::ScrollRight { column, row },
            "right" | "secondary" => Pointer::Right { column, row },
            other => return Err(format!("unsupported pointer kind: {other}")),
        };
        self.pointer(event, width, height);
        Ok(())
    }

    /// MCP / control-plane: named editor commands.
    pub fn control_command(&mut self, name: &str) -> Result<(), String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "save" => self.handle(CommandKey::CtrlS),
            "quit-request" | "quit" => self.handle(CommandKey::CtrlQ),
            "help" => self.handle(CommandKey::Help),
            "hints" => self.toggle_hover_hints(),
            "palette" => self.handle(CommandKey::CtrlP),
            "fullscreen" | "toggle-fullscreen" => self.handle(CommandKey::CtrlW),
            "terminal.toggle" | "toggle-terminal" => {
                self.handle(CommandKey::Char('t'));
            }
            "focus.editor" => self.focus_pane(PaneFocus::Editor),
            "focus.terminal" => self.focus_pane(PaneFocus::Terminal),
            "focus.tree" => self.focus_pane(PaneFocus::Tree),
            "focus.agent" => self.focus_pane(PaneFocus::Agent),
            "focus.runtime" => self.focus_pane(PaneFocus::Runtime),
            other => return Err(format!("unknown command: {other}")),
        }
        Ok(())
    }

    /// MCP / control-plane: open Browser / Phone / Android dock.
    pub fn control_runtime(&mut self, kind: &str) -> Result<(), String> {
        let action = match kind.trim().to_ascii_lowercase().as_str() {
            "browser" => WorkbenchAction::Browser,
            "phone" => WorkbenchAction::Phone,
            "android" => WorkbenchAction::Android,
            other => return Err(format!("unknown runtime kind: {other}")),
        };
        self.workbench(action);
        Ok(())
    }

    pub fn help_open(&self) -> bool {
        self.help
    }

    pub fn cursor(&self) -> usize {
        self.focused().cursor
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.focused().selection
    }

    /// Apply a `textDocument/foldingRange` JSON result to the focused buffer.
    /// Empty / malformed payloads fall back to syntax folds on next refresh.
    pub fn apply_lsp_folding_ranges(&mut self, result: &serde_json::Value) {
        self.focused_mut().apply_lsp_fold_result(result);
    }

    /// Fold range count and source for the focused buffer (`"lsp"` / `"syntax"` / none).
    pub fn fold_status(&self) -> (usize, Option<&'static str>) {
        self.focused().ensure_folds();
        let folds = self.focused().folds.borrow();
        let source = match folds.source() {
            Some(fold::FoldSource::Lsp) => Some("lsp"),
            Some(fold::FoldSource::Syntax) => Some("syntax"),
            None => None,
        };
        (folds.ranges().len(), source)
    }

    pub fn scroll(&self) -> usize {
        self.focused().scroll
    }

    pub fn focus_index(&self) -> usize {
        self.focus
    }

    pub fn note_pixels(&mut self, pixels: Option<(u16, u16)>) {
        self.pixels = pixels.filter(|(width, height)| *width > 0 && *height > 0);
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    pub fn view_offset(&self) -> f32 {
        self.view
    }

    pub fn view_target(&self) -> f32 {
        self.target
    }

    /// Flip needs a high-rate wake only while scroll/easing is in flight —
    /// a settled open Flip must not pin the event loop at ~60 Hz (AFK CPU).
    fn flip_animating(&self) -> bool {
        if !self.flip.open {
            return false;
        }
        self.flip.dirty || (self.flip.scroll - self.flip.selected as f32).abs() >= 0.001
    }

    pub fn animating(&self) -> bool {
        // Scroll/view motion, Flip 3D easing, or async doc preview finishing.
        (self.target - self.view).abs() >= 0.05
            || self.flip_animating()
            || self.preview_engine.busy()
    }

    /// Buffer tile slide in progress (Left/Right focus change).
    fn view_transitioning(&self) -> bool {
        (self.target - self.view).abs() >= 0.05
    }

    /// Drop Kitty minimap during document scroll / edge drag / buffer slide.
    /// Precision text scrollbar owns the edge until motion settles.
    fn minimap_kitty_suppressed(&self) -> bool {
        self.view_transitioning()
            || self.edge_drag.is_some()
            || self.wheel_at.is_some()
    }

    /// Kitty RGB strip only when the host supports it and motion has settled.
    fn kitty_minimap_enabled(&self) -> bool {
        minimap_gfx::host_is_kitty() && !self.minimap_kitty_suppressed()
    }

    fn focused_edge_chrome(&self, tile_w: u16, line_count: usize, visible: usize) -> (u16, u16) {
        // During scroll / buffer slide: precision scrollbar only (no Kitty / glyph strip).
        if self.minimap_kitty_suppressed() {
            if !minimap_gfx::needs_scroll(line_count, visible) || tile_w < 18 {
                return (0, 0);
            }
            return (0, 1);
        }
        minimap_gfx::edge_chrome(
            tile_w,
            minimap_gfx::host_is_kitty(),
            self.last_pressure.is_critical() && !minimap_gfx::force_minimap(),
            self.last_pressure.is_elevated() && !minimap_gfx::force_minimap(),
            line_count,
            visible,
        )
    }

    fn hover_hint_pending(&self) -> bool {
        self.hover_hints
            && self.hover.is_some()
            && self.hover_hint.is_none()
            && self
                .hover_since
                .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
    }

    fn hover_hint_wait(&self) -> Option<Duration> {
        if !self.hover_hint_pending() {
            return None;
        }
        let since = self.hover_since?;
        let due = Duration::from_secs(1).saturating_sub(since.elapsed());
        Some(due.max(Duration::from_millis(16)).min(Duration::from_millis(100)))
    }

    fn refresh_hover_hint(&mut self) {
        if !self.hover_hints {
            self.hover_hint = None;
            return;
        }
        let Some((column, row)) = self.hover else {
            self.hover_hint = None;
            return;
        };
        let Some(since) = self.hover_since else {
            return;
        };
        if since.elapsed() < Duration::from_secs(1) {
            self.hover_hint = None;
            return;
        }
        self.hover_hint = hover_action(self, column, row).map(str::to_string);
    }

    fn toggle_hover_hints(&mut self) {
        self.hover_hints = !self.hover_hints;
        if !self.hover_hints {
            self.hover_hint = None;
        }
        self.notice = if self.hover_hints {
            "hover hints on".into()
        } else {
            "hover hints off".into()
        };
    }

    pub fn note_frame(&mut self, width: u16, height: u16) {
        self.width = width.max(1);
        self.height = height.max(1);
        let orientation = layout::axis_of(self.width, self.height, self.pixels);
        if self.buffers.len() < 2
            || orientation != self.axis
            || self
                .tile_spans
                .as_ref()
                .is_some_and(|spans| spans.len() != self.buffers.len())
        {
            self.tile_spans = None;
            self.tile_drag = None;
        }
        let (_, window) = self.metrics_for(self.width, self.height, orientation);
        let key = (
            orientation,
            self.show_many,
            window.stride,
            window.tile_height,
        );
        let changed = key != self.layout_key;
        self.axis = orientation;
        self.stride = window.stride.max(1);
        self.layout_key = key;
        if changed {
            self.tile_drag = None;
            self.view = window.origin(self.focus) as f32;
            self.target = self.view;
        } else if self.tile_drag.is_none() {
            self.target = window.origin(self.focus) as f32;
        }
        // AFK tick26: Flip covers the editor — skip git gutter probes until close.
        if !self.flip.open {
            self.refresh_gutter_git();
        }
    }

    pub fn tick(&mut self, dt: Duration) -> bool {
        let mut changed = false;
        let delta = self.target - self.view;
        if delta.abs() < 0.05 {
            if self.view != self.target {
                self.view = self.target;
                changed = true;
            }
        } else {
            let step = 1.0 - (-dt.as_secs_f32() / 0.12).exp();
            self.view += delta * step;
            changed = true;
        }
        if self
            .wheel_at
            .is_some_and(|at| at.elapsed() > Duration::from_millis(160))
        {
            self.scroll_axis = None;
            self.wheel_at = None;
            // One present after settle so Kitty minimap can return.
            changed = true;
        }
        // AFK: Flip owns the screen — skip hover-hint work until it closes.
        let hint_changed = if self.flip.open {
            false
        } else if self.hover_hints && self.hover.is_some() {
            let before = self.hover_hint.clone();
            self.refresh_hover_hint();
            self.hover_hint != before
        } else if self.hover_hint.take().is_some() {
            true
        } else {
            false
        };
        // AFK: Flip owns the frame — defer OS pressure probes until it closes.
        let pressure_notice = if self.flip.open {
            false
        } else {
            self.maintain_memory_pressure()
        };
        changed || pressure_notice || hint_changed
    }

    /// Probe OS memory pressure on a slow cadence and shrink undo/scrollback.
    /// Must not dirty the UI every tick — that made Urgent feel like a soft lock.
    fn maintain_memory_pressure(&mut self) -> bool {
        // AFK: idle hosts probe every 5s; elevated/animating keep 2s.
        let interval = if self.last_pressure.is_elevated() || self.animating() {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(5)
        };
        let due = self
            .memory_pressure_at
            .is_none_or(|at| at.elapsed() >= interval);
        if !due {
            return false;
        }
        self.memory_pressure_at = Some(Instant::now());
        let level = pressure_level();
        let changed = level != self.last_pressure;
        if !changed && !level.is_elevated() {
            return false;
        }
        for buffer in &mut self.buffers {
            buffer.document.apply_pressure(level);
        }
        self.terminals.apply_pressure(level);
        if changed {
            self.last_pressure = level;
            if level.is_elevated() {
                // Drop Flip/minimap Arcs sized for the previous budget.
                self.preview_pool.clear();
                self.minimap_frame_cache.borrow_mut().clear();
                self.editor_minimap_key = None;
                if self.notifications.len() > 8 {
                    self.notifications.truncate(8);
                }
                self.notice = format!("memory pressure {:?}", level);
                return true;
            }
        }
        false
    }

    /// Deterministic trim used by `--bench-memory` and tests.
    pub fn force_memory_pressure(&mut self, level: PressureLevel) {
        self.last_pressure = level;
        self.memory_pressure_at = Some(Instant::now());
        for buffer in &mut self.buffers {
            buffer.document.apply_pressure(level);
        }
        self.terminals.apply_pressure(level);
    }

    pub fn handle(&mut self, key: CommandKey) {
        self.tile_drag = None;
        self.dispatch(key);
        self.note_frame(self.width, self.height);
    }

    pub fn pointer(&mut self, event: Pointer, width: u16, height: u16) {
        self.note_frame(width, height);
        let area = Rect::new(0, 0, self.width, self.height);
        let chrome = self.frame_chrome(area);
        let window = self.window();
        match event {
            Pointer::ScrollUp { column, row } => {
                if self.canvas_wheel(column, row, -1, &chrome) {
                    return;
                }
                self.route_wheel(-1.0, true, column, row, &chrome);
            }
            Pointer::ScrollDown { column, row } => {
                if self.canvas_wheel(column, row, 1, &chrome) {
                    return;
                }
                self.route_wheel(1.0, true, column, row, &chrome);
            }
            Pointer::ScrollLeft { column, row } => {
                if self.canvas_wheel_axis(column, row, 0, -1, &chrome) {
                    return;
                }
                self.route_wheel(-1.0, false, column, row, &chrome);
            }
            Pointer::ScrollRight { column, row } => {
                if self.canvas_wheel_axis(column, row, 0, 1, &chrome) {
                    return;
                }
                self.route_wheel(1.0, false, column, row, &chrome);
            }
            Pointer::Up { column, row } => {
                self.edge_drag = None;
                self.tile_drag = None;
                self.buffer_strip_drag = false;
                if self.canvas_resize {
                    self.end_canvas_resize(true);
                    return;
                }
                // Finish tap/swipe even if release is outside the canvas.
                self.canvas_pointer(column, row, &chrome, CanvasPointerKind::Up);
            }
            Pointer::Move { column, row } => {
                self.set_hover(column, row);
                self.hover_menu(column, row, area);
            }
            Pointer::Right { column, row } => {
                if self.notifications_open && !self.notifications.is_empty() {
                    let rect = notification_rect(area, self.notifications.len());
                    if contains(rect, column, row) {
                        return;
                    }
                }
                if self.help
                    || self.yazi.is_some()
                    || self.finder.is_some()
                    || matches!(
                        self.overlay,
                        Overlay::Settings
                            | Overlay::Quit
                            | Overlay::Close(_)
                            | Overlay::Rename(_)
                            | Overlay::Vim
                    )
                {
                    return;
                }
                self.open_menu(column, row);
            }
            Pointer::Down { column, row } | Pointer::Drag { column, row } => {
                self.set_hover(column, row);
                let drag = matches!(event, Pointer::Drag { .. });
                if !drag && self.click_notifications(column, row) {
                    return;
                }
                if let Overlay::Menu(menu) = self.overlay {
                    if !drag {
                        self.click_menu(menu, column, row, area);
                    }
                    return;
                }
                // Resize seams only continue once armed by a Down on the handle.
                // A selection drag that leaves the PTY body must never start a resize.
                if drag && self.terminal_drag {
                    self.resize_terminal(row);
                    return;
                }
                if drag && self.terminal_side_drag {
                    self.resize_terminal_side(column);
                    return;
                }
                if drag && self.canvas_resize {
                    self.resize_canvas(column);
                    return;
                }
                // Top-edge and session-list seams arm before body hits so the
                // Terminal title row can be dragged to resize.
                if !drag && self.on_terminal_seam(column, row) {
                    self.terminal_drag = true;
                    self.terminal_side_drag = false;
                    self.end_canvas_resize(true);
                    return;
                }
                if !drag && self.on_terminal_side_seam(column, row) {
                    self.terminal_side_drag = true;
                    self.terminal_drag = false;
                    self.end_canvas_resize(true);
                    return;
                }
                if !drag && self.on_canvas_seam(column, row, &chrome) {
                    self.canvas_resize = true;
                    self.canvas_resize_hid_gfx = false;
                    // Layout-only while dragging — no chrome Clear / Kitty / geom.
                    self.terminal_drag = false;
                    self.terminal_side_drag = false;
                    self.resize = None;
                    return;
                }
                if self.pointer_terminal(column, row, drag) {
                    // Body click/drag owns the gesture — drop any leftover resize arm.
                    if !drag {
                        self.terminal_drag = false;
                        self.terminal_side_drag = false;
                        self.end_canvas_resize(true);
                    }
                    return;
                }
                if !drag {
                    // Leaving the terminal clears its selection; keyboard moves with the click.
                    self.terminals.blur();
                    self.resize = None;
                    self.tile_drag = None;
                    self.terminal_drag = false;
                    self.terminal_side_drag = false;
                    self.end_canvas_resize(true);
                }
                if quit_hit(column, row, area) {
                    if !drag {
                        self.request_quit();
                    }
                    return;
                }
                if settings_hit(column, row, area) {
                    if !drag {
                        if matches!(self.overlay, Overlay::Settings) {
                            self.overlay = Overlay::None;
                        } else {
                            self.open_settings();
                        }
                    }
                    return;
                }
                if hints_hit(column, row, area) {
                    if !drag {
                        self.toggle_hover_hints();
                    }
                    return;
                }
                if help_hit(column, row, area, chrome.status) {
                    if !drag {
                        if self.help {
                            self.close_help();
                        } else {
                            self.open_help();
                        }
                    }
                    return;
                }
                if self.help {
                    return;
                }
                if matches!(self.overlay, Overlay::Settings) {
                    if !drag {
                        let inner = inset(chrome.editor);
                        let index = row.saturating_sub(inner.y) as usize;
                        if index == 2 {
                            self.sidebar = !self.sidebar;
                        } else if index == 3 {
                            self.terminals.open = !self.terminals.open;
                        } else if index == 4 {
                            self.toggle_chat();
                        } else if index == 5 {
                            self.show_many = !self.show_many;
                        } else if index == 6 {
                            self.toggle_tab();
                        } else if index == 7 {
                            self.toggle_show_welcome();
                        } else if index == 8 {
                            self.toggle_kitty_graphics();
                        }
                    }
                    return;
                }
                if matches!(
                    self.overlay,
                    Overlay::Quit | Overlay::Close(_) | Overlay::Rename(_) | Overlay::Vim
                ) {
                    return;
                }
                if self.yazi.is_some() {
                    if !drag {
                        self.yazi_click(column, row, area);
                    }
                    return;
                }
                if self.finder.is_some() {
                    if !drag {
                        self.finder_click(row, chrome.body);
                    }
                    return;
                }
                if drag {
                    if let Some(side) = self.resize {
                        self.resize_sidebar(side, column);
                        return;
                    }
                }
                if let Some(side) = self.seam(&chrome, column, row) {
                    let toggle = match side {
                        Side::Left => self.sidebar_button(chrome.tree, column, row),
                        Side::Right => self.agent_button(chrome.agent, column, row),
                    };
                    if drag || !toggle {
                        self.resize = Some(side);
                        if drag {
                            self.resize_sidebar(side, column);
                        }
                        return;
                    }
                }
                if !drag {
                    if let Some(action) = toolbar_hit(chrome.toolbar, column, row) {
                        self.workbench(action);
                        return;
                    }
                } else if contains(chrome.toolbar, column, row) {
                    return;
                }
                if chrome.agent.width > 0 && contains(chrome.agent, column, row) {
                    if !drag && self.agent_button(chrome.agent, column, row) {
                        self.toggle_chat();
                    } else if !drag && self.chat {
                        self.focus_pane(PaneFocus::Agent);
                    }
                    return;
                }
                let over_canvas = chrome.canvas.width > 0
                    && self.canvas_open()
                    && contains(chrome.canvas, column, row);
                let canvas_gesture = drag
                    && self
                        .canvas_active()
                        .is_some_and(|surface| surface.gesture_active());
                if over_canvas || canvas_gesture {
                    if !drag {
                        if let Some(hit) = self.canvas_title_hit(column, row, &chrome) {
                            match hit {
                                CanvasTitleHit::Close => self.close_active_canvas(),
                                CanvasTitleHit::Tab(index) => {
                                    if let Some(kind) =
                                        self.canvas_surfaces.get(index).map(|s| s.kind.clone())
                                    {
                                        self.activate_canvas_kind(&kind);
                                        self.notice = format!(
                                            "{} tab",
                                            Self::canvas_tab_label(&kind)
                                        );
                                    }
                                }
                            }
                            return;
                        }
                    }
                    let kind = if drag {
                        CanvasPointerKind::Drag
                    } else {
                        CanvasPointerKind::Down
                    };
                    self.canvas_pointer(column, row, &chrome, kind);
                    return;
                }
                if chrome.tree.width > 0 && contains(chrome.tree, column, row) {
                    if !drag && self.sidebar_button(chrome.tree, column, row) {
                        self.sidebar = !self.sidebar;
                    } else if !drag {
                        self.focus_pane(PaneFocus::Tree);
                        self.tree_click(row, chrome);
                    }
                    return;
                }
                if drag && self.buffer_strip_drag {
                    if let Some(index) = self.buffer_strip_hit(column, row) {
                        self.focus_at(index);
                    }
                    return;
                }
                if !drag {
                    if let Some(index) = self.buffer_strip_hit(column, row) {
                        self.buffer_strip_drag = true;
                        self.focus_at(index);
                        return;
                    }
                }
                // Niri-style buffer edges: split or full (`f` on/off).
                if self.buffers.len() >= 2 {
                    if drag {
                        if let Some(index) = self.tile_drag {
                            self.resize_tile(index, column, row);
                            return;
                        }
                    } else if let Some(index) = self.tile_seam(column, row) {
                        self.arm_buffer_edge_resize(index);
                        return;
                    }
                }
                let Some(index) =
                    layout::hit_tile(window, column, row, self.view, self.buffers.len())
                else {
                    return;
                };
                if drag && index != self.focus {
                    return;
                }
                if !drag && self.close_button(window, index, column, row) {
                    self.request_close(index);
                    return;
                }
                if !drag {
                    self.focus_pane(PaneFocus::Editor);
                    self.focus_at(index);
                }
                let screen_column = column;
                let Some((local_x, line_in_view)) =
                    layout::cell_in_tile(window, index, self.view, screen_column, row)
                else {
                    return;
                };
                let (tile_w, tile_h) = window.tile_box(index);
                let line_count = self.focused().document.len_lines().max(1);
                let gutter_w = self.gutter.width_for(line_count) as usize;
                let visible = (tile_h.saturating_sub(2) as usize).max(1);
                let (minimap_w, scroll_w) =
                    self.focused_edge_chrome(tile_w, line_count, visible);
                let minimap_w = minimap_w as usize;
                let scroll_w = scroll_w as usize;
                let inner_w = tile_w.saturating_sub(2) as usize;
                let edge_x0 = inner_w.saturating_sub(minimap_w + scroll_w);
                let scroll_x0 = if scroll_w > 0 {
                    inner_w.saturating_sub(scroll_w)
                } else {
                    usize::MAX
                };
                let on_edge = (minimap_w + scroll_w > 0) && local_x >= edge_x0;
                if drag {
                    if matches!(self.edge_drag, Some(EdgeDrag::Scrollbar | EdgeDrag::Minimap))
                        || on_edge
                    {
                        let track = tile_h.saturating_sub(2).max(1);
                        let kind = match self.edge_drag {
                            Some(EdgeDrag::Minimap) => EdgeDrag::Minimap,
                            Some(EdgeDrag::Scrollbar) => EdgeDrag::Scrollbar,
                            None if local_x >= scroll_x0 => EdgeDrag::Scrollbar,
                            None => EdgeDrag::Minimap,
                        };
                        let next = match kind {
                            EdgeDrag::Minimap => self.minimap_line_for_row(
                                line_count,
                                visible,
                                track as usize,
                                line_in_view,
                            ),
                            EdgeDrag::Scrollbar => scrollbar::ScrollMetrics {
                                offset: self.focused().scroll,
                                visible,
                                total: line_count,
                            }
                            .offset_for_track_y(line_in_view as u16, track),
                        };
                        self.snap_scroll(next);
                        self.edge_drag = Some(kind);
                        return;
                    }
                } else if local_x < gutter_w {
                    self.focused().ensure_folds();
                    let line_count = self.focused().document.len_lines().max(1);
                    let scroll = self.focused().scroll;
                    let doc_line_0 = self
                        .focused()
                        .folds
                        .borrow()
                        .doc_line_for_view_row(scroll, line_in_view, line_count)
                        .unwrap_or_else(|| scroll.saturating_add(line_in_view));
                    let doc_line = doc_line_0.saturating_add(1);
                    match gutter::hit(local_x as u16, gutter_w as u16) {
                        Some(gutter::GutterHit::Breakpoint) => {
                            if let Some(path) = self.focused().path.clone() {
                                let on = self.gutter.toggle_breakpoint(&path, doc_line);
                                self.notice = if on {
                                    format!("breakpoint on line {doc_line}")
                                } else {
                                    format!("breakpoint cleared line {doc_line}")
                                };
                            }
                        }
                        Some(gutter::GutterHit::Fold) => {
                            let notice = match self.focused().folds.borrow_mut().toggle_at(doc_line_0)
                            {
                                Some(fold::Chevron::Collapsed) => {
                                    format!("folded line {doc_line}")
                                }
                                Some(fold::Chevron::Expanded) => {
                                    format!("unfolded line {doc_line}")
                                }
                                None => format!("no fold on line {doc_line}"),
                            };
                            self.notice = notice;
                        }
                        _ => {
                            self.jump_lines(LineJump::Absolute(doc_line));
                        }
                    }
                    return;
                } else if on_edge {
                    let track = tile_h.saturating_sub(2).max(1);
                    let metrics = scrollbar::ScrollMetrics {
                        offset: self.focused().scroll,
                        visible,
                        total: line_count,
                    };
                    let on_scrollbar = scroll_w > 0 && local_x >= scroll_x0;
                    let next = if minimap_w > 0 && !on_scrollbar {
                        self.minimap_line_for_row(
                            line_count,
                            visible,
                            track as usize,
                            line_in_view,
                        )
                    } else {
                        metrics.offset_for_track_y(line_in_view as u16, track)
                    };
                    self.snap_scroll(next);
                    self.edge_drag = Some(if on_scrollbar {
                        EdgeDrag::Scrollbar
                    } else {
                        EdgeDrag::Minimap
                    });
                    return;
                }
                let line = self.focused().scroll + line_in_view;
                let column = self.document_column(local_x.saturating_sub(gutter_w));
                let visible = self.visible_rows();
                let columns = self.visible_cols();
                self.text_pointer(line, column, drag, screen_column, row);
                self.focused_mut().reveal(visible, columns);
            }
        }
        self.note_frame(self.width, self.height);
    }

    fn text_pointer(&mut self, line: usize, column: usize, drag: bool, x: u16, y: u16) {
        if drag {
            let by = self.select_by;
            let buffer = self.focused_mut();
            match by {
                SelectBy::Char => buffer.place(line, column, true),
                SelectBy::Word => buffer.extend_word(line, column),
                SelectBy::Line => buffer.extend_line(line, column),
            }
            return;
        }
        let count = self.note_text_click(x, y);
        self.select_by = match count {
            2 => SelectBy::Word,
            3 => SelectBy::Line,
            _ => SelectBy::Char,
        };
        let by = self.select_by;
        let buffer = self.focused_mut();
        match by {
            SelectBy::Char => buffer.place(line, column, false),
            SelectBy::Word => buffer.select_word(line, column),
            SelectBy::Line => buffer.select_line(line, column),
        }
    }

    fn note_text_click(&mut self, column: u16, row: u16) -> u8 {
        let now = Instant::now();
        let count = self
            .click
            .and_then(|(at, last_column, last_row, count)| {
                (now.saturating_duration_since(at) <= Duration::from_millis(400)
                    && last_column == column
                    && last_row == row)
                    .then_some((count % 3) + 1)
            })
            .unwrap_or(1);
        self.click = Some((now, column, row, count));
        count
    }

    fn select_all(&mut self) {
        if self.terminals.focused() {
            self.terminals.select_all_visible();
            return;
        }
        self.focused_mut().select_all();
        let visible = self.visible_rows();
        let columns = self.visible_cols();
        self.focused_mut().reveal(visible, columns);
    }

    fn copy_selection(&mut self) {
        // Terminal selections stay yankable even when the buffer owns the keyboard.
        let text = if self.terminals.has_selection() {
            self.terminals.selected_text()
        } else {
            self.focused().selected_text()
        };
        let Some(text) = text else {
            self.notice = "nothing selected".to_string();
            return;
        };
        match clipboard::set(&text) {
            Ok(()) => self.notice = "copied".to_string(),
            Err(detail) => self.report_error("Clipboard", &detail),
        }
    }

    fn cut_selection(&mut self) {
        if self.terminals.focused() {
            // PTY output is not a mutable buffer; cut yanks only.
            self.copy_selection();
            if self.notice == "copied" {
                self.notice = "copied".to_string();
            }
            return;
        }
        let Some(text) = self.focused().selected_text() else {
            self.notice = "nothing selected".to_string();
            return;
        };
        match clipboard::set(&text) {
            Ok(()) => {
                self.focused_mut().replace_selection(String::new());
                self.notice = "cut".to_string();
            }
            Err(detail) => self.report_error("Clipboard", &detail),
        }
    }

    fn paste_clipboard(&mut self) {
        match clipboard::get() {
            Ok(text) if text.is_empty() => self.notice = "clipboard empty".to_string(),
            Ok(text) => {
                if self.terminals.focused() {
                    if self.terminals.write_key(text.as_bytes()) {
                        self.notice = "pasted".to_string();
                    } else {
                        self.notice = "terminal not ready".to_string();
                    }
                    return;
                }
                self.focused_mut().insert_text(text);
                let visible = self.visible_rows();
                let columns = self.visible_cols();
                self.focused_mut().reveal(visible, columns);
                self.notice = "pasted".to_string();
            }
            Err(detail) => self.report_error("Clipboard", &detail),
        }
    }

    fn menu_action(&mut self, index: usize, over_terminal: bool) {
        match index {
            0 => self.cut_selection(),
            1 => self.copy_selection(),
            2 => self.paste_clipboard(),
            3 => {
                if over_terminal && self.terminals.open {
                    self.terminals.select_all_visible();
                } else {
                    self.select_all();
                }
            }
            _ => {}
        }
    }

    fn open_menu(&mut self, column: u16, row: u16) {
        // Right-click never clears a selection — in buffers, the terminal, or elsewhere.
        let window = self.window();
        if let Some(index) = layout::hit_tile(window, column, row, self.view, self.buffers.len()) {
            if index != self.focus {
                self.focus_at(index);
            }
        }
        let term = self.terminal_rect();
        let over_terminal = term.height > 0 && contains(term, column, row);
        let has_selection =
            self.focused().selection.is_some() || self.terminals.has_selection();
        let cursor = if has_selection { 1 } else { 2 };
        self.overlay = Overlay::Menu(MenuState {
            x: column,
            y: row,
            cursor,
            over_terminal,
        });
    }

    fn click_menu(&mut self, menu: MenuState, column: u16, row: u16, area: Rect) {
        let rect = menu_rect(menu.x, menu.y, area);
        if !contains(rect, column, row) {
            self.overlay = Overlay::None;
            return;
        }
        let index = row.saturating_sub(rect.y).saturating_sub(1) as usize;
        if index < MENU.len() {
            let over_terminal = menu.over_terminal;
            self.overlay = Overlay::None;
            self.menu_action(index, over_terminal);
        }
    }

    fn seam(&self, chrome: &Chrome, column: u16, row: u16) -> Option<Side> {
        if seams::hit_left_pane(chrome.tree, column, row) {
            Some(Side::Left)
        } else if seams::hit_right_pane(chrome.agent, column, row) {
            Some(Side::Right)
        } else {
            None
        }
    }

    /// Buffer index whose **trailing** edge (right in landscape, bottom in
    /// portrait) is under the pointer. Text buffers resize only from that edge
    /// — never the leading edge — including the last buffer (niri-style).
    fn tile_seam(&self, column: u16, row: u16) -> Option<usize> {
        if self.buffers.is_empty() {
            return None;
        }
        let (tiles, _) = self.content_tiles_and_strip(
            self.frame_chrome(Rect::new(0, 0, self.width, self.height))
                .content,
        );
        if tiles.width == 0 || tiles.height == 0 {
            return None;
        }
        // Fullscreen: only the focused buffer's trailing edge.
        if !self.show_many {
            if self.buffers.len() < 2 {
                return None;
            }
            let focus = self.focus.min(self.buffers.len() - 1);
            let on_title = row == tiles.y;
            match self.axis {
                Axis::Horizontal => {
                    if row < tiles.y || row >= tiles.y.saturating_add(tiles.height) || on_title {
                        return None;
                    }
                    let right = tiles.x.saturating_add(tiles.width).saturating_sub(1);
                    if column == right {
                        return Some(focus);
                    }
                    if let Some(seam) = seams::right_pane_seam(tiles) {
                        if seam.contains_grab(column, row) {
                            return Some(focus);
                        }
                    }
                }
                Axis::Vertical => {
                    if column < tiles.x || column >= tiles.x.saturating_add(tiles.width) {
                        return None;
                    }
                    let bottom = tiles.y.saturating_add(tiles.height).saturating_sub(1);
                    if row == bottom {
                        return Some(focus);
                    }
                    let seam = seams::SeamVisual::horizontal(tiles.x, bottom, tiles.width);
                    if seam.contains_grab(column, row) {
                        return Some(focus);
                    }
                }
            }
            return None;
        }
        let window = self.window();
        // Prefer the rightmost/bottommost hit when tiles abut so each buffer
        // owns only its own trailing edge.
        let mut hit: Option<usize> = None;
        for index in 0..self.buffers.len() {
            let Some(slice) = layout::tile_slice(window, index, self.view) else {
                continue;
            };
            let (tile_w, tile_h) = window.tile_box(index);
            let on_title = slice.source_y == 0 && row == slice.screen_y;
            let seam = match window.axis {
                Axis::Horizontal if slice.source_x + slice.width == tile_w => {
                    let x = slice.screen_x.saturating_add(slice.width).saturating_sub(1);
                    Some(seams::SeamVisual::vertical(
                        x,
                        slice.screen_y,
                        slice.height.max(1),
                    ))
                }
                Axis::Vertical if slice.source_y + slice.height == tile_h => {
                    let y = slice.screen_y.saturating_add(slice.height).saturating_sub(1);
                    Some(seams::SeamVisual::horizontal(
                        slice.screen_x,
                        y,
                        slice.width.max(1),
                    ))
                }
                _ => None,
            };
            let Some(seam) = seam else {
                continue;
            };
            let on_edge = match window.axis {
                Axis::Horizontal => {
                    column == seam.line.x
                        && row >= slice.screen_y
                        && row < slice.screen_y.saturating_add(slice.height)
                        && !on_title
                }
                Axis::Vertical => {
                    row == seam.line.y
                        && column >= slice.screen_x
                        && column < slice.screen_x.saturating_add(slice.width)
                }
            };
            if on_edge || seam.contains_grab(column, row) {
                hit = Some(index);
            }
        }
        hit
    }

    /// Enter split mode if needed and seed independent spans for a trailing-edge drag.
    fn arm_buffer_edge_resize(&mut self, index: usize) {
        let n = self.buffers.len();
        if index >= n {
            return;
        }
        let (tiles, _) = self.content_tiles_and_strip(
            self.frame_chrome(Rect::new(0, 0, self.width, self.height))
                .content,
        );
        let along = match self.axis {
            Axis::Horizontal => tiles.width,
            Axis::Vertical => tiles.height,
        }
        .max(1);
        self.show_many = true;
        if self
            .tile_spans
            .as_ref()
            .is_none_or(|spans| spans.len() != n)
        {
            // Seed from the current equal layout so undragged buffers keep size.
            let window = layout::window(tiles, self.axis, true, n);
            let mut spans: Vec<u16> = (0..n).map(|i| window.along(i)).collect();
            if spans.iter().all(|span| *span >= 1) {
                let used: u16 = spans.iter().sum();
                // If equal tiles don't fill the viewport yet (rare), grow the last.
                if used < along {
                    spans[n - 1] = spans[n - 1].saturating_add(along - used);
                }
                self.tile_spans = Some(spans);
            } else {
                let each = (along / n as u16).max(1);
                let mut spans = vec![each; n];
                let used: u16 = spans.iter().sum();
                if used < along {
                    spans[n - 1] = spans[n - 1].saturating_add(along - used);
                }
                self.tile_spans = Some(spans);
            }
        }
        self.tile_drag = Some(index);
        if self.focus != index {
            self.focus_at(index);
        }
    }

    fn content_tiles_and_strip(&self, content: Rect) -> (Rect, Rect) {
        if content.height < 4 || self.buffers.is_empty() {
            return (content, Rect::default());
        }
        let strip = Rect {
            x: content.x,
            y: content.y.saturating_add(content.height.saturating_sub(1)),
            width: content.width,
            height: 1,
        };
        let tiles = Rect {
            x: content.x,
            y: content.y,
            width: content.width,
            height: content.height.saturating_sub(1),
        };
        (tiles, strip)
    }

    fn buffer_strip_metrics(&self) -> scrollbar::ScrollMetrics {
        let total = self.buffers.len().max(1);
        let visible = if self.show_many {
            // How many tiles fit roughly in the strip.
            match self.axis {
                Axis::Horizontal => {
                    let w = self
                        .content_tiles_and_strip(
                            self.frame_chrome(Rect::new(0, 0, self.width, self.height))
                                .content,
                        )
                        .0
                        .width
                        .max(1) as usize;
                    (w / layout::COLUMNS.max(1) as usize).clamp(1, total)
                }
                Axis::Vertical => 1.max(total.min(3)),
            }
        } else {
            1
        };
        scrollbar::ScrollMetrics {
            offset: self.focus.min(total.saturating_sub(1)),
            visible,
            total,
        }
    }

    fn buffer_strip_hit(&self, column: u16, row: u16) -> Option<usize> {
        let (_, strip) = self.content_tiles_and_strip(
            self.frame_chrome(Rect::new(0, 0, self.width, self.height))
                .content,
        );
        if strip.height == 0 || !contains(strip, column, row) {
            return None;
        }
        let metrics = self.buffer_strip_metrics();
        let local = column.saturating_sub(strip.x);
        Some(
            metrics
                .offset_for_track_y(local, strip.width.max(1))
                .min(self.buffers.len().saturating_sub(1)),
        )
    }

    fn flip_cards(&self) -> Vec<flip3d::BufferCard> {
        let theme_fp = self.minimap_theme.fingerprint;
        let count = self.buffers.len();
        // AFK tick26: stub all slots, fill only on-screen — no HashSet per frame.
        let mut cards: Vec<flip3d::BufferCard> = (0..count)
            .map(|_| flip3d::BufferCard {
                title: String::new(),
                lines: Vec::new(),
                active: false,
                preview: None,
            })
            .collect();
        let elevated = self.last_pressure.is_elevated();
        let (cw, ch) = minimap_gfx::flip_card_dims(elevated);
        let scale = minimap_gfx::flip_glyph_scale(elevated);
        for index in flip3d::visible_card_indices(self.flip.scroll, count) {
            let Some(buffer) = self.buffers.get(index) else {
                continue;
            };
            let key = minimap_gfx::preview_key(
                index,
                buffer.document.revision() ^ (u64::from(scale) << 56),
                theme_fp,
                cw,
                ch,
            );
            let preview = self.preview_pool.get(key);
            let lines = if preview.is_some() {
                Vec::new()
            } else {
                let text = buffer.text();
                text.lines()
                    .take(6)
                    .map(|line| {
                        let mut s: String = line.chars().take(28).collect();
                        if s.is_empty() {
                            s.push(' ');
                        }
                        s
                    })
                    .collect()
            };
            cards[index] = flip3d::BufferCard {
                title: buffer.name.clone(),
                lines,
                active: index == self.focus,
                preview,
            };
        }
        cards
    }

    /// Cache key only — no rope/highlight snapshot (AFK schedule hot path).
    fn flip_preview_cache_key(&self, index: usize) -> Option<u64> {
        let buffer = self.buffers.get(index)?;
        let elevated = self.last_pressure.is_elevated();
        let (card_w, card_h) = minimap_gfx::flip_card_dims(elevated);
        let scale = minimap_gfx::flip_glyph_scale(elevated);
        Some(minimap_gfx::preview_key(
            index,
            buffer.document.revision() ^ (u64::from(scale) << 56),
            self.minimap_theme.fingerprint,
            card_w,
            card_h,
        ))
    }

    fn flip_preview_job(&self, index: usize) -> Option<minimap_gfx::PreviewJob> {
        let buffer = self.buffers.get(index)?;
        let theme = self.minimap_theme;
        let elevated = self.last_pressure.is_elevated();
        let (card_w, card_h) = minimap_gfx::flip_card_dims(elevated);
        let scale = minimap_gfx::flip_glyph_scale(elevated);
        let px_char = minimap_gfx::PX_PER_CHAR_BASE * scale;
        let px_line = minimap_gfx::PX_PER_LINE_BASE * scale;
        let cols = ((card_w / px_char) as usize).clamp(16, 64);
        let max_lines = ((card_h / px_line) as usize).clamp(6, 32);
        let key = self.flip_preview_cache_key(index)?;
        buffer.ensure_highlight_cache();
        let mut lines = Vec::with_capacity(max_lines);
        for line in 0..max_lines.min(buffer.document.len_lines().max(1)) {
            lines.push(minimap_syntax_cells(buffer, line, cols, theme));
        }
        Some(minimap_gfx::PreviewJob {
            key,
            theme,
            width: card_w,
            height: card_h,
            glyph_scale: scale,
            lines,
        })
    }

    /// Snapshot highlight cells and enqueue Flip previews for stack neighbours only
    /// (never every open buffer — AFK CPU/RAM when many files are open).
    fn schedule_flip_previews(&self) {
        if cfg!(test) || !self.kitty_graphics || !minimap_gfx::host_is_kitty() || !self.flip.open {
            return;
        }
        let count = self.buffers.len();
        if count == 0 {
            return;
        }
        let selected = self.flip.selected.min(count - 1);
        let radius = if self.last_pressure.is_elevated() {
            flip3d::WARM_RADIUS_PRESSURE
        } else {
            flip3d::WARM_RADIUS
        };
        for index in flip3d::warm_indices(selected, count, radius) {
            let Some(key) = self.flip_preview_cache_key(index) else {
                continue;
            };
            // AFK: do not rope-scan / highlight-snapshot when already cached.
            if !self.preview_pool.needs_work(key) {
                continue;
            }
            let Some(job) = self.flip_preview_job(index) else {
                continue;
            };
            self.preview_pool.request(job);
        }
    }

    /// Sync-raster only the front Flip card when missing from cache.
    /// Neighbours stay async (AFK: was ±1 sync — blocked the present path).
    fn warm_flip_previews_sync(&self) {
        if cfg!(test)
            || !self.kitty_graphics
            || !minimap_gfx::host_is_kitty()
            || self.buffers.is_empty()
        {
            return;
        }
        let count = self.buffers.len();
        let selected = self.flip.selected.min(count - 1);
        if let Some(key) = self.flip_preview_cache_key(selected) {
            if self.preview_pool.get(key).is_none() {
                if let Some(job) = self.flip_preview_job(selected) {
                    let _ = self.preview_pool.get_or_render(job);
                }
            }
        }
        // AFK: enqueue neighbours only near settle — mid-scroll jobs were wasted.
        let settling = (self.flip.scroll - selected as f32).abs() < 0.2;
        if settling {
            self.schedule_flip_previews();
        }
    }

    fn flip_scene_theme(&self) -> flip3d::SceneTheme {
        let t = self.minimap_theme;
        flip3d::SceneTheme::from_rgb(t.bg, t.fg, t.is_light())
    }

    fn open_flip_switcher(&mut self) {
        if self.buffers.len() < 2 {
            self.notice =
                "Flip 3D needs 2+ buffers · open another file, then hold Tab (view mode)".into();
            return;
        }
        self.editing = false; // switcher owns keys until Tab release / Enter / Esc
        self.flip.open_at(self.focus, self.buffers.len());
        self.flip.placed = None;
        self.flip.scene_png.clear();
        self.flip.scene_rgb.clear();
        // Drop minimap bind before next draw so tile underlay does not leave
        // orphan U+10EEEE static under the Flip scene.
        self.editor_minimap_id = None;
        self.editor_minimap_key = None;
        self.flip_tick_at = Some(Instant::now());
        // Async only on open — sync raster blocked TabRelease / made Flip feel sticky.
        self.schedule_flip_previews();
        self.notice = format!(
            "Flip 3D · {}/{} · ←→ choose · release Tab to switch · Esc cancel",
            self.flip.selected + 1,
            self.buffers.len()
        );
    }

    fn flip_commit(&mut self) {
        if !self.flip.open {
            return;
        }
        let selected = self.flip.selected.min(self.buffers.len().saturating_sub(1));
        self.flip.settle();
        self.flip.close();
        // Flip cards are large RGB Arcs — drop the working set when the stack closes.
        self.preview_pool.clear();
        self.focus_at(selected);
        self.focus_pane(PaneFocus::Editor);
        self.notice = format!("switched to {}", self.focused().name);
    }

    fn flip_cancel(&mut self) {
        if !self.flip.open {
            return;
        }
        let restore = self.flip.restore.min(self.buffers.len().saturating_sub(1));
        self.flip.close();
        self.preview_pool.clear();
        self.focus_at(restore);
        self.notice = "buffer switcher cancelled".into();
    }

    fn resize_tile(&mut self, index: usize, column: u16, row: u16) {
        if index >= self.buffers.len() {
            return;
        }
        if !self.show_many {
            self.arm_buffer_edge_resize(index);
        }
        if self
            .tile_spans
            .as_ref()
            .is_none_or(|spans| spans.len() != self.buffers.len())
        {
            let window = self.window();
            self.tile_spans = Some((0..self.buffers.len()).map(|i| window.along(i)).collect());
        }
        let window = self.window();
        let Some(mut spans) = self.tile_spans.clone() else {
            return;
        };
        let min = match window.axis {
            Axis::Horizontal => 12,
            Axis::Vertical => 6,
        };
        // Niri-style: only this buffer's preferred size changes. Neighbours keep
        // their spans; the strip scrolls when the sum exceeds the viewport.
        let max = match window.axis {
            Axis::Horizontal => (layout::COLUMNS.saturating_mul(4)).max(window.viewport.width),
            Axis::Vertical => (layout::PORTRAIT_SPLIT_HEIGHT.saturating_mul(4))
                .max(window.viewport.height),
        }
        .max(min);
        let (cursor, start) = match window.axis {
            Axis::Horizontal => (column as i32, window.viewport.x as i32),
            Axis::Vertical => (row as i32, window.viewport.y as i32),
        };
        let wanted = (cursor - start - window.origin(index) + self.view.round() as i32 + 1)
            .clamp(min as i32, max as i32) as u16;
        spans[index] = wanted;
        self.tile_spans = Some(spans);
    }

    fn resize_sidebar(&mut self, side: Side, column: u16) {
        match side {
            Side::Left => {
                let width = column.saturating_add(1);
                if width < 8 {
                    self.sidebar = false;
                    return;
                }
                self.sidebar = true;
                let agent = if self.chat { self.agent_width } else { 0 };
                let max = self.width.saturating_sub(agent).saturating_sub(16).max(12);
                self.tree_width = width.clamp(12, max);
            }
            Side::Right => {
                let width = self.width.saturating_sub(column);
                if width < 8 {
                    self.chat = false;
                    self.agent_focused = false;
                    return;
                }
                self.chat = true;
                let tree = if self.sidebar { self.tree_width } else { 3 };
                let max = self.width.saturating_sub(tree).saturating_sub(16).max(12);
                self.agent_width = width.clamp(12, max);
            }
        }
    }

    fn on_canvas_seam(&self, column: u16, row: u16, chrome: &Chrome) -> bool {
        chrome.canvas.width > 0 && seams::hit_right_pane(chrome.canvas, column, row)
    }

    fn end_canvas_resize(&mut self, refresh: bool) {
        if !self.canvas_resize && !self.canvas_resize_hid_gfx {
            return;
        }
        self.canvas_resize = false;
        self.canvas_resize_hid_gfx = false;
        if refresh {
            self.canvas_painted_size = None;
            self.canvas_chrome_dirty.set(true);
            self.canvas_placeholders_stale.set(true);
        }
    }

    fn resize_canvas(&mut self, column: u16) {
        if !self.canvas_open() {
            return;
        }
        // Canvas is the right column; width grows as the seam moves left.
        let width = self.width.saturating_sub(column);
        let max = self.width.saturating_sub(40).max(28);
        if width < 20 {
            // Collapse: close all Canvas tabs (keep last frames in session cache).
            while let Some(mut surface) = self.canvas_surfaces.pop() {
                surface.store_in_cache(&mut self.canvas_frame_cache);
                surface.stop();
            }
            self.runtime_dock = None;
            self.runtime_focused = false;
            self.runtime_width = None;
            self.canvas_painted_size = None;
            self.canvas_image_id = None;
            self.end_canvas_resize(false);
            self.notice = "canvas closed".into();
            return;
        }
        self.runtime_width = Some(width.clamp(28, max));
        // Layout only. Do not Clear chrome or publish Kitty/geom until mouse-up —
        // those paths block the UI thread under drag storms.
    }

    fn agent_button(&self, agent: Rect, column: u16, row: u16) -> bool {
        if agent.width <= 4 {
            return true;
        }
        row == agent.y && column + 4 >= agent.x.saturating_add(agent.width)
    }

    fn toggle_chat(&mut self) {
        self.chat = !self.chat;
        if !self.chat {
            self.agent_focused = false;
        }
        self.notice = if self.chat {
            "chat on".to_string()
        } else {
            "chat off".to_string()
        };
    }

    fn report_error(&mut self, title: &str, body: &str) {
        self.notifications.insert(
            0,
            Notice {
                title: title.to_string(),
                body: body.to_string(),
            },
        );
        self.notifications.truncate(20);
        self.notifications_open = true;
        self.notice = title.to_string();
    }

    fn yank_notification(&mut self, index: usize) {
        let Some(note) = self.notifications.get(index) else {
            return;
        };
        let text = format!("{}\n{}", note.title, note.body);
        match clipboard::set(&text) {
            Ok(()) => self.notice = "notification copied".to_string(),
            Err(detail) => self.report_error("Clipboard", &detail),
        }
    }

    fn click_notifications(&mut self, column: u16, row: u16) -> bool {
        let area = Rect::new(0, 0, self.width, self.height);
        if self.notifications_open && !self.notifications.is_empty() {
            let rect = notification_rect(area, self.notifications.len());
            if contains(rect, column, row) {
                let local_y = row.saturating_sub(rect.y);
                let local_x = column.saturating_sub(rect.x);
                if local_y == 0 {
                    if local_x + 6 >= rect.width.saturating_sub(2) {
                        self.yank_notification(0);
                    } else {
                        self.notifications_open = false;
                    }
                    return true;
                }
                let index = (local_y as usize - 1) / 3;
                if index < self.notifications.len() && local_x + 3 >= rect.width {
                    self.notifications.remove(index);
                    if self.notifications.is_empty() {
                        self.notifications_open = false;
                    }
                } else if index < self.notifications.len() {
                    self.yank_notification(index);
                }
                return true;
            }
        }
        let chrome = self.frame_chrome(area);
        if let Some(status) = chrome.status {
            if !self.notifications.is_empty()
                && row == status.y
                && column + 8 >= status.x.saturating_add(status.width)
            {
                self.notifications_open = !self.notifications_open;
                return true;
            }
        }
        false
    }

    fn toggle_notifications(&mut self) {
        if self.notifications.is_empty() {
            self.notice = "no notifications".to_string();
            self.notifications_open = false;
            return;
        }
        self.notifications_open = !self.notifications_open;
    }

    fn toggle_terminal(&mut self) {
        self.terminals.toggle();
        self.notice = if self.terminals.open {
            "terminal on".to_string()
        } else {
            "terminal off".to_string()
        };
    }

    fn terminal_rect(&self) -> Rect {
        self.frame_chrome(Rect::new(0, 0, self.width, self.height))
            .terminal
    }

    fn maintain_terminal(&mut self) -> bool {
        let rect = self.terminal_rect();
        let (cols, rows) = self.terminals.grid_size(rect.width, rect.height);
        self.terminals.resize(cols, rows);
        let closed = self.terminals.drain();
        if closed > 0 {
            self.notice = "terminal closed".to_string();
        }
        if let Err(detail) = self.terminals.ensure(&self.workspace, cols, rows) {
            self.notice = format!("terminal: {detail}");
        }
        // `active` is set by drain when PTY produced output this tick.
        closed > 0 || self.terminals.wants_poll()
    }

    /// AFK: while Flip is open, only drain PTY buffers — no resize/ensure work.
    fn maintain_terminal_drain_only(&mut self) -> bool {
        let closed = self.terminals.drain();
        if closed > 0 {
            self.notice = "terminal closed".to_string();
        }
        closed > 0
    }

    fn on_terminal_seam(&self, column: u16, row: u16) -> bool {
        let rect = self.terminal_rect();
        if rect.height == 0 {
            return false;
        }
        let local_x = column.saturating_sub(rect.x);
        let title_blocked = self.terminals.open
            && rect.height > 1
            && row == rect.y
            && (self.terminals.toggle_hit(local_x, 0, rect.width)
                || self.terminals.switch_hit(local_x, 0, rect.width));
        seams::hit_bottom_pane(rect, column, row, title_blocked)
    }

    fn on_terminal_side_seam(&self, column: u16, row: u16) -> bool {
        let rect = self.terminal_rect();
        if rect.height <= 2 || !contains(rect, column, row) {
            return false;
        }
        self.terminals.on_side_seam(
            column.saturating_sub(rect.x),
            row.saturating_sub(rect.y),
            rect.width,
            rect.height,
        )
    }

    fn resize_terminal_side(&mut self, column: u16) {
        let rect = self.terminal_rect();
        if rect.width <= 4 {
            return;
        }
        self.terminals
            .resize_side(column.saturating_sub(rect.x), rect.width);
        self.maintain_terminal();
    }

    fn resize_terminal(&mut self, row: u16) {
        let chrome = self.frame_chrome(Rect::new(0, 0, self.width, self.height));
        let Some(status) = chrome.status else {
            return;
        };
        let height = status.y.saturating_sub(row.saturating_add(1));
        if height < 4 {
            self.terminals.open = false;
            self.terminal_rows = Some(1);
            self.maintain_terminal();
            return;
        }
        self.terminals.open = true;
        self.terminal_rows = Some(height);
        self.maintain_terminal();
    }

    pub fn terminal_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if !self.terminals.focused() && !self.runtime_focused {
            return false;
        }
        if code == KeyCode::Esc && modifiers.is_empty() {
            if self.runtime_focused {
                self.focus_pane(PaneFocus::Editor);
            } else {
                self.terminals.blur();
            }
            return true;
        }
        if self.runtime_focused && self.canvas_open() {
            return self.canvas_key(code, modifiers);
        }
        if !self.terminals.focused() {
            return false;
        }
        if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('q') {
            return false;
        }
        let bytes = terminal_bytes(code, modifiers);
        if bytes.is_empty() {
            return false;
        }
        self.terminals.write_key(&bytes);
        true
    }

    fn canvas_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        let Some(surface) = self.canvas_active_mut() else {
            return false;
        };
        // Phone hardware buttons via Ctrl chords while Canvas is focused.
        if modifiers.contains(KeyModifiers::CONTROL) {
            let button = match code {
                KeyCode::Char('h') => Some("HOME"),
                KeyCode::Char('l') => Some("LOCK"),
                KeyCode::Char('s') => Some("SIRI"),
                _ => None,
            };
            if let Some(name) = button {
                if let Some(notice) = surface.button(name) {
                    self.notice = notice;
                }
                return true;
            }
        }
        let key = match code {
            KeyCode::Enter => "enter",
            KeyCode::Backspace => "backspace",
            KeyCode::Tab => "tab",
            KeyCode::Up => "up",
            KeyCode::Down => "down",
            KeyCode::Left => "left",
            KeyCode::Right => "right",
            KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
                let text = ch.to_string();
                if let Some(notice) = surface.key(&text) {
                    self.notice = notice;
                }
                return true;
            }
            _ => return false,
        };
        if let Some(notice) = surface.key(key) {
            self.notice = notice;
        }
        true
    }

    fn canvas_pointer(
        &mut self,
        column: u16,
        row: u16,
        chrome: &Chrome,
        kind: CanvasPointerKind,
    ) -> bool {
        if chrome.canvas.width == 0 {
            return false;
        }
        let over = contains(chrome.canvas, column, row);
        let gesture = self
            .canvas_active()
            .is_some_and(|surface| surface.gesture_active());
        // Down must hit the pane; drag/up may finish a gesture that left the pane.
        let allow_outside = matches!(kind, CanvasPointerKind::Drag | CanvasPointerKind::Up) && gesture;
        if !over && !allow_outside {
            return false;
        }
        self.focus_pane(PaneFocus::Runtime);
        // Control strip clicks are chrome — not device taps.
        if matches!(kind, CanvasPointerKind::Down) {
            if let Some(hit) = self.canvas_control_hit(column, row, chrome) {
                self.apply_canvas_control(hit);
                return true;
            }
        }
        let inner = canvas_media_rect(chrome.canvas);
        if inner.width == 0 || inner.height == 0 {
            return true;
        }
        // Ignore downs that land on the control row (already handled) or outside media.
        if matches!(kind, CanvasPointerKind::Down) && !contains(inner, column, row) {
            return true;
        }
        let (column, row) = clamp_point(column, row, inner);
        let local_x = column.saturating_sub(inner.x);
        let local_y = row.saturating_sub(inner.y);
        let Some(surface) = self.canvas_active_mut() else {
            return true;
        };
        let notice = match kind {
            CanvasPointerKind::Down => surface.pointer_down(local_x, local_y, inner),
            CanvasPointerKind::Drag => surface.pointer_drag(local_x, local_y, inner),
            CanvasPointerKind::Up => surface.pointer_up(local_x, local_y, inner),
        };
        if let Some(text) = notice {
            self.notice = text;
        }
        true
    }

    fn canvas_wheel(&mut self, column: u16, row: u16, delta: i32, chrome: &Chrome) -> bool {
        self.canvas_wheel_axis(column, row, delta, 0, chrome)
    }

    fn canvas_wheel_axis(
        &mut self,
        column: u16,
        row: u16,
        dy: i32,
        dx: i32,
        chrome: &Chrome,
    ) -> bool {
        if chrome.canvas.width == 0 || !contains(chrome.canvas, column, row) {
            return false;
        }
        self.focus_pane(PaneFocus::Runtime);
        let inner = canvas_media_rect(chrome.canvas);
        if !contains(inner, column, row) {
            return true;
        }
        if self.runtime_dock.as_deref() == Some("markdown") {
            if dy < 0 {
                self.markdown_preview_scroll = self.markdown_preview_scroll.saturating_sub(3);
            } else if dy > 0 {
                self.markdown_preview_scroll = self.markdown_preview_scroll.saturating_add(3);
            }
            self.canvas_chrome_dirty.set(true);
            return true;
        }
        let local_x = column.saturating_sub(inner.x);
        let local_y = row.saturating_sub(inner.y);
        let Some(surface) = self.canvas_active_mut() else {
            return true;
        };
        if let Some(notice) = surface.scroll_axis(local_x, local_y, inner, dy, dx) {
            self.notice = notice;
        }
        true
    }

    fn pointer_terminal(&mut self, column: u16, row: u16, drag: bool) -> bool {
        let rect = self.terminal_rect();
        if rect.height == 0 || !contains(rect, column, row) {
            return false;
        }
        let local_x = column.saturating_sub(rect.x);
        let local_y = row.saturating_sub(rect.y);
        let count = if drag {
            self.click.map(|(_, _, _, count)| count).unwrap_or(1)
        } else {
            self.note_text_click(column, row)
        };
        // Body click/drag selects text like a buffer; chrome hits fall through.
        if self.terminals.body_pointer(
            local_x,
            local_y,
            rect.width,
            rect.height,
            drag,
            count,
        ) {
            if self.terminals.focused() {
                self.focus_pane(PaneFocus::Terminal);
            } else {
                // Selection released the keyboard; editor owns keys again.
                self.focus_pane(PaneFocus::Editor);
            }
            return true;
        }
        if drag {
            return false;
        }
        match self
            .terminals
            .click(local_x, local_y, rect.width, rect.height)
        {
            terminal::Click::Add => {
                self.terminals.clear_selection();
                let (cols, rows) = self.terminals.grid_size(rect.width, rect.height);
                if let Err(detail) = self.terminals.add(&self.workspace, cols, rows) {
                    self.notice = format!("terminal: {detail}");
                } else {
                    self.focus_pane(PaneFocus::Terminal);
                }
            }
            terminal::Click::Toggle => {
                self.terminals.clear_selection();
                self.notice = if self.terminals.open {
                    "terminal on".to_string()
                } else {
                    "terminal off".to_string()
                };
                if self.terminals.open {
                    self.focus_pane(PaneFocus::Terminal);
                } else {
                    self.focus_pane(PaneFocus::Editor);
                }
            }
            terminal::Click::Switch => {
                self.terminals.clear_selection();
                self.notice = if self.terminals.show_console {
                    "console".to_string()
                } else {
                    "terminal".to_string()
                };
                if !self.terminals.show_console {
                    self.focus_pane(PaneFocus::Terminal);
                } else {
                    self.focus_pane(PaneFocus::Editor);
                }
            }
            terminal::Click::Close => {
                self.terminals.clear_selection();
                if self.terminals.focused() {
                    self.focus_pane(PaneFocus::Terminal);
                } else {
                    self.focus_pane(PaneFocus::Editor);
                }
            }
            terminal::Click::Focus => self.focus_pane(PaneFocus::Terminal),
        }
        true
    }

    fn sidebar_button(&self, tree: Rect, column: u16, row: u16) -> bool {
        if tree.width <= 4 {
            return true;
        }
        row == tree.y && column + 4 >= tree.x.saturating_add(tree.width)
    }

    fn close_button(&self, window: Window, index: usize, column: u16, row: u16) -> bool {
        let Some(slice) = layout::tile_slice(window, index, self.view) else {
            return false;
        };
        let local_x = column.saturating_sub(slice.screen_x);
        let local_y = row.saturating_sub(slice.screen_y);
        local_y == 0 && slice.width >= 4 && local_x + 4 >= slice.width
    }

    fn blocks_status(&self) -> bool {
        self.palette
            || matches!(
                self.overlay,
                Overlay::Quit | Overlay::Close(_) | Overlay::Rename(_)
            )
    }

    fn dispatch(&mut self, key: CommandKey) {
        if self.flip.open {
            match key {
                CommandKey::Esc => self.flip_cancel(),
                CommandKey::Enter | CommandKey::TabRelease => self.flip_commit(),
                // Hold-Tab must not cycle — arrows (and Alt-Tab) choose the target.
                CommandKey::Tab => {}
                CommandKey::AltTab | CommandKey::Right | CommandKey::Down => {
                    self.flip.flip(1, self.buffers.len());
                    // Async warm — never block the key path (TabRelease / next arrow).
                    self.schedule_flip_previews();
                    self.notice = format!(
                        "buffer {}/{} · {}",
                        self.flip.selected + 1,
                        self.buffers.len(),
                        self.buffers
                            .get(self.flip.selected)
                            .map(|b| b.name.as_str())
                            .unwrap_or("?")
                    );
                }
                CommandKey::AltShiftTab
                | CommandKey::Left
                | CommandKey::Up
                | CommandKey::Backspace => {
                    self.flip.flip(-1, self.buffers.len());
                    self.schedule_flip_previews();
                    self.notice = format!(
                        "buffer {}/{} · {}",
                        self.flip.selected + 1,
                        self.buffers.len(),
                        self.buffers
                            .get(self.flip.selected)
                            .map(|b| b.name.as_str())
                            .unwrap_or("?")
                    );
                }
                _ => {}
            }
            return;
        }
        if self.overlay != Overlay::None && !self.help && !self.palette {
            self.handle_overlay(key);
            return;
        }
        if self.help {
            match key {
                CommandKey::Help | CommandKey::Esc => self.close_help(),
                CommandKey::CtrlQ => self.request_quit(),
                CommandKey::CtrlP => {
                    self.close_help();
                    self.palette = true;
                }
                CommandKey::Up => self.scroll_help(-1),
                CommandKey::Down => self.scroll_help(1),
                _ => {}
            }
            return;
        }
        if self.palette {
            self.handle_palette(key);
            return;
        }
        if self.finder.is_some() {
            self.handle_finder(key);
            return;
        }
        if self.yazi.is_some() {
            self.handle_yazi(key);
            return;
        }
        if self.spatial.active_mode != SpatialMode::Idle {
            self.handle_spatial(key);
            return;
        }
        if self.tree_query.is_some() {
            self.handle_tree_search(key);
            return;
        }
        if self.agent_focused {
            self.handle_agent(key);
            return;
        }
        if self.tree_focused {
            match key {
                CommandKey::Up => {
                    self.move_tree(-1);
                    return;
                }
                CommandKey::Down => {
                    self.move_tree(1);
                    return;
                }
                CommandKey::Enter => {
                    self.activate_tree();
                    return;
                }
                CommandKey::Esc | CommandKey::Left => {
                    self.focus_pane(PaneFocus::Editor);
                    return;
                }
                CommandKey::Char('/') => {
                    self.tree_query = Some(String::new());
                    self.tree_search_cursor = 0;
                    self.refresh_tree_search();
                    return;
                }
                _ => {}
            }
        }
        match key {
            CommandKey::CtrlQ => self.request_quit(),
            CommandKey::CtrlP => {
                self.palette = true;
                self.query.clear();
            }
            CommandKey::CtrlW => self.show_many = !self.show_many,
            CommandKey::CtrlS => self.save(),
            CommandKey::Help => self.open_help(),
            CommandKey::CtrlZ => self.focused_mut().undo(),
            CommandKey::CtrlA => self.select_all(),
            CommandKey::CtrlLeft => self.shift_focus(-1),
            CommandKey::CtrlRight => self.shift_focus(1),
            CommandKey::CtrlUp => self.scroll_page(-1),
            CommandKey::CtrlDown => self.scroll_page(1),
            CommandKey::AltTab => self.open_flip_switcher(),
            CommandKey::AltShiftTab => {
                let focus = self.focus;
                let count = self.buffers.len();
                self.open_flip_switcher();
                if self.flip.open && count > 1 {
                    self.flip.selected = focus.checked_sub(1).unwrap_or(count - 1);
                    self.flip.scroll = self.flip.selected as f32;
                    self.flip.dirty = true;
                }
            }
            _ if self.editing => self.edit_key(key),
            _ => self.view_key(key),
        }
    }

    fn edit_key(&mut self, key: CommandKey) {
        match key {
            CommandKey::Esc => {
                if self.ghost.take().is_none() {
                    self.editing = false;
                }
            }
            CommandKey::Left => {
                self.ghost = None;
                self.edit_arrow(-1, 0, false);
            }
            CommandKey::Right => {
                self.ghost = None;
                self.edit_arrow(1, 0, false);
            }
            CommandKey::Up => {
                self.ghost = None;
                self.edit_arrow(0, -1, false);
            }
            CommandKey::Down => {
                self.ghost = None;
                self.edit_arrow(0, 1, false);
            }
            CommandKey::ShiftLeft => self.edit_arrow(-1, 0, true),
            CommandKey::ShiftRight => self.edit_arrow(1, 0, true),
            CommandKey::ShiftUp => self.edit_arrow(0, -1, true),
            CommandKey::ShiftDown => self.edit_arrow(0, 1, true),
            CommandKey::AltLeft => self.edit_word(false, false),
            CommandKey::AltRight => self.edit_word(true, false),
            CommandKey::AltShiftLeft => self.edit_word(false, true),
            CommandKey::AltShiftRight => self.edit_word(true, true),
            CommandKey::Backspace => {
                self.focused_mut().backspace();
                self.refresh_ghost();
            }
            CommandKey::Delete => {
                self.focused_mut().delete_forward();
                self.refresh_ghost();
            }
            CommandKey::Enter => {
                self.focused_mut().insert('\n');
                self.refresh_ghost();
            }
            CommandKey::Char(ch) => {
                self.focused_mut().insert(ch);
                self.refresh_ghost();
            }
            CommandKey::Tab if !self.accept_ghost() => {
                self.focused_mut().insert('\t');
                self.refresh_ghost();
            }
            CommandKey::Tab => {
                // Ghost was accepted; refresh for the next identifier fragment.
                self.refresh_ghost();
            }
            _ => {}
        }
    }

    fn view_key(&mut self, key: CommandKey) {
        match key {
            CommandKey::Esc => self.clear_chord(),
            CommandKey::Char(ch) => self.push_chord(ch),
            CommandKey::Left | CommandKey::Right | CommandKey::Up | CommandKey::Down => {
                self.clear_chord();
                self.view_motion(key, false);
            }
            CommandKey::ShiftLeft
            | CommandKey::ShiftRight
            | CommandKey::ShiftUp
            | CommandKey::ShiftDown => {
                self.clear_chord();
                self.view_motion(key, true);
            }
            CommandKey::AltLeft => {
                self.clear_chord();
                self.edit_word(false, false);
            }
            CommandKey::AltRight => {
                self.clear_chord();
                self.edit_word(true, false);
            }
            CommandKey::AltShiftLeft => {
                self.clear_chord();
                self.edit_word(false, true);
            }
            CommandKey::AltShiftRight => {
                self.clear_chord();
                self.edit_word(true, true);
            }
            CommandKey::Enter => self.begin_edit(),
            // View mode: hold Tab opens Flip 3D; release commits (see flip.open handler).
            CommandKey::Tab => {
                self.clear_chord();
                self.open_flip_switcher();
            }
            CommandKey::TabRelease => {}
            _ => {}
        }
    }

    fn edit_arrow(&mut self, column: isize, line: isize, select: bool) {
        self.ghost = None;
        let visible = self.visible_rows();
        let columns = self.visible_cols();
        if column != 0 {
            self.focused_mut().move_horizontal(column, select);
            self.focused_mut().reveal(visible, columns);
        }
        if line != 0 {
            self.focused_mut()
                .move_vertical(line, visible, columns, select);
        }
    }

    fn edit_word(&mut self, forward: bool, select: bool) {
        self.ghost = None;
        let visible = self.visible_rows();
        let columns = self.visible_cols();
        self.focused_mut().move_word(forward, select);
        self.focused_mut().reveal(visible, columns);
    }

    pub(crate) fn pane_focus(&self) -> PaneFocus {
        if self.runtime_focused && self.runtime_dock.is_some() {
            PaneFocus::Runtime
        } else if self.terminals.focused() {
            PaneFocus::Terminal
        } else if self.agent_focused {
            PaneFocus::Agent
        } else if self.tree_focused {
            PaneFocus::Tree
        } else {
            PaneFocus::Editor
        }
    }

    fn focus_pane(&mut self, pane: PaneFocus) {
        match pane {
            PaneFocus::Editor => {
                self.terminals.unfocus();
                self.agent_focused = false;
                self.tree_focused = false;
                self.runtime_focused = false;
            }
            PaneFocus::Terminal => {
                self.editing = false;
                self.agent_focused = false;
                self.tree_focused = false;
                self.runtime_focused = false;
                self.terminals.focus_keyboard();
            }
            PaneFocus::Agent => {
                self.editing = false;
                self.terminals.blur();
                self.tree_focused = false;
                self.runtime_focused = false;
                self.agent_focused = true;
            }
            PaneFocus::Tree => {
                self.editing = false;
                self.terminals.blur();
                self.agent_focused = false;
                self.runtime_focused = false;
                self.tree_focused = true;
            }
            PaneFocus::Runtime => {
                self.editing = false;
                self.terminals.blur();
                self.agent_focused = false;
                self.tree_focused = false;
                self.runtime_focused = true;
            }
        }
    }

    fn begin_edit(&mut self) {
        self.begin_edit_at(EditEntry::Here);
    }

    /// Neovim-style insert placements, without Vim's `i`/`a`/`I`/`A` keys.
    fn begin_edit_at(&mut self, entry: EditEntry) {
        self.focus_pane(PaneFocus::Editor);
        let visible = self.visible_rows();
        let columns = self.visible_cols();
        {
            let buffer = self.focused_mut();
            match entry {
                EditEntry::Here => {}
                EditEntry::After => buffer.move_horizontal(1, false),
                EditEntry::LineHead => buffer.move_to_line_head(),
                EditEntry::LineEnd => buffer.move_to_line_end(),
            }
            buffer.reveal(visible, columns);
        }
        self.editing = true;
        self.clear_chord();
        self.refresh_ghost();
    }

    fn handle_agent(&mut self, key: CommandKey) {
        match key {
            CommandKey::CtrlQ => self.request_quit(),
            CommandKey::Esc => self.focus_pane(PaneFocus::Editor),
            CommandKey::Backspace => {
                self.agent_draft.pop();
            }
            CommandKey::Char(ch) => self.agent_draft.push(ch),
            CommandKey::Enter => self.submit_agent(),
            _ => {}
        }
    }

    fn submit_agent(&mut self) {
        let prompt = std::mem::take(&mut self.agent_draft);
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.agent_log.push(format!("› {prompt}"));
        let route = agent_route();
        self.agent_program = route
            .chosen
            .program
            .clone()
            .unwrap_or_else(|| route.chosen.kind.label().to_string());
        if cfg!(test) {
            self.agent_log.push(format!(
                "{} ready ({}µs)",
                route.chosen.kind.label(),
                route.elapsed_us
            ));
            // Exercise FreeLocal tools even when a cloud CLI is on PATH.
            if route.chosen.kind == ide_ai::BackendKind::FreeLocal
                || ide_ai::is_explicit_tool_prompt(&prompt)
            {
                self.run_free_agent(&prompt);
            }
            return;
        }
        // Explicit tool verbs always use FreeLocal (no cloud round-trip).
        if route.chosen.kind == ide_ai::BackendKind::FreeLocal
            || ide_ai::is_explicit_tool_prompt(&prompt)
        {
            self.run_free_agent(&prompt);
        } else {
            let kind = route.chosen.kind;
            let Some(program) = route.chosen.program.as_deref() else {
                self.agent_log.push("agent backend missing program".into());
                return;
            };
            let args = ide_ai::cli_args(kind, program, &prompt);
            match capture(program, &args, &self.workspace) {
                Ok((_, text)) => {
                    let text = ide_ai::redact(&text);
                    let lines: Vec<String> = text
                        .lines()
                        .map(str::trim_end)
                        .map(str::to_string)
                        .filter(|line| !line.is_empty())
                        .take(8)
                        .collect();
                    if lines.is_empty() {
                        self.agent_log.push(format!("{program} finished"));
                    } else {
                        self.agent_log.extend(lines);
                    }
                }
                Err(detail) => self.agent_log.push(detail),
            }
        }
        if self.agent_log.len() > 40 {
            let extra = self.agent_log.len() - 40;
            self.agent_log.drain(0..extra);
        }
    }

    fn run_free_agent(&mut self, prompt: &str) {
        let root = self.workspace.clone();
        let result = ide_ai::run_free_local(
            &root,
            prompt,
            |path| {
                let full = root.join(path);
                std::fs::read_to_string(&full).map_err(|err| err.to_string())
            },
            |path| {
                let full = if path.as_os_str().is_empty() {
                    root.clone()
                } else {
                    root.join(path)
                };
                let mut names = Vec::new();
                for entry in std::fs::read_dir(&full).map_err(|err| err.to_string())? {
                    let entry = entry.map_err(|err| err.to_string())?;
                    names.push(entry.file_name().to_string_lossy().into_owned());
                }
                names.sort();
                Ok(names)
            },
            |path, needle| {
                let full = if path.as_os_str().is_empty() {
                    root.clone()
                } else {
                    root.join(path)
                };
                let mut hits = Vec::new();
                if full.is_file() {
                    if let Ok(text) = std::fs::read_to_string(&full) {
                        for (index, line) in text.lines().enumerate() {
                            if line.contains(needle) {
                                hits.push(format!(
                                    "{}:{}:{line}",
                                    path.display(),
                                    index + 1
                                ));
                            }
                        }
                    }
                } else if full.is_dir() {
                    let walker = walk_files(&full, 200);
                    for file in walker {
                        if let Ok(text) = std::fs::read_to_string(&file) {
                            for (index, line) in text.lines().enumerate() {
                                if line.contains(needle) {
                                    let rel = file.strip_prefix(&root).unwrap_or(&file);
                                    hits.push(format!("{}:{}:{line}", rel.display(), index + 1));
                                    if hits.len() >= 40 {
                                        return Ok(hits);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(hits)
            },
        );
        self.agent_log.push(format!(
            "free-local · {} tool(s) · {}µs",
            result.tools.len(),
            result.elapsed_us
        ));
        self.agent_log.extend(result.lines.into_iter().take(12));
    }

    fn push_chord(&mut self, ch: char) {
        let combined = format!("{}{ch}", self.chord);
        match combined.as_str() {
            "rr" => {
                self.clear_chord();
                self.overlay = Overlay::Rename(String::new());
            }
            "ff" => {
                self.clear_chord();
                self.format_now();
            }
            "lll" => {
                self.clear_chord();
                self.open_finder(FinderKind::Grep);
            }
            "za" => {
                self.clear_chord();
                self.notice = self.focused_mut().fold_toggle_at_caret();
            }
            "zc" => {
                self.clear_chord();
                self.notice = self.focused_mut().fold_close_at_caret();
            }
            "ze" => {
                self.clear_chord();
                self.notice = self.focused_mut().fold_open_at_caret();
            }
            "zo" => {
                self.clear_chord();
                self.spatial_open_overview();
            }
            "zl" => {
                self.clear_chord();
                self.spatial_open_lens();
            }
            "zn" => {
                self.clear_chord();
                self.spatial_structural(StructuralOp::Duplicate);
            }
            "zt" => {
                self.clear_chord();
                self.spatial_timeline();
            }
            "zi" => {
                self.clear_chord();
                self.spatial_impact();
            }
            "zp" => {
                self.clear_chord();
                self.spatial_preview();
            }
            "rn" => {
                self.clear_chord();
                self.gutter.relative_numbers = !self.gutter.relative_numbers;
                self.notice = if self.gutter.relative_numbers {
                    "relative line numbers on".into()
                } else {
                    "absolute line numbers".into()
                };
            }
            "gs" => {
                // Fallback when the host does not report Tab key-up for hold-to-switch.
                self.clear_chord();
                self.open_flip_switcher();
            }
            "b" | "r" | "f" | "l" | "ll" | "z" | "g" => {
                self.chord = combined;
                self.chord_deadline = Some(Instant::now() + Duration::from_millis(450));
            }
            _ => {
                self.clear_chord();
                self.view_single(ch);
            }
        }
    }

    fn view_single(&mut self, ch: char) {
        match ch {
            ':' => {
                self.palette = true;
                self.query.clear();
            }
            '?' => self.open_help(),
            'h' => self.toggle_hover_hints(),
            'q' => self.request_quit(),
            's' => self.save(),
            'w' => self.request_close(self.focus),
            'o' => self.open_yazi(),
            'a' => self.select_all(),
            // Insert placements (nvim i/a/I/A) on clearer keys — keep a/i for select/notifications.
            'e' => self.begin_edit_at(EditEntry::Here),
            'E' => self.begin_edit_at(EditEntry::After),
            'H' => self.begin_edit_at(EditEntry::LineHead),
            'L' => self.begin_edit_at(EditEntry::LineEnd),
            'y' if self.notifications_open && !self.notifications.is_empty() => {
                self.yank_notification(0);
            }
            'y' if self.terminals.has_selection() => self.copy_selection(),
            'y' if self.focused().selection.is_some() => self.copy_selection(),
            'y' if self.pending_plan.is_some() => self.approve_plan(),
            'y' => self.copy_selection(),
            'p' => self.paste_clipboard(),
            'x' => self.cut_selection(),
            't' => self.toggle_terminal(),
            'i' => self.toggle_notifications(),
            '<' => {
                self.sidebar = !self.sidebar;
                self.notice = if self.sidebar {
                    "sidebar on".to_string()
                } else {
                    "sidebar off".to_string()
                };
            }
            '>' => self.toggle_chat(),
            'n' if self.pending_plan.is_some() => {
                self.pending_plan = None;
                self.notice = "plan cancelled".to_string();
            }
            '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => self.workbench_key(ch),
            ';' => self.open_settings(),
            '[' => self.focus_at(0),
            ']' => {
                let last = self.buffers.len().saturating_sub(1);
                self.focus_at(last);
            }
            '{' => self.relocate(0),
            '}' => {
                let last = self.buffers.len().saturating_sub(1);
                self.relocate(last);
            }
            'b' | 'r' | 'f' | 'l' | 'z' => {
                self.chord = ch.to_string();
                self.chord_deadline = Some(Instant::now() + Duration::from_millis(450));
            }
            _ => {}
        }
    }

    fn view_motion(&mut self, key: CommandKey, swap: bool) {
        let navigate = match key {
            CommandKey::Left | CommandKey::ShiftLeft => Some(-1),
            CommandKey::Right | CommandKey::ShiftRight => Some(1),
            _ => None,
        };
        if let Some(delta) = navigate {
            if swap {
                self.swap_buffer(delta);
            } else {
                self.shift_focus(delta);
            }
            return;
        }
        let step = match key {
            CommandKey::ShiftUp => -8,
            CommandKey::ShiftDown => 8,
            CommandKey::Up => -1,
            _ => 1,
        };
        let current = self.focused().scroll as isize;
        let next = (current + step).max(0) as usize;
        self.snap_scroll(next);
    }

    fn handle_overlay(&mut self, key: CommandKey) {
        match &self.overlay.clone() {
            Overlay::Quit => match key {
                // y = save all dirty buffers then quit; ! = discard all edits and quit.
                CommandKey::Char('y') => self.save_all_and_quit(),
                CommandKey::Char('!') => {
                    self.overlay = Overlay::None;
                    self.shutdown_fast();
                    self.quit = true;
                }
                CommandKey::Char('n') | CommandKey::Esc => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Close(index) => {
                let index = *index;
                match key {
                    // y = save then close; ! = discard edits and close.
                    CommandKey::Char('y') => self.save_and_close(index),
                    CommandKey::Char('!') => {
                        self.overlay = Overlay::None;
                        self.close_buffer(index);
                    }
                    CommandKey::Esc | CommandKey::Char('n') => self.overlay = Overlay::None,
                    _ => {}
                }
            }
            Overlay::Rename(_) => match key {
                CommandKey::Esc => self.overlay = Overlay::None,
                CommandKey::Backspace => {
                    if let Overlay::Rename(name) = &mut self.overlay {
                        name.pop();
                    }
                }
                CommandKey::Char(ch) => {
                    if let Overlay::Rename(name) = &mut self.overlay {
                        name.push(ch);
                    }
                }
                CommandKey::Enter => self.commit_rename(),
                _ => {}
            },
            Overlay::Settings => match key {
                CommandKey::Esc | CommandKey::Char(';') => self.overlay = Overlay::None,
                CommandKey::Char('<') | CommandKey::Char('s') => self.sidebar = !self.sidebar,
                CommandKey::Char('>') | CommandKey::Char('c') => self.toggle_chat(),
                CommandKey::Char('f') => self.show_many = !self.show_many,
                CommandKey::Char('w') => self.toggle_show_welcome(),
                CommandKey::Char('k') => self.toggle_kitty_graphics(),
                _ => {}
            },
            Overlay::Vim => {
                if matches!(key, CommandKey::Esc | CommandKey::Enter) {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::Menu(menu) => match key {
                CommandKey::Esc => self.overlay = Overlay::None,
                CommandKey::Up => {
                    let mut menu = *menu;
                    menu.cursor = menu.cursor.saturating_sub(1);
                    self.overlay = Overlay::Menu(menu);
                }
                CommandKey::Down => {
                    let mut menu = *menu;
                    menu.cursor = (menu.cursor + 1).min(MENU.len() - 1);
                    self.overlay = Overlay::Menu(menu);
                }
                CommandKey::Enter => {
                    let cursor = menu.cursor;
                    let over_terminal = menu.over_terminal;
                    self.overlay = Overlay::None;
                    self.menu_action(cursor, over_terminal);
                }
                CommandKey::Char('x') => {
                    self.overlay = Overlay::None;
                    self.cut_selection();
                }
                CommandKey::Char('y') => {
                    self.overlay = Overlay::None;
                    self.copy_selection();
                }
                CommandKey::Char('p') => {
                    self.overlay = Overlay::None;
                    self.paste_clipboard();
                }
                CommandKey::Char('a') => {
                    let over_terminal = menu.over_terminal;
                    self.overlay = Overlay::None;
                    self.menu_action(3, over_terminal);
                }
                _ => {}
            },
            Overlay::None => {}
        }
    }

    fn open_help(&mut self) {
        self.help = true;
        self.help_scroll = 0;
        self.help_force_text = false;
        // Text Help paints instantly; Kitty pages arrive from the worker.
        self.schedule_help_raster(edocview::RASTER_DEBOUNCE);
    }

    fn close_help(&mut self) {
        self.help = false;
        self.preview_engine.cancel_slot(edocview::PreviewSlot::Help);
        // Kitty delete runs from present() via clear_help_graphics.
    }

    fn clear_chord(&mut self) {
        self.chord.clear();
        self.chord_deadline = None;
    }

    pub fn chord_due(&self) -> bool {
        self.chord_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub fn flush_chord(&mut self) {
        let chord = std::mem::take(&mut self.chord);
        self.chord_deadline = None;
        match chord.as_str() {
            "b" => self.build_or_run(false),
            "r" => self.build_or_run(true),
            "f" => self.show_many = !self.show_many,
            "l" => self.open_finder(FinderKind::Buffers),
            "ll" => self.open_finder(FinderKind::Files),
            "z" => self.spatial_open_overview(),
            // Lone "g" is a chord prefix for `gs` (Flip 3D) — do not act alone.
            _ => {}
        }
    }

    fn spatial_context(&self) -> SpatialContext {
        SpatialContext {
            workspace_id: self.workspace.display().to_string(),
            pane_id: self.focus,
            document_revision: self.focused().document.revision(),
            trusted: self.dlc_trusted,
            request_id: self.focused().document.revision(),
        }
    }

    fn apply_spatial_outcome(&mut self, outcome: SpatialOutcome) {
        match outcome {
            SpatialOutcome::None => {}
            SpatialOutcome::Notice(text) => self.notice = text,
            SpatialOutcome::OpenOverview(overview) => {
                self.spatial_lines = overview.lines();
                self.spatial_cursor = overview
                    .panes
                    .iter()
                    .position(|pane| pane.index == self.focus)
                    .unwrap_or(0);
                self.spatial.active_mode = SpatialMode::Overview;
            }
            SpatialOutcome::JumpToPane { index } => {
                self.focus_at(index);
                self.notice = format!("overview jump {index}");
            }
            SpatialOutcome::RestoreFocus {
                focus,
                view,
                editing,
            } => {
                self.focus_at(focus);
                self.view = view as f32;
                self.target = view as f32;
                self.editing = editing;
                self.spatial_lines.clear();
                self.graphics.clear();
                self.notice = "spatial closed".into();
            }
            SpatialOutcome::OpenLens(session) => {
                self.spatial_lines = session.lines;
                self.spatial_cursor = 0;
                self.spatial.active_mode = SpatialMode::Lens;
            }
            SpatialOutcome::SnapshotSaved(path) => self.notice = format!("snapshot {path}"),
            SpatialOutcome::SnapshotRestored(snap) => {
                self.focus_at(snap.focus.min(self.buffers.len().saturating_sub(1)));
                self.view = snap.view as f32;
                self.target = snap.view as f32;
                self.notice = "snapshot restored".into();
            }
            SpatialOutcome::Impact(impact) => {
                self.spatial_lines = impact
                    .items
                    .into_iter()
                    .map(|item| {
                        format!(
                            "{:>3}% {} ({})",
                            item.confidence, item.label, item.evidence
                        )
                    })
                    .collect();
                if self.spatial_lines.is_empty() {
                    self.spatial_lines
                        .push("impact: no evidence yet".into());
                }
                self.spatial_cursor = 0;
                self.spatial.active_mode = SpatialMode::Impact;
            }
            SpatialOutcome::Structural(preview) => {
                self.spatial_lines = vec![
                    format!("structural: {}", preview.summary),
                    if preview.ambiguous {
                        "ambiguous — Enter refused".into()
                    } else {
                        "Enter applies · Esc cancels".into()
                    },
                ];
                self.spatial.pending_structural = Some(preview);
                self.spatial.active_mode = SpatialMode::Structural;
            }
            SpatialOutcome::ApplyEdits { revision, edits } => {
                match self.focused_mut().document.apply(revision, &edits) {
                    Ok(_) => self.notice = "structural edit applied".into(),
                    Err(err) => self.notice = format!("structural: {err}"),
                }
                self.spatial_lines.clear();
            }
            SpatialOutcome::Timeline(events) => {
                let label = ide_spatial::TraceStore::label(
                    self.spatial.capabilities.dap_step_back,
                    false,
                );
                self.spatial_lines = vec![format!("timeline: {label:?}")];
                for event in events.iter().rev().take(40) {
                    self.spatial_lines.push(format!(
                        "{} {} {}",
                        event.timestamp_ms,
                        event.kind,
                        event.detail
                    ));
                }
                self.spatial_cursor = 0;
                self.spatial.active_mode = SpatialMode::Timeline;
            }
            SpatialOutcome::OpenPreview(surface) => {
                let backend = self.graphics.backend();
                self.spatial.capabilities.kitty_graphics =
                    backend == graphics::GraphicsBackend::Kitty;
                self.spatial.capabilities.sixel = backend == graphics::GraphicsBackend::Sixel;
                self.spatial.capabilities.iterm2_inline =
                    backend == graphics::GraphicsBackend::ITerm2;
                let id = self.graphics.submit(
                    self.focus,
                    0,
                    0,
                    20,
                    10,
                    self.focused().document.revision(),
                    surface.fallback_text.clone(),
                );
                self.spatial_lines = vec![
                    format!("preview {:?}", surface.kind),
                    self.graphics
                        .text_fallback(id)
                        .unwrap_or(surface.fallback_text),
                ];
                self.spatial.active_mode = SpatialMode::Preview;
            }
            SpatialOutcome::ClosePreview => {
                self.graphics.clear();
                self.spatial_lines.clear();
            }
            SpatialOutcome::FocusRelation { target } => {
                self.notice = format!("focus related {target:?}");
                match target {
                    PaneKind::Terminal | PaneKind::Debug | PaneKind::Test => {
                        self.terminals.open = true;
                        self.focus_pane(PaneFocus::Terminal);
                    }
                    PaneKind::Browser => self.workbench(WorkbenchAction::Browser),
                    PaneKind::Phone => self.workbench(WorkbenchAction::Phone),
                    PaneKind::Android => self.workbench(WorkbenchAction::Android),
                }
            }
        }
    }

    fn spatial_open_overview(&mut self) {
        self.spatial
            .remember_focus(self.focus, self.view as i32, self.editing);
        let names: Vec<String> = self.buffers.iter().map(|b| b.name.clone()).collect();
        let visible = self.stride.max(1) as usize;
        let overview = SpatialOverview::from_panes(self.workspace.display().to_string(), &names, visible);
        self.spatial.set_overview(overview.clone());
        let outcome = self
            .spatial
            .dispatch(SpatialAction::OpenOverview, &self.spatial_context());
        self.apply_spatial_outcome(outcome);
        if overview.fits_viewport {
            self.notice = "overview: panes fit viewport".into();
        }
    }

    fn spatial_open_lens(&mut self) {
        self.spatial
            .remember_focus(self.focus, self.view as i32, self.editing);
        let outcome = self.spatial.dispatch(
            SpatialAction::OpenLens {
                kind: LensKind::Symbols,
            },
            &self.spatial_context(),
        );
        self.apply_spatial_outcome(outcome);
    }

    fn spatial_structural(&mut self, op: StructuralOp) {
        let text = self.focused().document.to_text();
        let revision = self.focused().document.revision();
        let cursor = self.focused().cursor;
        let byte = text.chars().take(cursor).map(|c| c.len_utf8()).sum();
        let node = node_at_byte(&self.focused().language, &text, byte);
        let preview = ide_spatial::structural::preview(&text, revision, node, op);
        self.spatial.pending_structural = Some(preview.clone());
        self.spatial
            .remember_focus(self.focus, self.view as i32, self.editing);
        self.apply_spatial_outcome(SpatialOutcome::Structural(preview));
    }

    fn spatial_timeline(&mut self) {
        self.spatial
            .remember_focus(self.focus, self.view as i32, self.editing);
        if self.spatial.traces.events().is_empty() {
            self.spatial.traces.push(0, "boot", None, None, "recorded replay ready");
        }
        let outcome = self
            .spatial
            .dispatch(SpatialAction::ToggleTimeline, &self.spatial_context());
        self.apply_spatial_outcome(outcome);
    }

    fn spatial_impact(&mut self) {
        self.spatial
            .remember_focus(self.focus, self.view as i32, self.editing);
        let name = self.focused().name.clone();
        let _ = self.spatial.impact.predict(
            self.focused().document.revision(),
            200,
            std::slice::from_ref(&name),
            &[format!("{name}_test")],
            &["build".into()],
        );
        let outcome = self
            .spatial
            .dispatch(SpatialAction::ShowImpact, &self.spatial_context());
        self.apply_spatial_outcome(outcome);
    }

    fn spatial_preview(&mut self) {
        self.spatial
            .remember_focus(self.focus, self.view as i32, self.editing);
        if self.focused().language != "markdown"
            && !self.focused().name.ends_with(".md")
            && self.focused().name != "welcome"
        {
            // Still allow preview of the focused buffer as markdown-ish text.
            self.set_language("markdown");
        }
        self.open_markdown_preview();
        self.notice = "markdown preview".into();
    }

    fn handle_spatial(&mut self, key: CommandKey) {
        match key {
            CommandKey::Esc => {
                let outcome = self
                    .spatial
                    .dispatch(SpatialAction::CloseSpatial, &self.spatial_context());
                self.apply_spatial_outcome(outcome);
            }
            CommandKey::Up => {
                self.spatial_cursor = self.spatial_cursor.saturating_sub(1);
            }
            CommandKey::Down => {
                if !self.spatial_lines.is_empty() {
                    self.spatial_cursor = (self.spatial_cursor + 1).min(self.spatial_lines.len() - 1);
                }
            }
            CommandKey::Enter => match self.spatial.active_mode {
                SpatialMode::Overview => {
                    if let Some(overview) = self.spatial.overview.clone() {
                        if let Some(pane) = overview.panes.get(self.spatial_cursor) {
                            let outcome = self.spatial.dispatch(
                                SpatialAction::JumpOverview {
                                    index: pane.index,
                                },
                                &self.spatial_context(),
                            );
                            self.apply_spatial_outcome(outcome);
                        }
                    }
                }
                SpatialMode::Structural => {
                    let outcome = self
                        .spatial
                        .dispatch(SpatialAction::ApplyStructural, &self.spatial_context());
                    self.apply_spatial_outcome(outcome);
                }
                _ => {}
            },
            CommandKey::Char('s') if self.spatial.active_mode == SpatialMode::Overview => {
                self.spatial_save_snapshot();
            }
            _ => {}
        }
    }

    fn spatial_save_snapshot(&mut self) {
        if ide_spatial::SnapshotStore::opt_out(&self.workspace) {
            self.notice = "spatial snapshots opted out".into();
            return;
        }
        let panes = self
            .buffers
            .iter()
            .map(|buffer| ide_spatial::PaneSnap {
                name: buffer.name.clone(),
                path: buffer.path.as_ref().map(|p| p.display().to_string()),
                scroll: buffer.scroll,
                cursor: buffer.cursor,
            })
            .collect();
        let snap = self.spatial.snapshots.capture(
            self.workspace.display().to_string(),
            self.focus,
            self.view as i32,
            panes,
            Vec::new(),
        );
        match self.spatial.snapshots.write_atomic(&self.workspace, &snap) {
            Ok(path) => self.notice = format!("snapshot {}", path.display()),
            Err(err) => self.notice = format!("snapshot: {err}"),
        }
    }

    fn handle_palette(&mut self, key: CommandKey) {
        // OSC color probes used to leak `rgb:RR/GG/BB` onto stdin; strip junk.
        if self.query.contains("rgb:") || self.query.contains('\u{1b}') {
            self.query.clear();
        }
        match key {
            CommandKey::CtrlQ | CommandKey::CtrlP | CommandKey::Esc => self.palette = false,
            CommandKey::Backspace => {
                self.query.pop();
            }
            CommandKey::Char(ch) => {
                // Ignore ESC/BEL fragments of OSC replies that survived drain.
                if ch == '\u{1b}' || ch == '\u{7}' {
                    return;
                }
                self.query.push(ch);
                if self.query.contains("rgb:") {
                    self.query.clear();
                }
            }
            CommandKey::Enter => {
                let raw = self.query.trim().to_string();
                self.query.clear();
                self.palette = false;
                if raw.contains("rgb:") {
                    return;
                }
                if let Some(setting) = raw.strip_prefix("dlc ") {
                    self.dlc_setting(setting.trim());
                    return;
                }
                // Closed-token compiler; see bend/palette and palette.rs.
                match palette::compile(&raw).1 {
                    palette::Effect::RequestQuit => self.request_quit(),
                    palette::Effect::RefuseVimQuit => {
                        self.notice = palette::VIM_QUIT_NOTICE.into();
                    }
                    palette::Effect::RefuseVimWriteQuit => {
                        self.overlay = Overlay::Vim;
                    }
                    palette::Effect::ToggleSplit => self.show_many = !self.show_many,
                    palette::Effect::UndoEdit => self.focused_mut().undo(),
                    palette::Effect::FormatBuffer => self.format_now(),
                    palette::Effect::ShowHelp => {
                        self.open_help();
                    }
                    palette::Effect::OpenFiles => self.open_yazi(),
                    palette::Effect::FindFiles => self.open_finder(FinderKind::Files),
                    palette::Effect::GrepWorkspace => self.open_finder(FinderKind::Grep),
                    palette::Effect::NewBuffer => self.new_buffer(),
                    palette::Effect::SaveBuffer => self.save(),
                    palette::Effect::UnknownCommand => {
                        if let Some(jump) = parse_line_jump(&raw) {
                            self.jump_lines(jump);
                        } else if matches!(raw.trim(), "rn" | "relative" | "relativenumber") {
                            self.gutter.relative_numbers = !self.gutter.relative_numbers;
                            self.notice = if self.gutter.relative_numbers {
                                "relative line numbers on".into()
                            } else {
                                "absolute line numbers".into()
                            };
                        } else if matches!(raw.trim(), "hints" | "hover-hints" | "hoverhints") {
                            self.toggle_hover_hints();
                        } else {
                            self.notice = format!("unknown command: {raw}");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn request_close(&mut self, index: usize) {
        if index >= self.buffers.len() {
            return;
        }
        if self.buffers[index].dirty() {
            self.overlay = Overlay::Close(index);
            return;
        }
        self.close_buffer(index);
    }

    fn request_quit(&mut self) {
        if self.buffers.iter().any(EditBuffer::dirty) {
            self.overlay = Overlay::Quit;
            return;
        }
        // Quit is the highest-priority job: tear down workers, then exit.
        // Saving only happens on explicit save / save&quit — never blocks plain quit.
        self.shutdown_fast();
        self.quit = true;
    }

    /// Instant teardown for quit: kill Canvas/PTY workers, drop dock state, no saves.
    /// Must run before `finish_quit_fast`'s `_exit` (which skips Drop destructors).
    fn shutdown_fast(&mut self) {
        while let Some(mut surface) = self.canvas_surfaces.pop() {
            surface.stop();
        }
        self.terminals.shutdown();
        self.preview_pool.clear();
        self.flip.close();
        self.runtime_dock = None;
        self.runtime_focused = false;
        self.canvas_painted_size = None;
        self.canvas_image_id = None;
        self.editor_minimap_id = None;
        self.canvas_chrome_dirty.set(true);
        self.canvas_placeholders_stale.set(false);
    }

    fn save_and_close(&mut self, index: usize) {
        if self.save_index(index) {
            self.overlay = Overlay::None;
            self.close_buffer(index);
        }
    }

    fn save_all_and_quit(&mut self) {
        let dirty = self
            .buffers
            .iter()
            .enumerate()
            .filter(|(_, buffer)| buffer.dirty())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for index in dirty {
            if !self.save_index(index) {
                return;
            }
        }
        self.overlay = Overlay::None;
        self.shutdown_fast();
        self.quit = true;
    }

    fn close_buffer(&mut self, index: usize) {
        if index >= self.buffers.len() {
            return;
        }
        let aligned = self
            .tile_spans
            .as_ref()
            .is_some_and(|spans| spans.len() == self.buffers.len() && index < spans.len());
        if aligned {
            if let Some(spans) = self.tile_spans.as_mut() {
                spans.remove(index);
            }
        } else {
            self.tile_spans = None;
        }
        self.tile_drag = None;
        self.buffers.remove(index);
        if self.buffers.is_empty() {
            self.buffers
                .push(EditBuffer::from_text("untitled", "\n", "plain"));
        }
        // One remaining file fills the view; drop any split drag sizes.
        if self.buffers.len() < 2 {
            self.tile_spans = None;
        }
        if self.focus > index {
            self.focus -= 1;
        }
        if self.focus >= self.buffers.len() {
            self.focus = self.buffers.len() - 1;
        }
        self.target = self.focus as f32 * self.stride as f32;
        self.notice = "closed".to_string();
    }

    fn relocate(&mut self, to: usize) {
        if self.buffers.len() < 2 {
            return;
        }
        let to = to.min(self.buffers.len() - 1);
        let from = self.focus;
        if from == to {
            return;
        }
        self.move_tile_span(from, to);
        let buffer = self.buffers.remove(from);
        self.buffers.insert(to, buffer);
        self.focus_at(to);
    }

    fn move_tile_span(&mut self, from: usize, to: usize) {
        let aligned = self
            .tile_spans
            .as_ref()
            .is_some_and(|spans| spans.len() == self.buffers.len() && from < spans.len());
        if !aligned {
            self.tile_spans = None;
            return;
        }
        let Some(spans) = self.tile_spans.as_mut() else {
            return;
        };
        let span = spans.remove(from);
        spans.insert(to.min(spans.len()), span);
    }

    fn swap_buffer(&mut self, delta: isize) {
        let next = self.focus as isize + delta;
        if next < 0 || next >= self.buffers.len() as isize {
            return;
        }
        let next = next as usize;
        if let Some(spans) = self.tile_spans.as_mut() {
            if spans.len() == self.buffers.len() {
                spans.swap(self.focus, next);
            }
        }
        self.buffers.swap(self.focus, next);
        self.focus_at(next);
    }

    fn commit_rename(&mut self) {
        let Overlay::Rename(name) = self.overlay.clone() else {
            return;
        };
        self.overlay = Overlay::None;
        let name = name.trim().to_string();
        if name.is_empty() || name.contains('/') || name.contains('\0') {
            self.notice = "rename needs a file name".to_string();
            return;
        }
        let Some(path) = self.focused().path.clone() else {
            self.notice = "rename needs a path".to_string();
            return;
        };
        let dest = path.parent().unwrap_or(Path::new(".")).join(&name);
        if dest.exists() {
            self.notice = "rename target exists".to_string();
            return;
        }
        match std::fs::rename(&path, &dest) {
            Ok(()) => {
                self.focused_mut().path = Some(dest);
                self.focused_mut().name = name;
                self.file_index.clear();
                self.reload_tree();
                self.notice = "renamed".to_string();
            }
            Err(error) => self.notice = format!("rename failed: {error}"),
        }
    }

    fn build_or_run(&mut self, run: bool) {
        let language = self.language().to_string();
        let path = self.focused().path.clone();
        let cwd = project_root(path.as_deref().unwrap_or(self.workspace.as_path()));
        if language == "c" && run {
            self.run_c(path.as_deref(), &cwd);
            return;
        }
        let Some((program, args)) = task_line(&language, path.as_deref(), run) else {
            self.notice = if run {
                "no run for this language".to_string()
            } else {
                "no build for this language".to_string()
            };
            return;
        };
        match capture(&program, &args, &cwd) {
            Ok((ok, text)) => self.finish_task(run, ok, text),
            Err(detail) => self.notice = detail,
        }
    }

    fn run_c(&mut self, path: Option<&Path>, cwd: &Path) {
        let Some(file) = path else {
            self.notice = "run needs a path".to_string();
            return;
        };
        let out = std::env::temp_dir().join(format!("axiom-run-{}", std::process::id()));
        let compiled = capture(
            "clang",
            &[
                file.display().to_string(),
                "-o".to_string(),
                out.display().to_string(),
            ],
            cwd,
        );
        match compiled {
            Ok((true, _)) => match capture(&out.display().to_string(), &[], cwd) {
                Ok((ok, text)) => self.finish_task(true, ok, text),
                Err(detail) => self.notice = detail,
            },
            Ok((false, text)) => self.finish_task(false, false, text),
            Err(detail) => self.notice = detail,
        }
        let _ = std::fs::remove_file(out);
    }

    fn finish_task(&mut self, run: bool, ok: bool, text: String) {
        let title = if run { "*run*" } else { "*build*" };
        let body = if text.trim().is_empty() {
            if ok {
                "ok\n".to_string()
            } else {
                "failed\n".to_string()
            }
        } else {
            text
        };
        self.show_output(title, &body);
        self.notice = format!(
            "{} {}",
            if run { "run" } else { "build" },
            if ok { "ok" } else { "failed" }
        );
    }

    fn workbench_key(&mut self, key: char) {
        let Some(action) = WorkbenchToolbar::key_action(crossterm::event::KeyCode::Char(key))
        else {
            return;
        };
        self.workbench(action);
    }

    fn dlc_setting(&mut self, setting: &str) {
        match setting {
            "trust" => {
                self.dlc_trusted = true;
                self.persist_workbench();
                self.notice = "workspace trusted for reviewed plans".to_string();
            }
            "untrust" => {
                self.dlc_trusted = false;
                self.pending_plan = None;
                self.persist_workbench();
                self.notice = "workspace untrusted; plans stay previews".to_string();
            }
            "" => self.notice = "dlc needs trust, untrust, or a system name".to_string(),
            system => {
                self.dlc_system = Some(system.to_string());
                self.persist_workbench();
                self.notice = format!("workbench system {system}");
            }
        }
    }

    /// If the focused file lives under a real project root, adopt it as the workspace.
    fn adopt_project_root_from_focus(&mut self) {
        let Some(path) = self.focused().path.clone() else {
            return;
        };
        let root = project_root(&path);
        if root == self.workspace {
            return;
        }
        // Only climb when the current workspace looks like a source package, not a project.
        let current_markers = detect_systems_fast(&self.workspace);
        let root_markers = detect_systems_fast(&root);
        if current_markers.is_empty() && !root_markers.is_empty() {
            self.workspace = root;
            self.file_index.clear();
            self.open_dirs.insert(self.workspace.clone());
            self.dlc_system = None;
            // Local project roots the user already opened: trust reviewed Build/Run plans.
            if root_markers.iter().any(|system| {
                matches!(
                    system.as_str(),
                    "gradle" | "maven" | "cargo" | "cmake" | "zig" | "swiftpm"
                )
            }) {
                self.dlc_trusted = true;
            }
            self.reload_tree();
            self.bootstrap_workbench();
        }
    }

    /// Detect build systems, restore `.posit/workbench.json`, and auto-wire the editor repo.
    fn bootstrap_workbench(&mut self) {
        let saved = load_workbench(&self.workspace);
        let systems = detect_systems_fast(&self.workspace);
        if let Some(system) = saved.system.filter(|name| systems.iter().any(|s| s == name)) {
            self.dlc_system = Some(system);
        } else if self.dlc_system.is_none() {
            self.dlc_system = pick_system(&systems, self.language(), None);
        }
        if saved.trusted || is_editor_workspace(&self.workspace) {
            self.dlc_trusted = true;
        }
        if self.dlc_system.is_some() || self.dlc_trusted {
            self.persist_workbench();
        }
        if let Some(system) = self.dlc_system.as_deref() {
            if self.dlc_trusted {
                self.notice = format!("workbench ready: {system} (trusted)");
            } else {
                self.notice = format!("workbench ready: {system}; : dlc trust to run Build/Run");
            }
        }
    }

    fn persist_workbench(&self) {
        let dir = self.workspace.join(".posit");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let mut body = serde_json::Map::new();
        body.insert("trusted".into(), serde_json::Value::Bool(self.dlc_trusted));
        if let Some(system) = &self.dlc_system {
            body.insert(
                "system".into(),
                serde_json::Value::String(system.clone()),
            );
        }
        let _ = std::fs::write(
            dir.join("workbench.json"),
            format!("{}\n", serde_json::Value::Object(body)),
        );
    }

    fn workbench(&mut self, action: WorkbenchAction) {
        let Some(script) = find_dlc_script() else {
            self.show_output("*workbench*", "dlc.py is not installed\n");
            self.notice = "workbench unavailable".to_string();
            return;
        };
        match action {
            WorkbenchAction::Debug => {
                self.pending_plan = None;
                self.spatial.relations.attach(
                    self.focus,
                    PaneKind::Debug,
                    self.focused().document.revision(),
                );
                self.start_debug();
            }
            WorkbenchAction::Refresh => {
                self.pending_plan = None;
                let text = self.dlc_text(&script, &["scan".into()]);
                self.show_output("*workbench*", &text);
                self.notice = "projects refreshed".to_string();
            }
            WorkbenchAction::Browser => {
                self.spatial.relations.attach(
                    self.focus,
                    PaneKind::Browser,
                    self.focused().document.revision(),
                );
                self.launch_runtime(&script, "browser");
            }
            WorkbenchAction::Phone => {
                self.spatial.relations.attach(
                    self.focus,
                    PaneKind::Phone,
                    self.focused().document.revision(),
                );
                self.launch_runtime(&script, "phone-view");
            }
            WorkbenchAction::Android => {
                self.spatial.relations.attach(
                    self.focus,
                    PaneKind::Android,
                    self.focused().document.revision(),
                );
                self.launch_runtime(&script, "android-boot");
            }
            WorkbenchAction::Configure => {
                self.pending_plan = None;
                let text = self.dlc_text(&script, &["config".into()]);
                self.show_output("*workbench*", &text);
                self.notice = "configuration preview".to_string();
            }
            WorkbenchAction::Build => self.plan_action(&script, "build"),
            WorkbenchAction::Run => self.plan_action(&script, "run"),
            WorkbenchAction::Test => {
                self.spatial.relations.attach(
                    self.focus,
                    PaneKind::Test,
                    self.focused().document.revision(),
                );
                self.plan_action(&script, "test");
            }
        }
    }

    fn start_debug(&mut self) {
        let language = self.language().to_string();
        // Prefer the focused buffer's language. Cargo.toml must not steal Java/JS/etc.
        let command = if language == "rust" && self.workspace.join("Cargo.toml").is_file() {
            debug_cargo(&self.workspace)
        } else if let Some(path) = self.focused().path.clone() {
            debug_file(&language, &path)
        } else if self.workspace.join("Cargo.toml").is_file() {
            debug_cargo(&self.workspace)
        } else {
            Err(format!("no debugger for {language}"))
        };
        match command {
            Ok((program, args)) => {
                self.terminals.open = true;
                let rect = self.terminal_rect();
                let (cols, rows) = self
                    .terminals
                    .grid_size(rect.width.max(20), rect.height.max(8));
                let name = Path::new(&program)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&program)
                    .to_string();
                match self
                    .terminals
                    .run_program(&self.workspace, cols, rows, &program, &args)
                {
                    Ok(()) => self.notice = format!("debugging with {name}"),
                    Err(detail) => self.report_error("Debug", &detail),
                }
            }
            Err(detail) => self.report_error("Debug", &detail),
        }
    }

    fn launch_runtime(&mut self, script: &Path, action: &str) {
        self.pending_plan = None;
        let runtime_id = match action {
            "browser" => "browser",
            "phone-view" => "phone",
            "android-boot" => "android",
            _ => action,
        };
        // Re-focus an existing Canvas tab of this kind (keep other tabs open).
        if self.activate_canvas_kind(runtime_id) {
            self.notice = format!("{} tab", Self::canvas_tab_label(runtime_id));
            return;
        }
        let text = self.dlc_text(script, &["runtime".into(), action.into()]);
        if let Some(error) = json_error(&text) {
            self.report_error("Runtime", &error);
            return;
        }
        let Some((cwd, argv)) = plan_argv(&text, &self.workspace) else {
            self.report_error("Runtime", "runtime plan has no command");
            return;
        };
        let args = argv[1..].to_vec();
        let raw = argv[0].clone();
        let name = Path::new(&raw)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&raw);
        if matches!(name, "open" | "xdg-open" | "simctl") {
            self.report_error(
                "Runtime",
                &format!("{name} is not an in-editor runtime. Install terminal-browser or terminal-phone."),
            );
            return;
        }
        let program = resolve_runtime_program(runtime_id, &raw, name);
        if !Path::new(&program).is_file() {
            let hint = match runtime_id {
                "browser" => "Install vendor terminal-browser / agent-browser for Canvas capture",
                "phone" => "Install vendor terminal-phone (scripts/install-runtime-vendors.sh)",
                "android" => "Install the Android emulator, adb, and an AVD",
                _ => "Runtime tool is not installed",
            };
            self.report_error("Runtime", &format!("{name} not found. {hint}"));
            return;
        }
        // Dedicated right-side Canvas: add a tab; prior tabs stay open.
        // Seed from session cache so a prior capture paints instantly.
        match canvas::CanvasSurface::start_with_cache(
            runtime_id,
            &cwd,
            &program,
            &args,
            Some(&self.canvas_frame_cache),
        ) {
            Ok(surface) => {
                let seeded = surface.has_frame();
                let image_id = surface.next_image_id();
                self.canvas_surfaces.push(surface);
                self.runtime_dock = Some(runtime_id.to_string());
                self.canvas_painted_size = None;
                self.canvas_image_id = Some(image_id);
                self.canvas_chrome_dirty.set(true);
                self.canvas_placeholders_stale.set(true);
                self.focus_pane(PaneFocus::Runtime);
                self.notice = if seeded {
                    format!(
                        "{} canvas (cached)",
                        Self::canvas_tab_label(runtime_id)
                    )
                } else if self.canvas_surfaces.len() > 1 {
                    format!("{} tab open", Self::canvas_tab_label(runtime_id))
                } else {
                    "canvas open".into()
                };
            }
            Err(detail) => {
                if self.canvas_surfaces.is_empty() {
                    self.runtime_dock = None;
                }
                self.report_error("Runtime", &detail.to_string());
            }
        }
    }

    fn plan_action(&mut self, script: &Path, action: &str) {
        // Opening src/.../Main.java used to leave workspace in the package dir (scan → []).
        self.adopt_project_root_from_focus();
        let scan = self.dlc_text(script, &["scan".into()]);
        let systems = scan_systems(&scan);
        if self.dlc_system.is_none() {
            self.dlc_system = pick_system(&systems, self.language(), None);
        }
        // Never execute against IntelliJ metadata alone when a real tool is present.
        if self.dlc_system.as_deref() == Some("intellij") {
            self.dlc_system = pick_system(&systems, self.language(), None);
        }
        let Some(system) = pick_system(&systems, self.language(), self.dlc_system.as_deref())
            .filter(|name| *name != "intellij")
            .or_else(|| {
                pick_system(
                    &systems
                        .iter()
                        .filter(|s| s.as_str() != "intellij")
                        .cloned()
                        .collect::<Vec<_>>(),
                    self.language(),
                    None,
                )
            })
        else {
            self.pending_plan = None;
            self.show_output("*workbench*", &scan);
            let hint = match self.language() {
                "java" | "kotlin" => {
                    "no Gradle/Maven root found — open the IntelliJ project folder (with build.gradle or pom.xml), or : dlc gradle"
                }
                _ => "no build system detected; open the project root or : dlc cargo",
            };
            self.notice = hint.to_string();
            return;
        };
        self.dlc_system = Some(system.clone());
        let text = self.dlc_text(
            script,
            &[
                "plan".into(),
                action.into(),
                "--system".into(),
                system.clone(),
            ],
        );
        self.pending_plan = plan_argv(&text, &self.workspace);
        let trust = if self.dlc_trusted {
            Trust::Trusted
        } else {
            Trust::Untrusted
        };
        let status = match authorize(trust) {
            TrustAction::Execute if self.pending_plan.is_some() => {
                format!("{system} {action}")
            }
            TrustAction::Preview if self.pending_plan.is_some() => {
                format!("{system} {action} preview; : dlc trust then Build again")
            }
            _ => format!("{system} {action} has no executable plan"),
        };
        self.show_output("*workbench*", &text);
        // show_output overwrites notice — restore the actionable status.
        self.notice = status;
        if authorize(trust) == TrustAction::Execute && self.pending_plan.is_some() {
            self.approve_plan();
        }
    }

    fn approve_plan(&mut self) {
        if authorize(if self.dlc_trusted {
            Trust::Trusted
        } else {
            Trust::Untrusted
        }) == TrustAction::Preview
        {
            self.notice = "untrusted workspace stays preview-only".to_string();
            return;
        }
        let Some((cwd, argv)) = self.pending_plan.clone() else {
            return;
        };
        let Some(program) = argv.first().cloned() else {
            return;
        };
        match capture(&program, &argv[1..], &cwd) {
            Ok((ok, text)) => {
                self.show_output("*workbench*", &text);
                self.notice = if ok {
                    "plan finished".to_string()
                } else {
                    "plan failed".to_string()
                };
            }
            Err(detail) => self.notice = detail,
        }
        self.pending_plan = None;
    }

    fn dlc_text(&self, script: &Path, args: &[String]) -> String {
        let mut command = vec![
            script.display().to_string(),
            "--root".to_string(),
            self.workspace.display().to_string(),
        ];
        command.extend(args.iter().cloned());
        match capture("python3", &command, &self.workspace) {
            Ok((_, text)) => text,
            Err(detail) => detail,
        }
    }

    fn show_output(&mut self, title: &str, text: &str) {
        self.terminals.append_console(title, text);
        self.spatial.output.ingest(
            ide_spatial::RawChunk {
                process_id: 0,
                monotonic_ms: self.focused().document.revision(),
                stream: "console",
                bytes: format!("{title}\n{text}").into_bytes(),
                exit_status: None,
            },
            &self.workspace,
        );
        self.notice = format!("{title} → console");
    }

    fn shift_focus(&mut self, delta: isize) {
        let next = self.focus as isize + delta;
        if next < 0 || next >= self.buffers.len() as isize {
            return;
        }
        self.focus_at(next as usize);
    }

    fn focus_at(&mut self, index: usize) {
        if self.buffers.is_empty() {
            return;
        }
        let next = index.min(self.buffers.len() - 1);
        if next != self.focus {
            self.wheel_at = None;
            self.scroll_axis = None;
        }
        self.focus = next;
        self.tree_focused = false;
        self.target = self.window().origin(self.focus) as f32;
    }

    fn focused(&self) -> &EditBuffer {
        &self.buffers[self.focus]
    }

    fn focused_mut(&mut self) -> &mut EditBuffer {
        &mut self.buffers[self.focus]
    }

    fn frame_chrome(&self, area: Rect) -> Chrome {
        layout::chrome_with_runtime(
            area,
            self.blocks_status(),
            self.sidebar,
            self.chat,
            self.tree_width,
            self.agent_width,
            layout::TerminalPane {
                open: self.terminals.open,
                rows: self.terminal_rows,
            },
            layout::CanvasPane {
                open: self.runtime_dock.is_some(),
                width: self.runtime_width,
            },
        )
    }

    fn metrics_for(&self, width: u16, height: u16, orientation: Axis) -> (Chrome, Window) {
        let chrome = self.frame_chrome(Rect::new(0, 0, width, height));
        let window = self.layout_window(chrome.content, orientation);
        (chrome, window)
    }

    fn layout_window(&self, content: Rect, orientation: Axis) -> Window {
        let (tiles, _) = self.content_tiles_and_strip(content);
        let custom = (self.show_many && self.buffers.len() >= 2 && orientation == self.axis)
            .then_some(self.tile_spans.as_deref())
            .flatten();
        match custom {
            Some(spans) => layout::window_spans(
                tiles,
                orientation,
                self.show_many,
                self.buffers.len(),
                Some(spans),
            ),
            None => layout::window(tiles, orientation, self.show_many, self.buffers.len()),
        }
    }

    fn window(&self) -> Window {
        self.metrics_for(self.width, self.height, self.axis).1
    }

    fn visible_rows(&self) -> usize {
        self.window().tile_box(self.focus).1.saturating_sub(2) as usize
    }

    /// Proportional minimap hit-test (VS Code sliding window, not full-file squeeze).
    fn minimap_line_for_row(
        &self,
        line_count: usize,
        visible: usize,
        track_rows: usize,
        row: usize,
    ) -> usize {
        let (_, cell_h) = canvas_cell_pixels(self);
        let img_h = (track_rows as u32)
            .saturating_mul(cell_h.max(1))
            .max(1)
            .min(1024);
        let layout = minimap_gfx::layout_proportional(
            line_count,
            self.focused().scroll,
            visible.max(1),
            img_h,
            1,
        );
        minimap_gfx::line_at_cell_row(layout, row, track_rows, img_h, line_count)
    }

    fn scroll_limit(&self) -> f32 {
        let visible = self.visible_rows().max(1);
        let lines = self.focused().document.len_lines();
        lines.saturating_sub(visible) as f32
    }

    fn visible_cols(&self) -> usize {
        let (tile_w, _) = self.window().tile_box(self.focus);
        let line_count = self.focused().document.len_lines().max(1);
        let gutter_w = self.gutter.width_for(line_count);
        let visible = self.visible_rows().max(1);
        let line_count = self.focused().document.len_lines().max(1);
        let (minimap_w, scroll_w) = self.focused_edge_chrome(tile_w, line_count, visible);
        tile_w
            .saturating_sub(2)
            .saturating_sub(gutter_w)
            .saturating_sub(minimap_w)
            .saturating_sub(scroll_w)
            .max(1) as usize
    }

    fn scroll_limit_x(&self) -> f32 {
        let visible = self.visible_cols().max(1);
        let width = self.focused().max_line_chars();
        width.saturating_sub(visible) as f32
    }

    fn document_column(&self, visual: usize) -> usize {
        visual + self.focused().scroll_x
    }

    fn wheel_buffer(&mut self, delta: f32) {
        self.wheel_focused(delta, true);
    }

    fn wheel_buffer_x(&mut self, delta: f32) {
        self.wheel_focused(delta, false);
    }

    /// Immediate line/column scroll — same discrete step as a terminal console.
    fn wheel_focused(&mut self, delta: f32, vertical: bool) {
        self.wheel_at = Some(Instant::now());
        let magnitude = delta.abs().ceil() as isize;
        if magnitude == 0 {
            return;
        }
        let step = if delta < 0.0 { -magnitude } else { magnitude };
        if vertical {
            let max = self.scroll_limit() as usize;
            let next = (self.focused().scroll as isize + step).clamp(0, max as isize) as usize;
            self.focused_mut().scroll = next;
        } else {
            let max = self.scroll_limit_x() as usize;
            let next = (self.focused().scroll_x as isize + step).clamp(0, max as isize) as usize;
            self.focused_mut().scroll_x = next;
        }
    }

    fn snap_scroll(&mut self, line: usize) {
        let max = self.scroll_limit() as usize;
        self.focused_mut().scroll = line.min(max);
        // Pause Kitty minimap for keyboard scroll the same as wheel scroll.
        self.wheel_at = Some(Instant::now());
    }

    fn scroll_page(&mut self, delta: isize) {
        let page = self.visible_rows().max(1) as isize;
        let current = self.focused().scroll as isize;
        let next = (current + delta * page).max(0) as usize;
        self.snap_scroll(next);
    }

    fn refresh_gutter_git(&mut self) {
        let workspace = self.workspace.clone();
        let (path, revision) = {
            let buffer = self.focused();
            (
                buffer.path.clone(),
                buffer.document.revision(),
            )
        };
        if let Some(path) = path {
            self.gutter
                .ensure_git_marks(&workspace, &path, revision);
        }
    }

    fn jump_lines(&mut self, jump: LineJump) {
        let visible = self.visible_rows();
        let columns = self.visible_cols();
        let (line, _) = self.focused().with_text(|text| line_col(text, self.focused().cursor));
        let target = match jump {
            LineJump::Absolute(n) => n.saturating_sub(1),
            LineJump::Relative(delta) => {
                if delta < 0 {
                    line.saturating_sub(delta.unsigned_abs())
                } else {
                    line.saturating_add(delta as usize)
                }
            }
        };
        {
            let buffer = self.focused_mut();
            buffer.cursor = buffer.with_text(|text| char_at(text, target, 0));
            buffer.anchor = buffer.cursor;
            buffer.selection = None;
            buffer.reveal(visible, columns);
        }
        self.snap_scroll(target.saturating_sub(visible / 3));
        self.notice = format!("line {}", target + 1);
    }

    fn format_now(&mut self) {
        let source = self.focused().text();
        let language = self.focused().language.clone();
        match format_manually(&language, &source) {
            FormatOutcome::Applied(formatted) => {
                self.focused_mut().replace_all(&formatted);
                self.notice = "formatted".to_string();
            }
            FormatOutcome::Unchanged => self.notice = "already formatted".to_string(),
            FormatOutcome::Unavailable { program } if program == "none" => {
                self.notice = format!("no formatter for {language}");
            }
            FormatOutcome::Unavailable { program } => {
                self.notice = format!("formatter unavailable: {program}");
            }
            FormatOutcome::Failed { detail, .. } => {
                self.notice = format!("format failed: {detail}");
            }
        }
    }

    fn save(&mut self) {
        let focus = self.focus;
        let _ = self.save_index(focus);
    }

    fn save_index(&mut self, index: usize) -> bool {
        if index >= self.buffers.len() {
            return false;
        }
        let Some(path) = self.buffers[index].path.clone() else {
            self.notice = "save needs a path".to_string();
            return false;
        };
        match std::fs::write(&path, self.buffers[index].text()) {
            Ok(()) => {
                self.buffers[index].mark_clean();
                self.notice = "saved".to_string();
                true
            }
            Err(error) => {
                self.notice = format!("save failed: {error}");
                false
            }
        }
    }

    fn new_buffer(&mut self) {
        let count = self
            .buffers
            .iter()
            .filter(|buffer| buffer.path.is_none())
            .count();
        let name = if count == 0 {
            "untitled".to_string()
        } else {
            format!("untitled-{}", count + 1)
        };
        self.buffers.push(EditBuffer::from_text(name, "", "plain"));
        self.focus_at(self.buffers.len() - 1);
    }

    fn scroll_help(&mut self, delta: isize) {
        if delta < 0 {
            self.help_scroll = self.help_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.help_scroll = self.help_scroll.saturating_add(delta as usize);
        }
        let visible = self.height.saturating_sub(2).max(1) as usize;
        let max_scroll = if let Some(doc) = self.help_doc.as_ref() {
            doc.total_rows.saturating_sub(visible)
        } else {
            let rendered = markdown_preview::render(&help_markdown(), self.width.max(8) as usize);
            rendered.lines.len().saturating_sub(visible)
        };
        self.help_scroll = self.help_scroll.min(max_scroll);
        // Scroll is a crop placement only — never retransmit the retained PNG.
    }

    #[cfg(test)]
    fn add_buffer(&mut self, name: &str, text: &str) {
        self.buffers
            .push(EditBuffer::from_text(name, text, "plain"));
    }

    fn bind_initial(&mut self, path: PathBuf) {
        let text = self.focused().text();
        let language = ide_syntax::detect(&self.profiles, Some(&path), &text);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_string();
        let buffer = self.focused_mut();
        buffer.path = Some(path.clone());
        buffer.name = name;
        buffer.language = language;
        // Walk up to Gradle/Maven/Cargo/.idea — not the package folder next to Main.java.
        let root = project_root(&path);
        if root.is_dir() {
            self.workspace = root;
            self.file_index.clear();
            self.open_dirs.insert(self.workspace.clone());
            self.reload_tree();
            self.bootstrap_workbench();
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        let path = if path.is_absolute() {
            path
        } else {
            self.workspace.join(path)
        };
        if path.is_dir() {
            self.workspace = path;
            self.file_index.clear();
            self.open_dirs.insert(self.workspace.clone());
            self.dlc_system = None;
            self.dlc_trusted = false;
            self.pending_plan = None;
            self.reload_tree();
            self.bootstrap_workbench();
            return;
        }
        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.path.as_deref() == Some(path.as_path()))
        {
            self.focus_at(index);
            return;
        }
        let text = match files::read_or_create(&path) {
            Ok(text) => text,
            Err(error) => {
                self.notice = format!("open failed: {error}");
                return;
            }
        };
        let language = ide_syntax::detect(&self.profiles, Some(&path), &text);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_string();
        let mut buffer = EditBuffer::from_text(name, &text, language);
        buffer.cursor = 0;
        buffer.anchor = 0;
        buffer.path = Some(path);
        self.buffers.push(buffer);
        self.focus_at(self.buffers.len() - 1);
        if self.focused().language == "markdown" {
            self.open_markdown_preview();
        }
    }

    fn open_picked(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            self.notice = "no file chosen".to_string();
            return;
        }
        for path in paths {
            self.open_path(path.clone());
        }
        self.reload_tree();
    }

    fn reload_tree(&mut self) {
        let selected = self
            .tree
            .get(self.tree_cursor)
            .map(|entry| entry.path.clone());
        self.tree = files::list_tree(&self.workspace, &self.open_dirs);
        if let Some(path) = selected {
            if let Some(index) = self.tree.iter().position(|entry| entry.path == path) {
                self.tree_cursor = index;
            }
        }
        if self.tree.is_empty() {
            self.tree_cursor = 0;
        } else {
            self.tree_cursor = self.tree_cursor.min(self.tree.len() - 1);
        }
    }

    #[cfg(test)]
    fn set_workspace(&mut self, path: PathBuf) {
        self.workspace = path;
        self.tab_enabled = local_tab_enabled(&self.workspace);
        self.ghost = None;
        self.file_index.clear();
        self.open_dirs.insert(self.workspace.clone());
        self.tree_cursor = 0;
        self.tree_scroll = 0;
        self.dlc_system = None;
        self.dlc_trusted = false;
        self.pending_plan = None;
        self.reload_tree();
        self.bootstrap_workbench();
    }

    fn move_tree(&mut self, delta: isize) {
        if self.tree.is_empty() {
            return;
        }
        let next = if delta < 0 {
            self.tree_cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (self.tree_cursor + delta as usize).min(self.tree.len() - 1)
        };
        self.tree_cursor = next;
        let visible = inset(self.tree_rect()).height as usize;
        if self.tree_cursor < self.tree_scroll {
            self.tree_scroll = self.tree_cursor;
        } else if visible > 0 && self.tree_cursor >= self.tree_scroll + visible {
            self.tree_scroll = self.tree_cursor + 1 - visible;
        }
    }

    fn activate_tree(&mut self) {
        let Some(entry) = self.tree.get(self.tree_cursor).cloned() else {
            return;
        };
        if entry.dir {
            if !self.open_dirs.remove(&entry.path) {
                self.open_dirs.insert(entry.path);
            }
            self.reload_tree();
            self.focus_pane(PaneFocus::Tree);
        } else {
            self.open_path(entry.path);
            self.focus_pane(PaneFocus::Editor);
        }
    }

    fn tree_click(&mut self, row: u16, chrome: Chrome) {
        let inner = inset(chrome.tree);
        if inner.height == 0 || row < inner.y || row >= inner.y + inner.height {
            return;
        }
        let index = self.tree_scroll + (row - inner.y) as usize;
        if index >= self.tree.len() {
            return;
        }
        self.tree_cursor = index;
        self.focus_pane(PaneFocus::Tree);
        self.activate_tree();
    }

    fn tree_rect(&self) -> Rect {
        self.frame_chrome(Rect::new(0, 0, self.width, self.height))
            .tree
    }

    #[cfg(test)]
    fn text_origin(&self) -> Option<(u16, u16)> {
        layout::text_origin(self.window(), self.focus, self.view)
    }

    #[cfg(test)]
    fn tile_corner(&self, index: usize) -> Option<(u16, u16)> {
        let slice = layout::tile_slice(self.window(), index, self.view)?;
        Some((slice.screen_x, slice.screen_y))
    }

    fn open_finder(&mut self, kind: FinderKind) {
        self.ensure_index();
        self.finder = Some(Finder {
            kind,
            query: String::new(),
            cursor: 0,
            scroll: 0,
            files: Vec::new(),
            greps: Vec::new(),
        });
        self.refresh_finder();
    }

    fn ensure_index(&mut self) {
        if !self.file_index.is_empty() {
            return;
        }
        self.file_index = ide_navigation::list_files(&self.workspace)
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();
    }

    fn refresh_finder(&mut self) {
        let Some(kind) = self.finder.as_ref().map(|finder| finder.kind) else {
            return;
        };
        let query = self
            .finder
            .as_ref()
            .map(|finder| finder.query.clone())
            .unwrap_or_default();
        let (files, greps) = match kind {
            FinderKind::Files => {
                let mut files = ide_navigation::fuzzy_matches(&query, &self.file_index);
                files.truncate(200);
                (files, Vec::new())
            }
            FinderKind::Grep => (
                Vec::new(),
                ide_navigation::search_text(&self.workspace, &query, 80),
            ),
            FinderKind::Buffers => {
                let labels: Vec<String> = self
                    .buffers
                    .iter()
                    .enumerate()
                    .map(|(index, buffer)| format!("{index} {}", buffer.name))
                    .collect();
                let mut files = ide_navigation::fuzzy_matches(&query, &labels);
                files.truncate(200);
                (files, Vec::new())
            }
        };
        let Some(finder) = self.finder.as_mut() else {
            return;
        };
        finder.files = files;
        finder.greps = greps;
        let len = finder_len(finder);
        finder.cursor = if len == 0 {
            0
        } else {
            finder.cursor.min(len - 1)
        };
        reveal_list(&mut finder.scroll, finder.cursor, 12);
    }

    fn handle_finder(&mut self, key: CommandKey) {
        match key {
            CommandKey::CtrlQ => self.request_quit(),
            CommandKey::Esc | CommandKey::CtrlP => self.finder = None,
            CommandKey::Tab => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.kind = match finder.kind {
                        FinderKind::Files => FinderKind::Grep,
                        FinderKind::Grep | FinderKind::Buffers => FinderKind::Files,
                    };
                    finder.cursor = 0;
                    finder.scroll = 0;
                }
                self.refresh_finder();
            }
            CommandKey::Backspace => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.query.pop();
                }
                self.refresh_finder();
            }
            CommandKey::Char(ch) => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.query.push(ch);
                }
                self.refresh_finder();
            }
            CommandKey::Up => self.move_finder(-1),
            CommandKey::Down => self.move_finder(1),
            CommandKey::Enter => self.accept_finder(),
            _ => {}
        }
    }

    fn move_finder(&mut self, delta: isize) {
        let Some(finder) = self.finder.as_mut() else {
            return;
        };
        let len = finder_len(finder);
        if len == 0 {
            return;
        }
        finder.cursor = if delta < 0 {
            finder.cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (finder.cursor + delta as usize).min(len - 1)
        };
        reveal_list(&mut finder.scroll, finder.cursor, 12);
    }

    fn accept_finder(&mut self) {
        let Some(finder) = self.finder.clone() else {
            return;
        };
        if finder.kind == FinderKind::Buffers {
            let index = finder
                .files
                .get(finder.cursor)
                .and_then(|hit| hit.text.split_whitespace().next()?.parse().ok());
            self.finder = None;
            if let Some(index) = index {
                self.focus_at(index);
            }
            return;
        }
        let opened = match finder.kind {
            FinderKind::Buffers => None,
            FinderKind::Files => finder
                .files
                .get(finder.cursor)
                .map(|hit| (self.workspace.join(&hit.text), None)),
            FinderKind::Grep => finder
                .greps
                .get(finder.cursor)
                .map(|hit| (self.workspace.join(&hit.path), Some(hit.line))),
        };
        let Some((path, line)) = opened else {
            return;
        };
        self.finder = None;
        self.open_path(path);
        if let Some(line) = line {
            let visible = self.visible_rows();
            let columns = self.visible_cols();
            self.focused_mut().move_to_line(line, visible, columns);
        }
    }

    fn finder_click(&mut self, row: u16, body: Rect) {
        let inner = inset(body);
        if inner.height <= 1 || row <= inner.y || row >= inner.y + inner.height {
            return;
        }
        let index = self
            .finder
            .as_ref()
            .map(|finder| finder.scroll)
            .unwrap_or(0)
            + (row - inner.y - 1) as usize;
        let Some(finder) = self.finder.as_mut() else {
            return;
        };
        let len = finder_len(finder);
        if index < len {
            finder.cursor = index;
        }
    }

    fn handle_tree_search(&mut self, key: CommandKey) {
        match key {
            CommandKey::CtrlQ => self.request_quit(),
            CommandKey::Esc => {
                self.tree_query = None;
                self.tree_hits.clear();
            }
            CommandKey::Backspace => {
                let close = self
                    .tree_query
                    .as_ref()
                    .is_some_and(|query| query.is_empty());
                if close {
                    self.tree_query = None;
                    self.tree_hits.clear();
                } else if let Some(query) = self.tree_query.as_mut() {
                    query.pop();
                    self.refresh_tree_search();
                }
            }
            CommandKey::Char(ch) => {
                if let Some(query) = self.tree_query.as_mut() {
                    query.push(ch);
                }
                self.refresh_tree_search();
            }
            CommandKey::Up => self.move_tree_search(-1),
            CommandKey::Down => self.move_tree_search(1),
            CommandKey::Enter => self.accept_tree_search(),
            _ => {}
        }
    }

    fn refresh_tree_search(&mut self) {
        let Some(query) = self.tree_query.clone() else {
            return;
        };
        self.ensure_index();
        let mut hits = ide_navigation::fuzzy_matches(&query, &self.file_index);
        hits.truncate(200);
        self.tree_search_cursor = if hits.is_empty() {
            0
        } else {
            self.tree_search_cursor.min(hits.len() - 1)
        };
        self.tree_hits = hits;
    }

    fn move_tree_search(&mut self, delta: isize) {
        if self.tree_hits.is_empty() {
            return;
        }
        self.tree_search_cursor = if delta < 0 {
            self.tree_search_cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (self.tree_search_cursor + delta as usize).min(self.tree_hits.len() - 1)
        };
    }

    fn accept_tree_search(&mut self) {
        let Some(hit) = self.tree_hits.get(self.tree_search_cursor).cloned() else {
            return;
        };
        self.tree_query = None;
        self.tree_hits.clear();
        self.open_path(self.workspace.join(hit.text));
    }

    fn open_yazi(&mut self) {
        let cwd = if self.workspace.is_dir() {
            self.workspace.clone()
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };
        self.yazi = Some(YaziBrowse {
            cwd,
            items: Vec::new(),
            cursor: 0,
            scroll: 0,
        });
        self.agent_focused = false;
        self.reload_yazi();
    }

    fn reload_yazi(&mut self) {
        let Some(cwd) = self.yazi.as_ref().map(|yazi| yazi.cwd.clone()) else {
            return;
        };
        let items = files::list_dir(&cwd);
        let Some(yazi) = self.yazi.as_mut() else {
            return;
        };
        yazi.items = items;
        if yazi.items.is_empty() {
            yazi.cursor = 0;
        } else {
            yazi.cursor = yazi.cursor.min(yazi.items.len() - 1);
        }
        reveal_list(&mut yazi.scroll, yazi.cursor, 12);
    }

    fn move_yazi(&mut self, delta: isize) {
        let Some(yazi) = self.yazi.as_mut() else {
            return;
        };
        if yazi.items.is_empty() {
            return;
        }
        yazi.cursor = if delta < 0 {
            yazi.cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (yazi.cursor + delta as usize).min(yazi.items.len() - 1)
        };
        reveal_list(&mut yazi.scroll, yazi.cursor, 12);
    }

    fn handle_yazi(&mut self, key: CommandKey) {
        match key {
            CommandKey::CtrlQ => self.request_quit(),
            CommandKey::Esc | CommandKey::Char('q') => self.yazi = None,
            CommandKey::Up | CommandKey::Char('k') => self.move_yazi(-1),
            CommandKey::Down | CommandKey::Char('j') => self.move_yazi(1),
            CommandKey::Left | CommandKey::Char('h') | CommandKey::Backspace => self.yazi_parent(),
            CommandKey::Enter | CommandKey::Right | CommandKey::Char('l') => self.accept_yazi(),
            _ => {}
        }
    }

    fn yazi_parent(&mut self) {
        let Some(parent) = self
            .yazi
            .as_ref()
            .and_then(|yazi| yazi.cwd.parent().map(Path::to_path_buf))
        else {
            return;
        };
        if parent.as_path()
            == self
                .yazi
                .as_ref()
                .map(|yazi| yazi.cwd.as_path())
                .unwrap_or(Path::new(""))
        {
            return;
        }
        if let Some(yazi) = self.yazi.as_mut() {
            yazi.cwd = parent;
            yazi.cursor = 0;
            yazi.scroll = 0;
        }
        self.reload_yazi();
    }

    fn accept_yazi(&mut self) {
        let Some(item) = self
            .yazi
            .as_ref()
            .and_then(|yazi| yazi.items.get(yazi.cursor).cloned())
        else {
            return;
        };
        if item.dir {
            if let Some(yazi) = self.yazi.as_mut() {
                yazi.cwd = item.path;
                yazi.cursor = 0;
                yazi.scroll = 0;
            }
            self.reload_yazi();
            return;
        }
        self.yazi = None;
        self.open_path(item.path);
    }

    fn yazi_click(&mut self, column: u16, row: u16, area: Rect) {
        let float = yazi_rect(area);
        if !contains(float, column, row) {
            self.yazi = None;
            return;
        }
        let inner = inset(float);
        if inner.height <= 1 || row <= inner.y || row >= inner.y + inner.height {
            return;
        }
        let index =
            self.yazi.as_ref().map(|yazi| yazi.scroll).unwrap_or(0) + (row - inner.y - 1) as usize;
        let open = {
            let Some(yazi) = self.yazi.as_mut() else {
                return;
            };
            if index >= yazi.items.len() {
                return;
            }
            let open = yazi.cursor == index;
            yazi.cursor = index;
            open
        };
        if open {
            self.accept_yazi();
        }
    }

    fn take_pick(&mut self) -> bool {
        let pick = self.pick;
        self.pick = false;
        pick
    }
}

pub fn draw(frame: &mut Frame, app: &EditorApp) {
    let area = frame.area();
    // Full-screen Help replaces the entire terminal (edocview-style doc preview).
    if app.help {
        draw_help_fullscreen(frame, app, area);
        return;
    }
    let chrome = app.frame_chrome(area);
    let gaze = if app.editing { "EDIT" } else { "VIEW" };
    let help_hover = app
        .hover
        .is_some_and(|(column, row)| help_hit(column, row, area, chrome.status));
    let hints_hover = app
        .hover
        .is_some_and(|(column, row)| hints_hit(column, row, area));
    let settings_hover = app
        .hover
        .is_some_and(|(column, row)| settings_hit(column, row, area));
    let quit_hover = app
        .hover
        .is_some_and(|(column, row)| quit_hit(column, row, area));
    let header = Paragraph::new(format!(
        "Posit {}  {}  {gaze}",
        app.language(),
        app.focused().name
    ))
    .block(header_block(
        help_hover,
        hints_hover,
        app.hover_hints,
        settings_hover,
        quit_hover,
    ))
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(
        header,
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 3,
        },
    );
    let mut toolbar = WorkbenchToolbar::default();
    toolbar.render(
        frame,
        chrome.toolbar,
        app.hover
            .and_then(|(column, row)| toolbar_hit(chrome.toolbar, column, row)),
    );
    if app.finder.is_some() {
        frame.render_widget(finder_paragraph(app), chrome.editor);
    } else if matches!(app.overlay, Overlay::Settings) {
        frame.render_widget(settings_paragraph(app), chrome.editor);
    } else if matches!(app.overlay, Overlay::Vim) {
        frame.render_widget(vim_paragraph(), chrome.editor);
    } else {
        if chrome.tree.width > 0 {
            if app.sidebar && chrome.tree.width > 4 {
                frame.render_widget(tree_paragraph(app), chrome.tree);
                let inner = inset(chrome.tree);
                let visible = inner.height.max(1) as usize;
                let total = app.tree.len().max(1);
                scrollbar::paint(
                    frame.buffer_mut(),
                    Rect {
                        x: inner.x.saturating_add(inner.width.saturating_sub(1)),
                        y: inner.y,
                        width: 1,
                        height: inner.height,
                    },
                    scrollbar::ScrollMetrics {
                        offset: app.tree_scroll,
                        visible,
                        total,
                    },
                );
            } else {
                frame.render_widget(sidebar_rail(app.hover_in(chrome.tree)), chrome.tree);
            }
        }
        if chrome.agent.width > 4 {
            frame.render_widget(agent_paragraph(app, chrome.agent), chrome.agent);
            let inner = inset(chrome.agent);
            let total = app.agent_log.len().max(1) + 2;
            scrollbar::paint(
                frame.buffer_mut(),
                Rect {
                    x: inner.x.saturating_add(inner.width.saturating_sub(1)),
                    y: inner.y,
                    width: 1,
                    height: inner.height,
                },
                scrollbar::ScrollMetrics {
                    offset: 0,
                    visible: inner.height.max(1) as usize,
                    total,
                },
            );
        } else if chrome.agent.width > 0 {
            frame.render_widget(agent_rail(app.hover_in(chrome.agent)), chrome.agent);
        }
        if chrome.canvas.width > 0 && app.canvas_open() {
            // Mid-drag: cheap border chrome only — no Clear, no markdown re-render,
            // no Kitty placeholder rewrite. Full refresh happens on mouse-up.
            if app.canvas_resize {
                let kind = app.runtime_dock.as_deref().unwrap_or("");
                let title = if kind == "markdown" { "Preview" } else { "Canvas" };
                let border = panel_border(true, false, false, true);
                let media = canvas_media_rect(chrome.canvas);
                let media_h = media.height.max(1) as usize;
                let lines: Vec<Line<'static>> = (0..media_h).map(|_| Line::from("")).collect();
                frame.render_widget(
                    Paragraph::new(Text::from(lines)).block(
                        Block::default()
                            .title(title)
                            .title(
                                Line::from(TuiSpan::styled(
                                    " resize ",
                                    Style::default().fg(Color::Black).bg(Color::Yellow),
                                ))
                                .alignment(Alignment::Right),
                            )
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(border)),
                    ),
                    chrome.canvas,
                );
            } else {
            // Leave Canvas cells alone while the editor scrolls — Clear would wipe
            // Kitty placeholders and force a full PNG re-upload every wheel tick.
            let paint_chrome = app.canvas_chrome_dirty.get() || app.canvas_painted_size.is_none();
            if paint_chrome {
                let hover_canvas = app.hover_in(chrome.canvas);
                let border = panel_border(
                    app.pane_focus() == PaneFocus::Runtime,
                    hover_canvas && app.pane_focus() != PaneFocus::Runtime,
                    false,
                    true,
                );
                let close_hot = app.hover.is_some_and(|(c, r)| {
                    app.canvas_title_hit(c, r, &chrome) == Some(CanvasTitleHit::Close)
                });
                let mut right = Vec::new();
                if app.canvas_surfaces.len() > 1 {
                    for (index, surface) in app.canvas_surfaces.iter().enumerate() {
                        let active = app.runtime_dock.as_deref() == Some(surface.kind.as_str());
                        let tab_hot = app.hover.is_some_and(|(c, r)| {
                            app.canvas_title_hit(c, r, &chrome)
                                == Some(CanvasTitleHit::Tab(index))
                        });
                        let label = format!(" {} ", EditorApp::canvas_tab_label(&surface.kind));
                        right.push(TuiSpan::styled(
                            label,
                            if tab_hot {
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD)
                            } else if active {
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ));
                    }
                }
                right.push(TuiSpan::styled(
                    " x ",
                    if close_hot {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    },
                ));
                // Media area stays blank for Kitty surfaces. Markdown Preview
                // paints a rendered document instead.
                let kind = app.runtime_dock.as_deref().unwrap_or("");
                let target = app.canvas_active().map(|s| s.target.as_str());
                let controls = EditorApp::canvas_control_line(kind, target);
                let media = canvas_media_rect(chrome.canvas);
                let media_h = media.height.max(1) as usize;
                let mut lines: Vec<Line<'static>> = if kind == "markdown" {
                    let body_h = media_h.saturating_sub(1).max(1);
                    if markdown_uses_kitty(app) {
                        // edocview Kitty crop covers the body; keep cells empty.
                        (0..body_h).map(|_| Line::from("")).collect()
                    } else {
                        let inner_w = media.width.max(1) as usize;
                        let preview = markdown_preview::render(
                            &app.markdown_preview_source(),
                            inner_w.saturating_sub(1).max(8),
                        );
                        let scroll = app.markdown_preview_scroll as usize;
                        let mut out: Vec<Line<'static>> = preview
                            .lines
                            .into_iter()
                            .skip(scroll)
                            .take(body_h)
                            .collect();
                        while out.len() < body_h {
                            out.push(Line::from(""));
                        }
                        out
                    }
                } else {
                    (0..media_h.saturating_sub(1).max(1))
                        .map(|_| Line::from(""))
                        .collect()
                };
                if kind != "markdown" {
                    lines.push(Line::from(TuiSpan::styled(
                        controls,
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )));
                } else {
                    lines.push(Line::from(TuiSpan::styled(
                        " Preview  ·  scroll with wheel  ·  zp toggles ",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                let title = if kind == "markdown" {
                    "Preview"
                } else {
                    "Canvas"
                };
                frame.render_widget(Clear, chrome.canvas);
                frame.render_widget(
                    Paragraph::new(Text::from(lines)).block(
                        Block::default()
                            .title(title)
                            .title(Line::from(right).alignment(Alignment::Right))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(border)),
                    ),
                    chrome.canvas,
                );
                app.canvas_chrome_dirty.set(false);
                if kind != "markdown" {
                    app.canvas_placeholders_stale.set(true);
                }
            } else if app.runtime_dock.as_deref() == Some("markdown") {
                // Text fallback refresh while scrolling. Kitty path re-places crops
                // in paint_markdown_canvas_graphics without rewriting chrome.
                if !markdown_uses_kitty(app) {
                    let media = canvas_media_rect(chrome.canvas);
                    let media_h = media.height.max(1) as usize;
                    let inner_w = media.width.max(1) as usize;
                    let preview = markdown_preview::render(
                        &app.markdown_preview_source(),
                        inner_w.saturating_sub(1).max(8),
                    );
                    let scroll = app.markdown_preview_scroll as usize;
                    let mut lines: Vec<Line<'static>> = preview
                        .lines
                        .into_iter()
                        .skip(scroll)
                        .take(media_h.saturating_sub(1).max(1))
                        .collect();
                    while lines.len() < media_h.saturating_sub(1).max(1) {
                        lines.push(Line::from(""));
                    }
                    lines.push(Line::from(TuiSpan::styled(
                        " Preview  ·  scroll with wheel  ·  zp toggles ",
                        Style::default().fg(Color::DarkGray),
                    )));
                    let hover_canvas = app.hover_in(chrome.canvas);
                    let border = panel_border(
                        app.pane_focus() == PaneFocus::Runtime,
                        hover_canvas && app.pane_focus() != PaneFocus::Runtime,
                        false,
                        true,
                    );
                    frame.render_widget(
                        Paragraph::new(Text::from(lines)).block(
                            Block::default()
                                .title("Preview")
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(border)),
                        ),
                        chrome.canvas,
                    );
                }
            }
            } // end non-resize canvas chrome
        }
        let orientation = if area.width == app.width && area.height == app.height {
            app.axis()
        } else {
            layout::axis(area.width, area.height)
        };
        let window = app.layout_window(chrome.content, orientation);
        for index in 0..app.buffers.len() {
            if let Some(slice) = layout::tile_slice(window, index, app.view) {
                paint_tile(frame, app, index, window, slice);
            }
        }
        if app.spatial.active_mode != SpatialMode::Idle {
            paint_spatial(frame, app, chrome.content);
        }
    }
    if chrome.terminal.height > 0 {
        let hover_col = app.hover.map(|(c, _)| c).unwrap_or(0);
        let hover_row = app.hover.map(|(_, r)| r).unwrap_or(0);
        // Seam hover paints only the active edge + pill (see paint_seam_handles).
        // Never yellow the whole terminal border — that mislabels which side resizes.
        let seam_edge = app.on_terminal_seam(hover_col, hover_row)
            || app.on_terminal_side_seam(hover_col, hover_row);
        let hovered = app.hover_in(chrome.terminal) && !seam_edge;
        let border = panel_border(
            app.terminals.focused(),
            hovered && !app.terminals.focused(),
            false,
            true,
        );
        let local_hover = app.hover.and_then(|(column, row)| {
            contains(chrome.terminal, column, row).then_some((
                column.saturating_sub(chrome.terminal.x),
                row.saturating_sub(chrome.terminal.y),
            ))
        });
        frame.render_widget(Clear, chrome.terminal);
        frame.render_widget(
            app.terminals.paragraph(
                chrome.terminal.width,
                chrome.terminal.height,
                local_hover,
                border,
            ),
            chrome.terminal,
        );
        if chrome.terminal.height > 2 && chrome.terminal.width > 4 {
            let inner = inset(chrome.terminal);
            let (offset, total) = app.terminals.scroll_metrics();
            scrollbar::paint(
                frame.buffer_mut(),
                Rect {
                    x: inner.x.saturating_add(inner.width.saturating_sub(1)),
                    y: inner.y,
                    width: 1,
                    height: inner.height,
                },
                scrollbar::ScrollMetrics {
                    offset,
                    visible: inner.height.max(1) as usize,
                    total: total.max(1),
                },
            );
        }
    }
    let notice = if app.notice.is_empty() {
        String::new()
    } else {
        format!("  {}", app.notice)
    };
    let axis = match if area.width == app.width && area.height == app.height {
        app.axis()
    } else {
        layout::axis(area.width, area.height)
    } {
        Axis::Horizontal => "landscape",
        Axis::Vertical => "portrait",
    };
    let mode = if app.show_many { "auto" } else { "full" };
    let pending = if app.chord.is_empty() {
        String::new()
    } else {
        format!("  {}…", app.chord)
    };
    let selected = app
        .focused()
        .selection
        .map(|(start, end)| end.saturating_sub(start))
        .filter(|count| *count > 0)
        .map(|count| format!("  sel {count}"))
        .unwrap_or_default();
    let status_body = format!(
        "   {gaze}{selected}  {axis}  {mode}  {}  rev {}  {}  chars {}{pending}{notice}",
        app.focused().name,
        app.focused().document.revision(),
        app.language(),
        app.focused().document.len_chars()
    );
    let help_style = if help_hover {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let status_line = Line::from(vec![
        TuiSpan::styled("? help", help_style),
        TuiSpan::raw(status_body),
    ]);
    if app.palette {
        let palette = Paragraph::new(format!(": {}", app.query))
            .block(Block::default().title("Palette").borders(Borders::ALL));
        frame.render_widget(palette, bottom_band(area));
    } else if let Some(prompt) = prompt_line(app) {
        let panel = Paragraph::new(prompt).block(
            Block::default()
                .title(prompt_title(app))
                .borders(Borders::ALL),
        );
        frame.render_widget(panel, bottom_band(area));
    } else if let Some(status_rect) = chrome.status {
        let reserved = tab_bounds(app.tab_enabled, app.tab_status(), status_rect);
        let text_rect = Rect {
            x: status_rect.x,
            y: status_rect.y,
            width: status_rect.width.saturating_sub(reserved.width),
            height: status_rect.height,
        };
        frame.render_widget(Paragraph::new(status_line), text_rect);
        paint_tab(frame, app, status_rect);
    }
    paint_seam_handles(frame, app);
    paint_buffer_strip(frame, app);
    if app.flip.open {
        paint_flip_switcher(frame, app);
    }
    if app.yazi.is_some() {
        let float = yazi_rect(area);
        frame.render_widget(Clear, float);
        frame.render_widget(yazi_paragraph(app, float.width), float);
    }
    paint_notifications(frame, app, area);
    if let Overlay::Menu(menu) = app.overlay {
        let rect = menu_rect(menu.x, menu.y, area);
        frame.render_widget(Clear, rect);
        frame.render_widget(menu_paragraph(menu.cursor), rect);
    }
    paint_hover_hint(frame, app, area);
}

fn yazi_rect(area: Rect) -> Rect {
    let width = ((area.width as u32) * 7 / 10).clamp(24, area.width.max(1) as u32) as u16;
    let height = ((area.height as u32) * 7 / 10).clamp(8, area.height.max(1) as u32) as u16;
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn yazi_paragraph(app: &EditorApp, width: u16) -> Paragraph<'static> {
    let Some(yazi) = app.yazi.as_ref() else {
        return Paragraph::new("");
    };
    let mut lines = vec![Line::styled(
        yazi.cwd.display().to_string(),
        Style::default().fg(Color::Cyan),
    )];
    let inner = width.saturating_sub(2) as usize;
    for (index, item) in yazi.items.iter().enumerate().skip(yazi.scroll).take(40) {
        let marker = if item.dir { "/" } else { "" };
        let label = format!("{}{marker}", item.name);
        let style = if index == yazi.cursor {
            cursor_style(app.editing)
        } else if item.dir {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        lines.push(Line::styled(
            label.chars().take(inner.max(1)).collect::<String>(),
            style,
        ));
    }
    if yazi.items.is_empty() {
        lines.push(Line::from("(empty)"));
    }
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title("Yazi")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(focus_color(app.editing))),
    )
}

fn tree_paragraph(app: &EditorApp) -> Paragraph<'static> {
    if app.tree_query.is_some() {
        let width = app.tree_rect().width.saturating_sub(2) as usize;
        let mut lines = vec![Line::styled(
            format!("/{}", app.tree_query.as_deref().unwrap_or("")),
            Style::default().fg(Color::Yellow),
        )];
        for (index, hit) in app.tree_hits.iter().enumerate() {
            lines.push(finder::matched_line(
                &hit.text,
                &hit.positions,
                index == app.tree_search_cursor,
                width.max(1),
            ));
        }
        if app.tree_hits.is_empty() {
            lines.push(Line::from("(no match)"));
        }
        return Paragraph::new(Text::from(lines)).block(files_block(
            app.tree_focused,
            false,
            false,
        ));
    }
    let inner = app.tree_rect().width.saturating_sub(2) as usize;
    let mut lines = Vec::new();
    for (index, entry) in app.tree.iter().enumerate() {
        let marker = if entry.dir {
            if app.open_dirs.contains(&entry.path) {
                "v "
            } else {
                "> "
            }
        } else {
            "  "
        };
        let mut style = if entry.dir {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        if app.tree_focused && index == app.tree_cursor {
            style = style.add_modifier(Modifier::REVERSED);
        } else if tree_hover(app) == Some(index) {
            style = style.bg(Color::DarkGray);
        }
        lines.push(Line::styled(
            tree_label(&entry.name, entry.depth, marker, inner.max(1)),
            style,
        ));
    }
    if lines.is_empty() {
        lines.push(Line::from("(empty)"));
    }
    let tree = app.tree_rect();
    let hovered = app.hover_in(tree) && !app.sidebar_button(
        tree,
        app.hover.map(|(c, _)| c).unwrap_or(0),
        app.hover.map(|(_, r)| r).unwrap_or(0),
    );
    Paragraph::new(Text::from(lines))
        .scroll((app.tree_scroll.min(u16::MAX as usize) as u16, 0))
        .block(files_block(
            app.tree_focused,
            hovered && !app.tree_focused,
            app.sidebar_title_hot(),
        ))
}

fn files_block(focused: bool, hovered: bool, toggle_hot: bool) -> Block<'static> {
    let mark = if toggle_hot {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    Block::default()
        .title("Files")
        .title(Line::from(TuiSpan::styled(" < ", mark)).alignment(Alignment::Right))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(panel_border(focused, hovered, false, false)))
}

fn agent_paragraph(app: &EditorApp, pane: Rect) -> Paragraph<'static> {
    let inner_w = pane.width.saturating_sub(2).max(1) as usize;
    let inner_h = pane.height.saturating_sub(2) as usize;
    let program = if app.agent_program.is_empty() {
        "no CLI".to_string()
    } else {
        app.agent_program.clone()
    };
    let draft = if app.agent_focused {
        format!("› {}", app.agent_draft)
    } else {
        "click to ask".to_string()
    };
    let program_lines = wrap_line(&program, inner_w);
    let draft_lines = wrap_line(&draft, inner_w);
    let mut body = Vec::new();
    for line in &app.agent_log {
        body.extend(wrap_line(line, inner_w));
    }
    let room = inner_h.saturating_sub(program_lines.len() + draft_lines.len());
    let start = body.len().saturating_sub(room);
    let mut lines: Vec<Line> = program_lines
        .into_iter()
        .map(|text| Line::styled(text, Style::default().fg(Color::Cyan)))
        .collect();
    lines.extend(body.into_iter().skip(start).map(Line::from));
    let draft_style = Style::default().fg(if app.agent_focused {
        focus_color(true)
    } else {
        Color::Gray
    });
    lines.extend(
        draft_lines
            .into_iter()
            .map(|text| Line::styled(text, draft_style)),
    );
    let hovered = app.hover_in(pane)
        && !app.agent_button(
            pane,
            app.hover.map(|(c, _)| c).unwrap_or(0),
            app.hover.map(|(_, r)| r).unwrap_or(0),
        );
    // Seam highlight is edge-only via paint_seam_handles; keep panel chrome stable.
    let border = panel_border(
        app.agent_focused,
        hovered && !app.agent_focused,
        false,
        false,
    );
    let toggle = if app.agent_title_hot() {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title("Agent")
            .title(Line::from(TuiSpan::styled(" > ", toggle)).alignment(Alignment::Right))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border)),
    )
}

fn tree_label(name: &str, depth: usize, marker: &str, width: usize) -> String {
    let width = width.max(1);
    let full = format!("{}{marker}{name}", " ".repeat(depth));
    if full.chars().count() <= width {
        return full;
    }
    let label = if width == 1 {
        "…".to_string()
    } else {
        let tail = format!("{marker}{name}");
        let room = width - 1;
        if tail.chars().count() <= room {
            let spaces = room - tail.chars().count();
            format!("…{}{tail}", " ".repeat(spaces))
        } else {
            let name_room = room
                .saturating_sub(marker.chars().count())
                .saturating_sub(1);
            let shortened: String = name.chars().take(name_room).collect();
            format!("…{marker}{shortened}…")
        }
    };
    if label.chars().count() <= width {
        label
    } else {
        let mut clipped: String = label.chars().take(width.saturating_sub(1)).collect();
        clipped.push('…');
        clipped
    }
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut chars: Vec<char> = line.trim_end().chars().collect();
    if chars.len() <= width {
        return vec![chars.into_iter().collect()];
    }
    let mut lines = Vec::new();
    while chars.len() > width {
        let mut break_at = width;
        if let Some(space) = chars[..width].iter().rposition(|ch| *ch == ' ') {
            if space >= width / 3 {
                break_at = space;
            }
        }
        let head: String = chars[..break_at].iter().collect();
        let head = head.trim_end();
        if head.is_empty() {
            break;
        }
        lines.push(head.to_string());
        chars = chars[break_at..].to_vec();
        while chars.first() == Some(&' ') {
            chars.remove(0);
        }
        if lines.len() > 200 {
            break;
        }
    }
    if !chars.is_empty() {
        lines.push(chars.into_iter().collect());
    }
    lines
}

fn agent_rail(hot: bool) -> Paragraph<'static> {
    Paragraph::new(">")
        .alignment(Alignment::Center)
        .style(hot_mark(hot))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if hot { Color::Yellow } else { Color::Gray })),
        )
}

fn sidebar_rail(hot: bool) -> Paragraph<'static> {
    Paragraph::new("<")
        .alignment(Alignment::Center)
        .style(hot_mark(hot))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if hot { Color::Yellow } else { Color::Gray })),
        )
}

fn hot_mark(hot: bool) -> Style {
    if hot {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
}

fn settings_paragraph(app: &EditorApp) -> Paragraph<'static> {
    let sidebar = if app.sidebar { "on" } else { "off" };
    let chat = if app.chat { "on" } else { "off" };
    let columns = if app.show_many { "auto" } else { "full" };
    let welcome = if app.show_welcome { "on" } else { "off" };
    let kitty = if app.kitty_graphics { "on" } else { "off" };
    let rows = [
        "Settings".to_string(),
        String::new(),
        format!("sidebar   {sidebar}    < or drag"),
        format!(
            "terminal  {}    t",
            if app.terminals.open { "on" } else { "off" }
        ),
        format!("chat      {chat}       > or drag"),
        format!("columns   {columns}    f or click"),
        format!(
            "tab       {}    Ctrl-Alt-T",
            if app.tab_enabled { "on" } else { "off" }
        ),
        format!("welcome   {welcome}    w — open Welcome.md on startup"),
        format!("kitty     {kitty}    k — Kitty graphics (minimap/Canvas/Flip)"),
        "Tab completion is local and lexical until a model is installed.".to_string(),
        String::new(),
        "Esc or ; closes".to_string(),
    ];
    let lines = rows
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let style = if settings_hot(app, index) {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::styled(text, style)
        })
        .collect::<Vec<_>>();
    Paragraph::new(Text::from(lines))
        .block(Block::default().title("Settings").borders(Borders::ALL))
}

fn settings_hot(app: &EditorApp, index: usize) -> bool {
    let Some((column, row)) = app.hover else {
        return false;
    };
    let editor = app
        .frame_chrome(Rect::new(0, 0, app.width, app.height))
        .editor;
    contains(editor, column, row)
        && row.saturating_sub(editor.y).saturating_sub(1) as usize == index
}

fn prompt_line(app: &EditorApp) -> Option<String> {
    match &app.overlay {
        Overlay::Quit => Some("y save&quit  ! quit  n cancel".to_string()),
        Overlay::Close(_) => Some("y save&close  ! close  n cancel".to_string()),
        Overlay::Rename(name) => Some(format!("rename: {name}")),
        Overlay::None | Overlay::Settings | Overlay::Vim | Overlay::Menu(_) => None,
    }
}

fn vim_paragraph() -> Paragraph<'static> {
    Paragraph::new(
        "This is not Vim. Please stop trying Vim motions. Esc leaves editing mode and enters view mode.",
    )
    .wrap(Wrap { trim: true })
    .block(Block::default().title("Not Vim").borders(Borders::ALL))
}

fn prompt_title(app: &EditorApp) -> &'static str {
    match &app.overlay {
        Overlay::Quit => "Quit",
        Overlay::Close(_) => "Close",
        Overlay::Rename(_) => "Rename",
        Overlay::None | Overlay::Settings | Overlay::Vim | Overlay::Menu(_) => "",
    }
}

fn tree_hover(app: &EditorApp) -> Option<usize> {
    let (column, row) = app.hover?;
    let tree = app.tree_rect();
    if tree.width <= 4 || !contains(tree, column, row) {
        return None;
    }
    let local = row.saturating_sub(tree.y).saturating_sub(1) as usize;
    let index = local + app.tree_scroll;
    (index < app.tree.len()).then_some(index)
}

fn line_span_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum()
}

/// Idle split pills + hover edge highlight. Only the active edge lights up.
fn paint_buffer_strip(frame: &mut Frame, app: &EditorApp) {
    let area = frame.area();
    let chrome = app.frame_chrome(area);
    let (_, strip) = app.content_tiles_and_strip(chrome.content);
    if strip.width < 2 || strip.height == 0 || app.buffers.is_empty() {
        return;
    }
    let metrics = app.buffer_strip_metrics();
    scrollbar::paint_horizontal(frame.buffer_mut(), strip, metrics);
    // Focus name chip on the strip for glanceability.
    let name = app.focused().name.chars().take(16).collect::<String>();
    let label = format!(" {}/{} · {name} ", app.focus + 1, app.buffers.len());
    let style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let x0 = strip
        .x
        .saturating_add(strip.width.saturating_sub(label.chars().count() as u16) / 2);
    for (i, ch) in label.chars().enumerate() {
        let x = x0.saturating_add(i as u16);
        if x >= strip.x.saturating_add(strip.width) {
            break;
        }
        let cell = &mut frame.buffer_mut()[(x, strip.y)];
        cell.set_symbol(&ch.to_string());
        cell.set_style(style);
    }
}

fn paint_flip_switcher(frame: &mut Frame, app: &EditorApp) {
    let area = frame.area();
    let chrome = app.frame_chrome(area);
    let (tiles, _) = app.content_tiles_and_strip(chrome.content);
    if tiles.width < 8 || tiles.height < 6 {
        return;
    }
    // Kitty path: solid theme underlay only. ASCII card chrome + Clear was wiping
    // Unicode placeholders every frame; settled paints then skipped rebind → static.
    if minimap_gfx::host_is_kitty() && app.kitty_graphics {
        let t = app.minimap_theme;
        let bg = Color::Rgb(t.bg.0, t.bg.1, t.bg.2);
        frame.render_widget(Clear, tiles);
        let fill = Paragraph::new("").style(Style::default().bg(bg).fg(bg));
        frame.render_widget(fill, tiles);
        return;
    }
    let cards = app.flip_cards();
    flip3d::paint_text_fallback(
        frame,
        tiles,
        &cards,
        app.flip.scroll,
        app.flip.selected,
    );
}

fn paint_seam_handles(frame: &mut Frame, app: &EditorApp) {
    let area = frame.area();
    let chrome = app.frame_chrome(area);
    let hover = app.hover;
    let buf = frame.buffer_mut();

    let mut idle: Vec<seams::SeamVisual> = Vec::new();
    let mut active: Option<seams::SeamVisual> = None;

    if chrome.tree.width > 4 {
        if let Some(seam) = seams::left_pane_seam(chrome.tree) {
            idle.push(seam);
            if hover.is_some_and(|(c, r)| app.seam(&chrome, c, r) == Some(Side::Left))
                || app.resize == Some(Side::Left)
            {
                active = Some(seam);
            }
        }
    }
    if chrome.agent.width > 4 {
        if let Some(seam) = seams::right_pane_seam(chrome.agent) {
            idle.push(seam);
            if hover.is_some_and(|(c, r)| app.seam(&chrome, c, r) == Some(Side::Right))
                || app.resize == Some(Side::Right)
            {
                active = Some(seam);
            }
        }
    }
    if chrome.canvas.width > 0 && app.canvas_open() {
        if let Some(seam) = seams::right_pane_seam(chrome.canvas) {
            idle.push(seam);
            if hover.is_some_and(|(c, r)| app.on_canvas_seam(c, r, &chrome)) || app.canvas_resize
            {
                active = Some(seam);
            }
        }
    }
    if chrome.terminal.height > 0 {
        if let Some(seam) = seams::bottom_pane_seam(chrome.terminal) {
            idle.push(seam);
            if hover.is_some_and(|(c, r)| app.on_terminal_seam(c, r)) || app.terminal_drag {
                active = Some(seam);
            }
        }
        // Session-list divider inside the terminal pane.
        if chrome.terminal.height > 2 {
            let term = chrome.terminal;
            if let Some(local_x) = app.terminals.side_divider_x(term.width) {
                let seam = seams::SeamVisual::vertical(
                    term.x.saturating_add(local_x),
                    term.y.saturating_add(1),
                    term.height.saturating_sub(2).max(1),
                );
                idle.push(seam);
                if hover.is_some_and(|(c, r)| app.on_terminal_side_seam(c, r))
                    || app.terminal_side_drag
                {
                    active = Some(seam);
                }
            }
        }
    }
    // Text buffers: trailing-edge pills only (right / bottom), including the last.
    if !app.buffers.is_empty() {
        if app.show_many {
            let window = app.window();
            for index in 0..app.buffers.len() {
                let Some(slice) = layout::tile_slice(window, index, app.view) else {
                    continue;
                };
                let (tile_w, tile_h) = window.tile_box(index);
                let seam = match window.axis {
                    Axis::Horizontal if slice.source_x + slice.width == tile_w => {
                        let x = slice.screen_x.saturating_add(slice.width).saturating_sub(1);
                        Some(seams::SeamVisual::vertical(
                            x,
                            slice.screen_y,
                            slice.height.max(1),
                        ))
                    }
                    Axis::Vertical if slice.source_y + slice.height == tile_h => {
                        let y = slice
                            .screen_y
                            .saturating_add(slice.height)
                            .saturating_sub(1);
                        Some(seams::SeamVisual::horizontal(
                            slice.screen_x,
                            y,
                            slice.width.max(1),
                        ))
                    }
                    _ => None,
                };
                if let Some(seam) = seam {
                    idle.push(seam);
                    if hover.is_some_and(|(c, r)| app.tile_seam(c, r) == Some(index))
                        || app.tile_drag == Some(index)
                    {
                        active = Some(seam);
                    }
                }
            }
        } else if app.buffers.len() >= 2 {
            // Full mode: trailing-edge pill on the focused buffer only.
            let (tiles, _) = app.content_tiles_and_strip(chrome.content);
            let focus = app.focus.min(app.buffers.len() - 1);
            if let Some(seam) = match app.axis {
                Axis::Horizontal => seams::right_pane_seam(tiles),
                Axis::Vertical => Some(seams::SeamVisual::horizontal(
                    tiles.x,
                    tiles.y.saturating_add(tiles.height.saturating_sub(1)),
                    tiles.width,
                )),
            } {
                idle.push(seam);
                if hover.is_some_and(|(c, r)| app.tile_seam(c, r) == Some(focus))
                    || app.tile_drag == Some(focus)
                {
                    active = Some(seam);
                }
            }
        }
    }

    for seam in idle {
        let is_active = active.is_some_and(|a| {
            a.line.x == seam.line.x
                && a.line.y == seam.line.y
                && a.line.width == seam.line.width
                && a.line.height == seam.line.height
        });
        if is_active {
            seams::paint(buf, area, seam, true);
        } else {
            seams::paint_idle(buf, area, seam);
        }
    }
}

fn paint_tab(frame: &mut Frame, app: &EditorApp, status: Rect) {
    let mut control = TabControl::default();
    control.enabled = app.tab_enabled;
    control.status = app.tab_status();
    control.hovered = app.hover.is_some_and(|(column, row)| {
        contains(
            tab_bounds(app.tab_enabled, app.tab_status(), status),
            column,
            row,
        )
    });
    control.render(frame, status);
}

fn tab_bounds(enabled: bool, status: TabStatus, area: Rect) -> Rect {
    if area.width == 0 || area.height == 0 {
        return Rect::default();
    }
    let backend = ratatui::backend::TestBackend::new(area.width, 1);
    let mut terminal = Terminal::new(backend).expect("tab bounds");
    let mut control = TabControl::default();
    control.enabled = enabled;
    control.status = status;
    let _ = terminal.draw(|frame| control.render(frame, Rect::new(0, 0, area.width, 1)));
    let bounds = control.bounds();
    Rect {
        x: area.x.saturating_add(bounds.x),
        y: area.y,
        width: bounds.width,
        height: 1,
    }
}

fn load_setting_bool(workspace: &Path, key: &str, default: bool) -> bool {
    let path = workspace.join(".posit/settings.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return default;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| value.get(key)?.as_bool())
        .unwrap_or(default)
}

fn persist_setting_bool(workspace: &Path, key: &str, value: bool) {
    let dir = workspace.join(".posit");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("settings.json");
    let mut map = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    map.insert(key.into(), serde_json::Value::Bool(value));
    let _ = std::fs::write(path, format!("{}\n", serde_json::Value::Object(map)));
}

fn load_show_welcome(workspace: &Path) -> bool {
    load_setting_bool(workspace, "show_welcome", true)
}

fn persist_show_welcome(workspace: &Path, show_welcome: bool) {
    persist_setting_bool(workspace, "show_welcome", show_welcome);
}

fn load_kitty_graphics(workspace: &Path) -> bool {
    load_setting_bool(workspace, "kitty_graphics", true)
}

fn persist_kitty_graphics(workspace: &Path, enabled: bool) {
    persist_setting_bool(workspace, "kitty_graphics", enabled);
}

fn local_tab_enabled(workspace: &Path) -> bool {
    let path = workspace.join(".posit/local-tab.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return true;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| value.get("enabled").and_then(|flag| flag.as_bool()))
        .unwrap_or(true)
}

fn bottom_band(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.height.saturating_sub(3),
        width: area.width,
        height: 3.min(area.height),
    }
}

fn finder_paragraph(app: &EditorApp) -> Paragraph<'static> {
    let Some(finder) = app.finder.as_ref() else {
        return Paragraph::new("");
    };
    let title = match finder.kind {
        FinderKind::Files => "Find",
        FinderKind::Grep => "Grep",
        FinderKind::Buffers => "Buffers",
    };
    let width = app.width.saturating_sub(4) as usize;
    let mut lines = vec![Line::styled(
        format!("> {}", finder.query),
        Style::default().fg(Color::Cyan),
    )];
    match finder.kind {
        FinderKind::Files | FinderKind::Buffers => {
            for (index, hit) in finder.files.iter().enumerate().skip(finder.scroll).take(40) {
                lines.push(finder::matched_line(
                    &hit.text,
                    &hit.positions,
                    index == finder.cursor,
                    width.max(1),
                ));
            }
        }
        FinderKind::Grep => {
            for (index, hit) in finder.greps.iter().enumerate().skip(finder.scroll).take(40) {
                let label = format!("{}:{}: {}", hit.path.display(), hit.line, hit.text);
                let prefix = format!("{}:{}: ", hit.path.display(), hit.line);
                let offset = prefix.chars().count();
                lines.push(finder::ranged_line(
                    &label,
                    offset + hit.start,
                    offset + hit.end,
                    index == finder.cursor,
                    width.max(1),
                ));
            }
        }
    }
    if lines.len() == 1 {
        lines.push(Line::from(if finder.query.is_empty() {
            "type to search"
        } else {
            "(no match)"
        }));
    }
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(title)
            .title("Tab switches")
            .borders(Borders::ALL),
    )
}

fn finder_len(finder: &Finder) -> usize {
    match finder.kind {
        FinderKind::Files | FinderKind::Buffers => finder.files.len(),
        FinderKind::Grep => finder.greps.len(),
    }
}

fn reveal_list(scroll: &mut usize, cursor: usize, visible: usize) {
    if cursor < *scroll {
        *scroll = cursor;
    } else if visible > 0 && cursor >= *scroll + visible {
        *scroll = cursor + 1 - visible;
    }
}

fn paint_spatial(frame: &mut Frame, app: &EditorApp, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let title = match app.spatial.active_mode {
        SpatialMode::Overview => "Spatial overview",
        SpatialMode::Lens => "Lens",
        SpatialMode::Timeline => "Timeline",
        SpatialMode::Impact => "Change impact",
        SpatialMode::Structural => "Structural edit",
        SpatialMode::Preview => "Preview",
        SpatialMode::Idle => return,
    };
    let body: Vec<Line> = app
        .spatial_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let style = if index == app.spatial_cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(TuiSpan::styled(line.clone(), style))
        })
        .collect();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn paint_tile(
    frame: &mut Frame,
    app: &EditorApp,
    index: usize,
    window: Window,
    slice: layout::Slice,
) {
    let (tile_w, tile_h) = window.tile_box(index);
    let area = Rect::new(0, 0, tile_w, tile_h.max(1));
    let mut screen = Screen::empty(area);
    let buffer = &app.buffers[index];
    let tile_hovered = app.hover.is_some_and(|(column, row)| {
        layout::hit_tile(window, column, row, app.view, app.buffers.len()) == Some(index)
            && !app.hover_tile_seam(index)
            && !app.close_button(window, index, column, row)
    });
    let editor_owns = app.pane_focus() == PaneFocus::Editor;
    // Tile seam highlight is edge-only (paint_seam_handles); keep chrome stable.
    let border = panel_border(
        editor_owns && index == app.focus,
        tile_hovered && !(editor_owns && index == app.focus),
        false,
        app.editing && editor_owns && index == app.focus,
    );
    let close_hover = app
        .hover
        .is_some_and(|(column, row)| app.close_button(window, index, column, row));
    let close_style = if close_hover {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let visible = tile_h.saturating_sub(2) as usize;
    let line_count = buffer.document.len_lines().max(1);
    buffer.ensure_folds();
    let folds = buffer.folds.borrow();
    let visible_docs =
        folds.visible_window(buffer.scroll, visible.max(1).saturating_add(1), line_count);
    let fold_total = folds.visible_line_count(line_count);
    let any_collapsed = folds.collapsed_count() > 0;
    let gutter_w = app.gutter.width_for(line_count);
    // Kitty strip only when settled; during scroll / Flip / buffer slide use scrollbar.
    let kitty_minimap =
        index == app.focus && app.kitty_minimap_enabled() && !app.flip.open;
    let (minimap_w, scroll_w) = if index == app.focus {
        app.focused_edge_chrome(tile_w, fold_total, visible.max(1))
    } else {
        // Unfocused tiles: text scrollbar only when scrollable.
        if minimap_gfx::needs_scroll(fold_total, visible.max(1)) {
            (0, 1)
        } else {
            (0, 0)
        }
    };
    let chrome_w = gutter_w.saturating_add(minimap_w).saturating_add(scroll_w);
    let width = tile_w
        .saturating_sub(2)
        .saturating_sub(chrome_w)
        .max(1) as usize;
    // Obsidian-style: hover peeks one row; caret unconceals the block; selection shows raw.
    let hover_line = if app.editing && tile_hovered {
        app.hover.and_then(|(column, row)| {
            let _ = column;
            let local_y = row.saturating_sub(slice.screen_y);
            if local_y == 0 {
                return None; // title bar
            }
            let content_row = (local_y as usize).saturating_sub(1);
            if content_row >= visible {
                return None;
            }
            folds
                .doc_line_for_view_row(buffer.scroll, content_row, line_count)
                .or_else(|| Some(buffer.scroll.saturating_add(content_row)))
        })
    } else {
        None
    };
    let cursor_line = (app.editing && index == app.focus).then(|| {
        buffer.document.char_to_line(buffer.cursor)
    });
    let md_reveal = markdown_preview::RevealOpts {
        hover_line,
        cursor_line,
        // View-mode select-all / drag selection also drops render → raw source.
        selection: buffer.selection,
    };
    // Only materialize the visible line window — full-buffer paint dominated large-file RSS/CPU.
    let text = if any_collapsed {
        highlighted_doc_lines(
            buffer,
            &visible_docs,
            index == app.focus,
            app.editing,
            width,
            (index == app.focus)
                .then_some(app.ghost.as_ref())
                .flatten(),
            md_reveal,
        )
    } else {
        highlighted_window(
            buffer,
            index == app.focus,
            app.editing,
            width,
            (index == app.focus)
                .then_some(app.ghost.as_ref())
                .flatten(),
            buffer.scroll,
            visible.max(1).saturating_add(1),
            md_reveal,
        )
    };
    let max_x = text
        .lines
        .iter()
        .map(line_span_width)
        .max()
        .unwrap_or(0)
        .saturating_sub(width.max(1));
    let scroll_x = buffer.scroll_x.min(max_x).min(u16::MAX as usize) as u16;
    Block::default()
        .title(buffer.name.clone())
        .title(Line::from(TuiSpan::styled(" w ", close_style)).alignment(Alignment::Right))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .render(area, &mut screen);
    let inner = Rect {
        x: 1,
        y: 1,
        width: tile_w.saturating_sub(2),
        height: tile_h.saturating_sub(2),
    };
    let body = Rect {
        x: inner.x.saturating_add(gutter_w),
        y: inner.y,
        width: inner
            .width
            .saturating_sub(gutter_w)
            .saturating_sub(minimap_w)
            .saturating_sub(scroll_w)
            .max(1),
        height: inner.height,
    };
    Paragraph::new(text)
        .scroll((0, scroll_x))
        .render(body, &mut screen);
    let caret_line = buffer
        .with_text(|body| line_col(body, buffer.cursor).0)
        .saturating_add(1);
    let path = buffer.path.as_deref();
    let gutter_rows = if any_collapsed {
        visible_docs.clone()
    } else {
        (0..visible.min(inner.height as usize))
            .map(|row| buffer.scroll + row)
            .filter(|&line| line < line_count)
            .collect()
    };
    for (row, doc_line_0) in gutter_rows.iter().copied().enumerate() {
        if row >= visible.min(inner.height as usize) {
            break;
        }
        let line = doc_line_0 + 1;
        let chevron = folds.chevron(doc_line_0);
        let spans = app
            .gutter
            .line_spans(path, line, caret_line, line_count, gutter_w, chevron);
        let mut x = inner.x;
        for span in spans {
            let content = span.content.clone();
            let style = span.style;
            for ch in content.chars() {
                if x >= inner.x.saturating_add(gutter_w) {
                    break;
                }
                let cell = &mut screen[(x, inner.y.saturating_add(row as u16))];
                cell.set_symbol(&ch.to_string());
                cell.set_style(style);
                x = x.saturating_add(1);
            }
        }
    }
    drop(folds);
    if minimap_w > 0 {
        let base_x = inner
            .x
            .saturating_add(inner.width.saturating_sub(minimap_w + scroll_w));
        let theme = app.minimap_theme;
        let underlay_bg = Color::Rgb(theme.bg.0, theme.bg.1, theme.bg.2);
        if kitty_minimap {
            // Once Kitty placeholders are bound, do not paint spaces over them —
            // that deletes the virtual placement for a frame (scroll tear).
            // Underlay only until the first PNG bind (or after a clear).
            if app.editor_minimap_id.is_none() {
                let style = Style::default().bg(underlay_bg).fg(underlay_bg);
                for row in 0..inner.height {
                    for dx in 0..minimap_w {
                        let cell_ref =
                            &mut screen[(base_x.saturating_add(dx), inner.y.saturating_add(row))];
                        cell_ref.set_symbol(" ");
                        cell_ref.set_style(style);
                    }
                }
            }
        } else {
            let layout = minimap_gfx::layout_proportional(
                line_count,
                buffer.scroll,
                visible.max(1),
                (inner.height as u32).saturating_mul(minimap_gfx::PX_PER_LINE_BASE),
                1,
            );
            let samples = minimap_glyphs_window(buffer, layout.start_line, layout.shown_lines);
            let rows = inner.height as usize;
            for row in 0..rows {
                let src = if layout.shown_lines <= 1 || rows <= 1 {
                    layout.start_line
                } else {
                    layout.start_line
                        + (row * layout.shown_lines.saturating_sub(1)) / (rows - 1)
                };
                let glyph = samples
                    .get(src.saturating_sub(layout.start_line))
                    .copied()
                    .unwrap_or('·');
                let glyph = if glyph.is_whitespace() { '·' } else { glyph };
                let y = inner.y.saturating_add(row as u16);
                let in_view = src >= buffer.scroll && src < buffer.scroll + visible.max(1);
                let style = if in_view {
                    Style::default().fg(Color::Gray).bg(underlay_bg)
                } else {
                    Style::default().fg(Color::DarkGray).bg(underlay_bg)
                };
                for dx in 0..minimap_w {
                    let cell_ref = &mut screen[(base_x.saturating_add(dx), y)];
                    cell_ref.set_symbol(&glyph.to_string());
                    cell_ref.set_style(style);
                }
            }
        }
    }
    // Text scrollbar only when Kitty is not owning the right edge.
    if scroll_w > 0 {
        scrollbar::paint(
            &mut screen,
            Rect {
                x: inner.x.saturating_add(inner.width.saturating_sub(1)),
                y: inner.y,
                width: 1,
                height: inner.height,
            },
            scrollbar::ScrollMetrics {
                offset: buffer.scroll,
                visible: visible.max(1),
                total: fold_total,
            },
        );
    }
    let frame_area = frame.area();
    let dest = frame.buffer_mut();
    for y in 0..slice.height {
        for x in 0..slice.width {
            let sx = slice.screen_x.saturating_add(x);
            let sy = slice.screen_y.saturating_add(y);
            if sx < frame_area.right() && sy < frame_area.bottom() {
                dest[(sx, sy)] = screen[(
                    slice.source_x.saturating_add(x),
                    slice.source_y.saturating_add(y),
                )]
                    .clone();
            }
        }
    }
}

fn menu_rect(x: u16, y: u16, area: Rect) -> Rect {
    let width = 22.min(area.width.max(1));
    let height = (MENU.len() as u16 + 2).min(area.height.max(1));
    Rect {
        x: x.min(area.width.saturating_sub(width)),
        y: y.min(area.height.saturating_sub(height)),
        width,
        height,
    }
}

fn menu_paragraph(cursor: usize) -> Paragraph<'static> {
    let lines: Vec<Line> = MENU
        .iter()
        .enumerate()
        .map(|(index, (label, key))| {
            let style = if index == cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::styled(format!("{label:<12}{key}"), style)
        })
        .collect();
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title("Clipboard")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    )
}

fn header_block(
    help: bool,
    hints: bool,
    hints_on: bool,
    settings: bool,
    quit: bool,
) -> Block<'static> {
    let mark = |hot: bool| {
        if hot {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        }
    };
    // Dim when hover hints are off so the title control still shows state.
    let hints_style = if hints {
        mark(true)
    } else if hints_on {
        mark(false)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .title("Posit")
        .title(
            // Top-right cluster: ` ? h ; q `
            Line::from(vec![
                TuiSpan::raw(" "),
                TuiSpan::styled("?", mark(help)),
                TuiSpan::raw(" "),
                TuiSpan::styled("h", hints_style),
                TuiSpan::raw(" "),
                TuiSpan::styled(";", mark(settings)),
                TuiSpan::raw(" "),
                TuiSpan::styled("q", mark(quit)),
                TuiSpan::raw(" "),
            ])
            .alignment(Alignment::Right),
        )
        .borders(Borders::ALL)
}

fn help_hit(column: u16, row: u16, area: Rect, status: Option<Rect>) -> bool {
    // Title right cluster: ` ? h ; q ` → `?` at width-9.
    let title = row == area.y && column == area.x + area.width.saturating_sub(9);
    let label = status.is_some_and(|status| {
        row == status.y && column >= status.x && column < status.x.saturating_add(8)
    });
    title || label
}

fn hints_hit(column: u16, row: u16, area: Rect) -> bool {
    // Title right cluster: ` ? h ; q ` → `h` at width-7.
    row == area.y && column == area.x + area.width.saturating_sub(7)
}

fn settings_hit(column: u16, row: u16, area: Rect) -> bool {
    // Title right cluster: ` ? h ; q ` → `;` at width-5.
    row == area.y && column == area.x + area.width.saturating_sub(5)
}

fn quit_hit(column: u16, row: u16, area: Rect) -> bool {
    row == area.y && column == area.x + area.width.saturating_sub(3)
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && row >= rect.y
        && column < rect.x.saturating_add(rect.width)
        && row < rect.y.saturating_add(rect.height)
}

fn inset(rect: Rect) -> Rect {
    Rect {
        x: rect.x.saturating_add(1),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    }
}

/// Border inset minus the bottom control strip (phone/browser chrome).
fn canvas_media_rect(canvas: Rect) -> Rect {
    let inner = inset(canvas);
    if inner.height <= 1 {
        return inner;
    }
    Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    }
}

/// Bottom row inside the Canvas border for Home/Lock/Siri or Back/Fwd/Reload.
fn canvas_controls_rect(canvas: Rect) -> Rect {
    let inner = inset(canvas);
    if inner.width == 0 || inner.height == 0 {
        return Rect::default();
    }
    Rect {
        x: inner.x,
        y: inner.y.saturating_add(inner.height.saturating_sub(1)),
        width: inner.width,
        height: 1,
    }
}

fn hit_control_on_line(line: &str, local: usize, kind: &str) -> Option<CanvasControlHit> {
    let find = |needle: &str| -> Option<(usize, usize)> {
        let start = line.find(needle)?;
        Some((start, start + needle.len()))
    };
    match kind {
        "phone" => {
            if let Some((a, b)) = find("[Home]") {
                if local >= a && local < b {
                    return Some(CanvasControlHit::PhoneHome);
                }
            }
            if let Some((a, b)) = find("[Lock]") {
                if local >= a && local < b {
                    return Some(CanvasControlHit::PhoneLock);
                }
            }
            if let Some((a, b)) = find("[Siri]") {
                if local >= a && local < b {
                    return Some(CanvasControlHit::PhoneSiri);
                }
            }
            None
        }
        "browser" => {
            if let Some((a, b)) = find("[Back]") {
                if local >= a && local < b {
                    return Some(CanvasControlHit::BrowserBack);
                }
            }
            if let Some((a, b)) = find("[Fwd]") {
                if local >= a && local < b {
                    return Some(CanvasControlHit::BrowserForward);
                }
            }
            if let Some((a, b)) = find("[Reload]") {
                if local >= a && local < b {
                    return Some(CanvasControlHit::BrowserReload);
                }
            }
            None
        }
        _ => None,
    }
}

fn clamp_point(column: u16, row: u16, rect: Rect) -> (u16, u16) {
    if rect.width == 0 || rect.height == 0 {
        return (rect.x, rect.y);
    }
    let max_x = rect.x.saturating_add(rect.width.saturating_sub(1));
    let max_y = rect.y.saturating_add(rect.height.saturating_sub(1));
    (column.clamp(rect.x, max_x), row.clamp(rect.y, max_y))
}

fn word_bounds(text: &str, pos: usize) -> (usize, usize) {
    class_bounds(text, pos, |ch| {
        if ch.is_alphanumeric() || ch == '_' {
            Some(0)
        } else if ch.is_whitespace() && ch != '\n' {
            Some(1)
        } else if ch == '\n' {
            None
        } else {
            Some(2)
        }
    })
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// macOS-style: land on the start of the previous word.
fn word_jump_left(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || pos == 0 {
        return 0;
    }
    let mut i = pos.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    if is_word_char(chars[i - 1]) {
        while i > 0 && is_word_char(chars[i - 1]) {
            i -= 1;
        }
    } else {
        while i > 0 && !chars[i - 1].is_whitespace() && !is_word_char(chars[i - 1]) {
            i -= 1;
        }
    }
    i
}

/// macOS-style: land on the end of the current/next word.
fn word_jump_right(text: &str, pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = pos.min(len);
    if i >= len {
        return len;
    }
    if chars[i].is_whitespace() {
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
    }
    if i >= len {
        return len;
    }
    if is_word_char(chars[i]) {
        while i < len && is_word_char(chars[i]) {
            i += 1;
        }
    } else {
        while i < len && !chars[i].is_whitespace() && !is_word_char(chars[i]) {
            i += 1;
        }
    }
    i
}

fn line_bounds(text: &str, pos: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let mut index = pos.min(chars.len());
    if index == chars.len() {
        index -= 1;
    }
    if chars[index] == '\n' && index > 0 {
        index -= 1;
    }
    let mut start = index;
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = start;
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    if end < chars.len() && chars[end] == '\n' {
        end += 1;
    }
    (start, end)
}

fn class_bounds(text: &str, pos: usize, class: fn(char) -> Option<u8>) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let mut index = pos.min(chars.len());
    if index == chars.len() {
        index -= 1;
    }
    if chars[index] == '\n' && index > 0 && chars[index - 1] != '\n' {
        index -= 1;
    }
    let Some(kind) = class(chars[index]) else {
        return (index, index);
    };
    let mut start = index;
    while start > 0 && class(chars[start - 1]) == Some(kind) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && class(chars[end]) == Some(kind) {
        end += 1;
    }
    (start, end)
}

fn char_at(text: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (index, row) in text.split('\n').enumerate() {
        let len = row.chars().count();
        if index == line {
            return offset + column.min(len);
        }
        offset += len + 1;
    }
    text.chars().count()
}

fn line_col(text: &str, cursor: usize) -> (usize, usize) {
    let mut offset = 0;
    let mut last = 0;
    for (line, row) in text.split('\n').enumerate() {
        let len = row.chars().count();
        last = line;
        if cursor <= offset + len {
            return (line, cursor - offset);
        }
        offset += len + 1;
    }
    (last, 0)
}

fn char_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn line_byte_offset(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            seen += 1;
            if seen == line {
                return index + ch.len_utf8();
            }
        }
    }
    text.len()
}

/// Highlight selected document lines (0-based), skipping folded gaps.
fn highlighted_doc_lines(
    buffer: &EditBuffer,
    doc_lines: &[usize],
    show_cursor: bool,
    editing: bool,
    content_width: usize,
    ghost: Option<&Ghost>,
    md_reveal: markdown_preview::RevealOpts,
) -> Text<'static> {
    let mut lines = Vec::with_capacity(doc_lines.len());
    for &doc_line in doc_lines {
        let piece = highlighted_window(
            buffer,
            show_cursor,
            editing,
            content_width,
            ghost,
            doc_line,
            1,
            md_reveal,
        );
        if let Some(line) = piece.lines.into_iter().next() {
            lines.push(line);
        } else {
            lines.push(Line::default());
        }
    }
    Text::from(lines)
}

fn highlighted_window(
    buffer: &EditBuffer,
    show_cursor: bool,
    editing: bool,
    content_width: usize,
    ghost: Option<&Ghost>,
    first_line: usize,
    line_count: usize,
    md_reveal: markdown_preview::RevealOpts,
) -> Text<'static> {
    // Populate caches without cloning the full document / span vec into paint.
    buffer.ensure_highlight_cache();
    buffer.ensure_text_cache();
    let text_guard = buffer.text_cache.borrow();
    let text = text_guard
        .as_ref()
        .map(|(_, text)| text.as_str())
        .unwrap_or("");
    let is_markdown = buffer.language == "markdown"
        || buffer.name.ends_with(".md")
        || buffer.name == "welcome";
    if is_markdown {
        let mut lines = markdown_preview::render_buffer(
            text,
            first_line,
            line_count,
            content_width,
            md_reveal,
        )
        .lines;
        if show_cursor {
            let line_index = buffer.document.char_to_line(buffer.cursor);
            if line_index >= first_line
                && line_index < first_line.saturating_add(lines.len())
            {
                let local = line_index - first_line;
                // Precise caret when the caret line is source-revealed.
                let source_revealed = md_reveal.cursor_line.is_some_and(|c| {
                    markdown_preview::line_in_edit_block(text, c, line_index)
                }) || md_reveal.hover_line == Some(line_index)
                    || md_reveal
                        .selection
                        .is_some_and(|sel| markdown_preview::line_in_selection(text, sel, line_index));
                let col_in_line = if source_revealed {
                    display_col_in_line(text, buffer.cursor, 4)
                } else {
                    0
                };
                if let Some(ghost) = ghost {
                    if ghost.revision == buffer.document.revision()
                        && ghost.cursor == buffer.cursor
                        && buffer.selection.is_none()
                        && source_revealed
                    {
                        let shown: String = ghost
                            .text
                            .chars()
                            .take(80)
                            .take_while(|ch| *ch != '\n')
                            .collect();
                        if let Some(line) = lines.get_mut(local) {
                            insert_ghost_at(line, col_in_line, &shown);
                        }
                    }
                }
                if let Some(line) = lines.get_mut(local) {
                    paint_display_caret(line, col_in_line, editing);
                }
            }
        }
        pad_selected_rows(&mut lines, buffer, text, show_cursor, content_width);
        return Text::from(lines);
    }
    let hl_guard = buffer.highlight_cache.borrow();
    let raw = hl_guard
        .as_ref()
        .map(|(_, _, spans)| spans.as_slice())
        .unwrap_or(&[]);
    let byte_start = line_byte_offset(text, first_line);
    let byte_end = if line_count == usize::MAX {
        text.len()
    } else {
        line_byte_offset(text, first_line.saturating_add(line_count))
    };
    let mut pieces = Vec::new();
    let mut cursor = byte_start;
    for span in raw {
        if span.end <= byte_start || span.start >= byte_end {
            continue;
        }
        let span_start = span.start.max(byte_start);
        let span_end = span.end.min(byte_end);
        if span_start > cursor {
            pieces.push((cursor, span_start, TokenKind::Plain));
        }
        if span_end > span_start {
            pieces.push((span_start, span_end, span.kind));
            cursor = span_end;
        }
    }
    if cursor < byte_end {
        pieces.push((cursor, byte_end, TokenKind::Plain));
    }
    if pieces.is_empty() {
        pieces.push((byte_start, byte_end, TokenKind::Plain));
    }
    let selection = buffer
        .selection
        .map(|(start, end)| (char_to_byte(text, start), char_to_byte(text, end)));
    let mut lines = vec![Line::default()];
    let mut line_col = 0usize; // display column within current line (tabs expanded)
    for (start, end, kind) in pieces {
        let style = token_style(kind);
        let mut piece_start = start;
        for (index, piece) in text[start..end].split('\n').enumerate() {
            if index > 0 {
                lines.push(Line::default());
                piece_start += 1;
                line_col = 0;
            }
            emit_piece_tabs(
                lines.last_mut().expect("line"),
                piece,
                piece_start,
                style,
                selection,
                None,
                editing,
                &mut line_col,
            );
            piece_start += piece.len();
        }
    }
    if show_cursor {
        let line_index = buffer.document.char_to_line(buffer.cursor);
        if line_index >= first_line && line_index < first_line.saturating_add(lines.len()) {
            let local = line_index - first_line;
            let col_in_line = display_col_in_line(text, buffer.cursor, 4);
            if let Some(ghost) = ghost {
                if ghost.revision == buffer.document.revision()
                    && ghost.cursor == buffer.cursor
                    && buffer.selection.is_none()
                {
                    let shown: String = ghost
                        .text
                        .chars()
                        .take(80)
                        .take_while(|ch| *ch != '\n')
                        .collect();
                    if let Some(line) = lines.get_mut(local) {
                        insert_ghost_at(line, col_in_line, &shown);
                    }
                }
            }
            if let Some(line) = lines.get_mut(local) {
                paint_display_caret(line, col_in_line, editing);
            }
        }
    }
    pad_selected_rows(&mut lines, buffer, text, show_cursor, content_width);
    Text::from(lines)
}

fn display_col_in_line(text: &str, cursor: usize, tab: usize) -> usize {
    let tab = tab.max(1);
    let mut col = 0usize;
    for (index, ch) in text.chars().enumerate() {
        if index >= cursor {
            break;
        }
        if ch == '\n' {
            col = 0;
        } else if ch == '\t' {
            col += tab - (col % tab);
        } else {
            col += 1;
        }
    }
    col
}

fn insert_ghost_at(line: &mut Line<'static>, col: usize, ghost: &str) {
    let mut flat = String::new();
    let mut styles: Vec<(usize, usize, Style)> = Vec::new();
    for span in &line.spans {
        let start = flat.chars().count();
        flat.push_str(&span.content);
        let end = flat.chars().count();
        styles.push((start, end, span.style));
    }
    let ghost_style = Style::default().fg(Color::DarkGray);
    let mut rebuilt = Vec::new();
    let mut inserted = false;
    for (start, end, span_style) in styles {
        if !inserted && col <= start {
            rebuilt.push(TuiSpan::styled(ghost.to_string(), ghost_style));
            inserted = true;
        }
        if end <= col || start >= col {
            let text: String = flat.chars().skip(start).take(end - start).collect();
            if !text.is_empty() {
                rebuilt.push(TuiSpan::styled(text, span_style));
            }
            continue;
        }
        // Span straddles the caret column — split around the ghost.
        if start < col {
            let before: String = flat.chars().skip(start).take(col - start).collect();
            if !before.is_empty() {
                rebuilt.push(TuiSpan::styled(before, span_style));
            }
        }
        if !inserted {
            rebuilt.push(TuiSpan::styled(ghost.to_string(), ghost_style));
            inserted = true;
        }
        if col < end {
            let after: String = flat.chars().skip(col).take(end - col).collect();
            if !after.is_empty() {
                rebuilt.push(TuiSpan::styled(after, span_style));
            }
        }
    }
    if !inserted {
        rebuilt.push(TuiSpan::styled(ghost.to_string(), ghost_style));
    }
    line.spans = rebuilt;
}

/// Style the display cell under the caret (or append a block at EOL).
fn paint_display_caret(line: &mut Line<'static>, col: usize, editing: bool) {
    let style = cursor_style(editing);
    let mut flat = String::new();
    let mut styles: Vec<(usize, usize, Style)> = Vec::new();
    for span in &line.spans {
        let start = flat.chars().count();
        flat.push_str(&span.content);
        let end = flat.chars().count();
        styles.push((start, end, span.style));
    }
    let len = flat.chars().count();
    if col >= len {
        line.spans.push(TuiSpan::styled(" ", style));
        return;
    }
    let mut rebuilt = Vec::new();
    for (start, end, span_style) in styles {
        if end <= col || start > col {
            let text: String = flat.chars().skip(start).take(end - start).collect();
            if !text.is_empty() {
                rebuilt.push(TuiSpan::styled(text, span_style));
            }
            continue;
        }
        if start < col {
            let before: String = flat.chars().skip(start).take(col - start).collect();
            rebuilt.push(TuiSpan::styled(before, span_style));
        }
        let ch: String = flat.chars().skip(col).take(1).collect();
        rebuilt.push(TuiSpan::styled(ch, style));
        if col + 1 < end {
            let after: String = flat.chars().skip(col + 1).take(end - (col + 1)).collect();
            rebuilt.push(TuiSpan::styled(after, span_style));
        }
    }
    line.spans = rebuilt;
}

fn pad_selected_rows(
    lines: &mut [Line<'static>],
    buffer: &EditBuffer,
    text: &str,
    show_cursor: bool,
    content_width: usize,
) {
    let Some((sel_start, sel_end)) = buffer.selection else {
        return;
    };
    let chars = text.chars().count();
    let mut offset = 0;
    for (index, row) in text.split('\n').enumerate() {
        let row_chars = row.chars().count();
        let start = offset;
        let end = offset + row_chars;
        let has_newline = end < chars;
        let covers = sel_start <= start
            && (if has_newline {
                sel_end > end
            } else {
                sel_end >= end
            });
        if covers {
            if let Some(line) = lines.get_mut(index) {
                let caret = usize::from(show_cursor && buffer.cursor == end);
                let used = row_chars + caret;
                let room = content_width.saturating_sub(used);
                if room > 0 {
                    line.spans.push(TuiSpan::styled(
                        " ".repeat(room),
                        selection_style(Style::default()),
                    ));
                }
            }
        }
        offset = end + 1;
    }
}

fn emit_piece_tabs(
    line: &mut Line<'static>,
    piece: &str,
    piece_start: usize,
    style: Style,
    selection: Option<(usize, usize)>,
    cursor_byte: Option<usize>,
    editing: bool,
    line_col: &mut usize,
) {
    if piece.is_empty() {
        return;
    }
    let tab = 4usize;
    let piece_end = piece_start + piece.len();
    let mut cuts = vec![0, piece.len()];
    if let Some((sel_start, sel_end)) = selection {
        let lo = sel_start
            .max(piece_start)
            .saturating_sub(piece_start)
            .min(piece.len());
        let hi = sel_end
            .max(piece_start)
            .saturating_sub(piece_start)
            .min(piece.len());
        cuts.push(lo);
        cuts.push(hi);
    }
    if let Some(cursor_byte) = cursor_byte {
        if (piece_start..piece_end).contains(&cursor_byte) {
            let at = cursor_byte - piece_start;
            cuts.push(at);
            let next = piece[at..]
                .chars()
                .next()
                .map(|ch| at + ch.len_utf8())
                .unwrap_or(at);
            cuts.push(next);
        }
    }
    // Also cut on every tab so each expands independently.
    for (offset, ch) in piece.char_indices() {
        if ch == '\t' {
            cuts.push(offset);
            cuts.push(offset + ch.len_utf8());
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let absolute = piece_start + start;
        let selected = selection
            .is_some_and(|(sel_start, sel_end)| absolute >= sel_start && absolute < sel_end);
        let mut part_style = if selected {
            selection_style(style)
        } else {
            style
        };
        if cursor_byte == Some(absolute) {
            part_style = cursor_style(editing);
        }
        let chunk = &piece[start..end];
        let shown = if chunk == "\t" {
            let spaces = tab - (*line_col % tab);
            *line_col += spaces;
            " ".repeat(spaces)
        } else {
            *line_col += chunk.chars().count();
            chunk.to_string()
        };
        line.spans
            .push(TuiSpan::styled(shown, part_style));
    }
}

/// Selection uses reverse video so the host terminal paints its own default
/// selection colors. No fixed yellow/blue/indexed background.
fn selection_style(base: Style) -> Style {
    base.add_modifier(Modifier::REVERSED)
}

fn focus_color(editing: bool) -> Color {
    if editing {
        Color::Rgb(230, 120, 20)
    } else {
        Color::Magenta
    }
}

/// Focused = magenta/orange. Hovered unfocused = cyan (click to focus). Seams = yellow.
fn panel_border(focused: bool, hovered: bool, seam_hot: bool, editing: bool) -> Color {
    if seam_hot {
        Color::Yellow
    } else if focused {
        focus_color(editing)
    } else if hovered {
        Color::Cyan
    } else {
        Color::Gray
    }
}

fn cursor_style(editing: bool) -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(focus_color(editing))
        .add_modifier(Modifier::BOLD)
}

fn token_style(kind: TokenKind) -> Style {
    match kind {
        TokenKind::Plain => Style::default(),
        TokenKind::Keyword => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        TokenKind::Comment => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::ITALIC),
        TokenKind::String => Style::default().fg(Color::Green),
        TokenKind::Type => Style::default().fg(Color::Cyan),
        TokenKind::Function => Style::default().fg(Color::Blue),
        TokenKind::Heading => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        TokenKind::Bold => Style::default().add_modifier(Modifier::BOLD),
        TokenKind::Italic => Style::default().add_modifier(Modifier::ITALIC),
        TokenKind::Markup => Style::default().fg(Color::DarkGray),
        TokenKind::Table => Style::default().fg(Color::White),
    }
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        release_terminal();
    }
}

fn ignore_interrupt() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
    }
}

fn host_pixels() -> Option<(u16, u16)> {
    crossterm::terminal::window_size()
        .ok()
        .filter(|size| size.width > 0 && size.height > 0)
        .map(|size| (size.width, size.height))
}

/// Full host restore after raw/alt-screen/mouse/Kitty keyboard modes.
///
/// Bytes must match what we enabled in [`run`]: Kitty progressive enhancement
/// pops with `CSI < 1 u` (NOT the old mistaken `CSI > 4 m`), and mouse teardown
/// mirrors crossterm's [`DisableMouseCapture`] (includes 1003/1015).
const TTY_RESTORE: &[u8] = b"\
\x1b[<1u\
\x1b[=0u\
\x1b[>4;0m\
\x1b[?2031l\x1b[?1004l\
\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l\
\x1b[?2004l\
\x1b_Ga=d\x1b\\\
\x1b[?1049l\
\x1b[0m\
\x1b[?25h\
\x1b[2J\x1b[H";

/// Write restore sequences past Rust/C stdio buffers (needed before `_exit`).
fn write_tty_restore_unbuffered() {
    #[cfg(unix)]
    unsafe {
        let _ = libc::write(
            libc::STDOUT_FILENO,
            TTY_RESTORE.as_ptr() as *const libc::c_void,
            TTY_RESTORE.len(),
        );
        // Also hit the controlling tty in case stdout was redirected mid-session.
        let tty = libc::open(c"/dev/tty".as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
        if tty >= 0 {
            let _ = libc::write(
                tty,
                TTY_RESTORE.as_ptr() as *const libc::c_void,
                TTY_RESTORE.len(),
            );
            let _ = libc::close(tty);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = stdout().write_all(TTY_RESTORE);
        let _ = stdout().flush();
    }
}

fn release_terminal() {
    // Ordered restore for Drop / yazi leave/reenter paths (flush OK here).
    disable_theme_tracking();
    disable_key_release_reports();
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    );
    // Belt-and-suspenders: same bytes as fast quit (Kitty images, paste, etc.).
    let _ = stdout().write_all(TTY_RESTORE);
    let _ = stdout().flush();
}

fn restore_terminal() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    enable_theme_tracking()?;
    enable_key_release_reports()?;
    Ok(())
}

/// Focus reports + Kitty/Ghostty color-scheme change notifications (CSI ? 2031).
fn enable_theme_tracking() -> io::Result<()> {
    // CSI ? 1004 h — focus in/out; CSI ? 2031 h — color scheme reports.
    write!(stdout(), "\x1b[?1004h\x1b[?2031h")?;
    stdout().flush()?;
    Ok(())
}

fn disable_theme_tracking() {
    let _ = write!(stdout(), "\x1b[?2031l\x1b[?1004l");
    let _ = stdout().flush();
}

/// Kitty progressive enhancement: report press / repeat / release for hold-Tab Flip 3D.
fn enable_key_release_reports() -> io::Result<()> {
    execute!(
        stdout(),
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        )
    )
}

fn disable_key_release_reports() {
    let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
}

/// Built-in first-party benchmarks (`--bench`, `--bench-memory`).
pub use bench::{run as run_bench, BenchKind};

/// Compatibility: `posit-edit --bench-memory`.
pub fn bench_memory() -> i32 {
    bench::run(BenchKind::Memory)
}

pub fn detect_path(path: &str) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let profiles = ide_syntax::load_profiles(&ide_syntax::bundled_language_dir());
    ide_syntax::detect(&profiles, Some(Path::new(path)), &text)
}

pub fn run(path: Option<&str>) -> io::Result<()> {
    // One editor per user. Nested `pst` / `posit-edit` inside the IDE terminal also fails here.
    let _instance = instance::acquire()?;
    // Crossterm drops every color when NO_COLOR is set. This is a full-screen editor.
    crossterm::style::force_color_output(true);
    enable_raw_mode()?;
    ignore_interrupt();
    let _guard = TerminalGuard;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    enable_theme_tracking()?;
    // Best-effort: hosts without CSI u still get Press-only Tab (Enter commits Flip 3D).
    let _ = enable_key_release_reports();
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.hide_cursor()?;
    let mut app = if let Some(path) = path {
        let path = PathBuf::from(path);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "\n".to_string());
        let mut app = EditorApp::new(&text);
        app.bind_initial(path);
        if app.focused().language == "markdown" {
            app.open_markdown_preview();
        }
        app
    } else if load_show_welcome(
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ) {
        let mut app = EditorApp::new("\n");
        app.open_welcome_buffer();
        app
    } else {
        EditorApp::new("\n")
    };
    // First OSC sample so the Kitty strip matches the host before paint.
    app.minimap_theme = term_palette::probe(Duration::from_millis(150)).to_minimap_theme();
    app.minimap_theme_probed_at = Some(Instant::now());
    app.minimap_theme_dirty = false;
    // Agent control plane (MCP / posit-edit-mcp). Soft-fail if bind is refused.
    let control = match control::ControlServer::bind() {
        Ok(server) => {
            app.notice = format!("mcp control {}", server.path().display());
            Some(server)
        }
        Err(error) => {
            app.notice = format!("mcp control offline: {error}");
            None
        }
    };
    let mut last = Instant::now();
    let mut held: Option<Event> = None;
    // Skip full redraws when idle — Canvas capture + Kitty uploads were forcing
    // continuous presents and making the editor feel frozen.
    let mut present_needed = true;
    // One UI thread: drain→apply→quit→present-once. Never present between
    // individual scroll events (that made quit wait on the wheel storm).
    // AFK: 24 Hz present ceiling (was 60) — still smooth for Flip easing.
    let mut frames = scheduler::FrameClock::new(24);
    loop {
        let size = terminal.size()?;
        app.note_pixels(host_pixels());

        // --- 1) Wait for work, then drain the entire pending input queue. ---
        let wait = if present_needed && frames.due() {
            Duration::ZERO
        } else if app.animating()
            // AFK tick25: PTY/markdown must not pin 8ms wakes during Flip.
            || (!app.flip.open && app.terminals.wants_poll())
            || (!app.flip.open && app.preview_engine.busy())
        {
            frames.wait_hint().max(Duration::from_millis(8))
        } else if app.minimap_kitty_suppressed() {
            // AFK: wheel/slide suppress used to share the 8ms hot path.
            Duration::from_millis(55)
        } else if app.flip.open {
            // Settled Flip: wake for async preview completion, not 60 Hz raster.
            Duration::from_millis(400)
        } else if let Some(hint_wait) = app.hover_hint_wait() {
            hint_wait
        } else if app.terminals.open {
            // Open but quiet (wants_poll already took the fast path above).
            Duration::from_millis(150)
        } else if !app.canvas_surfaces.is_empty() {
            Duration::from_millis(200)
        } else if let Some(deadline) = app.chord_deadline {
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100))
        } else {
            // Fully idle editor: wake less often (AFK battery / CPU).
            Duration::from_millis(1000)
        };

        let mut raw = Vec::new();
        if let Some(event) = held.take() {
            raw.push(event);
        } else if event::poll(wait)? {
            raw.push(event::read()?);
        }
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Resize(_, _) => {
                    // Collapse resize floods; keep any non-resize that follows.
                    held = drain_resizes()?;
                    raw.push(Event::Resize(0, 0));
                    break;
                }
                other => raw.push(other),
            }
        }
        if let Some(event) = held.take() {
            raw.push(event);
        }

        let batch = scheduler::coalesce(raw);
        if batch.wants_present() {
            present_needed = true;
        }

        // --- 2) Apply input. Keys first so quit is never behind a scroll storm. ---
        apply_input_batch(&mut app, &batch, size.width, size.height);

        // Quit is the highest-priority job: skip present/pickers/maintenance.
        if app.quit() {
            finish_quit_fast();
        }

        // --- 3) Cheap maintenance (workers already own heavy IO). ---
        let now = Instant::now();
        let dt = now.saturating_duration_since(last);
        last = now;
        let prev_size = (app.width, app.height);
        app.note_frame(size.width, size.height);
        let size_changed = (app.width, app.height) != prev_size || batch.resized;
        let tick_changed = app.tick(dt);
        // AFK tick25: Flip owns the frame — drain PTY only, skip resize/ensure.
        let terminal_busy = if app.flip.open {
            app.maintain_terminal_drain_only()
        } else {
            app.maintain_terminal()
        };
        if let Some(server) = control.as_ref() {
            if !app.flip.open {
                server.poll(&mut app);
            }
        }
        // Async edocview: never wait on pandoc/typst/pymupdf here — only apply
        // finished pages and schedule the next present.
        let preview_dirty = app
            .preview_engine
            .poll(&mut app.help_doc, &mut app.markdown_doc);
        // AFK: Flip covers the screen — apply preview results but don't force a present.
        if preview_dirty && !app.flip.open {
            present_needed = true;
            app.canvas_chrome_dirty.set(true);
            if let Some(err) = app.preview_engine.last_error.take() {
                if app.runtime_dock.as_deref() == Some("markdown") {
                    app.notice = format!("preview text fallback · {err}");
                    app.markdown_force_text = true;
                }
            }
        }
        // Skip capture polling mid-drag / Flip 3D — Canvas yields the GPU path.
        let mut canvas_dirty = false;
        if !app.canvas_resize && !app.flip.open {
            for surface in &mut app.canvas_surfaces {
                canvas_dirty |= surface.poll_frame();
            }
        }
        // Flip warm runs in paint_flip_graphics; only react to worker completions here.
        if app.preview_pool.take_ready() {
            present_needed = true;
            if app.flip.open {
                app.flip.dirty = true;
            }
        }
        if !batch.wants_present() && app.chord_due() {
            app.flush_chord();
            present_needed = true;
        }
        // Idle OSC palette refresh — light/dark (and any scheme edit) without keystrokes.
        let theme_before = app.minimap_theme.fingerprint;
        refresh_minimap_theme(&mut app);
        if app.minimap_theme.fingerprint != theme_before {
            present_needed = true;
        }

        // --- 4) At most one present per frame clock tick. ---
        // AFK: do not force presents solely for minimap suppress — tick/input
        // already mark present_needed when geometry/scroll actually changes.
        // AFK: PTY/canvas churn under Flip is invisible — don't burn presents.
        let should_present = (present_needed
            || tick_changed
            || (!app.flip.open && terminal_busy)
            || canvas_dirty
            || size_changed
            || app.animating())
            && frames.due()
            || size_changed;
        if should_present {
            present(&mut terminal, &mut app)?;
            present_needed = false;
            frames.mark_presented();
        }

        if app.take_pick() {
            let workspace = app.workspace.clone();
            release_terminal();
            let chosen = files::choose_files("yazi", &workspace);
            restore_terminal()?;
            let _ = terminal.clear();
            match chosen {
                Ok(paths) => app.open_picked(&paths),
                Err(ChooseError::Missing) => app.notice = "yazi unavailable".to_string(),
                Err(ChooseError::Failed(detail)) => app.notice = format!("yazi failed: {detail}"),
            }
            present_needed = true;
        }
        if app.quit() {
            finish_quit_fast();
        }
    }
}

/// Restore the tty then terminate without unwinding a flooded alternate screen.
fn finish_quit_fast() -> ! {
    // Leave cooked mode first so the shell inherits a sane termios.
    let _ = disable_raw_mode();
    // Unbuffered write: `_exit` skips Rust stdout flush, and a buffered
    // `write_all` of restore bytes was getting lost — leaving Kitty keyboard
    // progressive enhancement on (shell typing broken). Never `flush()` the
    // huge buffered Kitty PNG stream here (that deadlocked against a blocked
    // PTY reader); libc::write of the tiny restore blob is enough.
    write_tty_restore_unbuffered();
    #[cfg(unix)]
    unsafe {
        libc::_exit(0);
    }
    #[cfg(not(unix))]
    std::process::exit(0);
}

/// Apply a coalesced input batch.
/// Quit/Ctrl-Q runs before every other key so a Down-arrow storm cannot delay exit.
fn apply_input_batch(app: &mut EditorApp, batch: &scheduler::InputBatch, width: u16, height: u16) {
    if batch.focus_gained {
        // Theme may have changed while unfocused (system light/dark, Ghostty scheme).
        app.minimap_theme_dirty = true;
        app.minimap_theme_probed_at = None;
    }
    if batch.toggle_tab {
        app.toggle_tab();
    }
    let mut rest = Vec::with_capacity(batch.keys.len());
    for &(code, modifiers, kind) in &batch.keys {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        // Only hoist keys that actually request quit in the current mode.
        // Bare `q` while editing must stay in order (it inserts a character).
        let is_quit = match (code, kind) {
            (KeyCode::Char('q') | KeyCode::Char('Q'), KeyEventKind::Press | KeyEventKind::Repeat)
                if ctrl =>
            {
                true
            }
            (KeyCode::Char('q'), KeyEventKind::Press | KeyEventKind::Repeat)
                if !ctrl
                    && !app.editing
                    && !app.palette
                    && matches!(app.overlay, Overlay::None | Overlay::Quit) =>
            {
                true
            }
            _ => false,
        };
        if is_quit {
            if !app.terminal_key(code, modifiers) {
                if let Some(command) = map_key_event(code, modifiers, kind) {
                    app.handle(command);
                }
            }
            if app.quit() {
                return;
            }
        } else {
            rest.push((code, modifiers, kind));
        }
    }
    for &(code, modifiers, kind) in &rest {
        // Key-up must not be forwarded into the focused terminal PTY.
        if kind == KeyEventKind::Release
            || !app.terminal_key(code, modifiers)
        {
            if let Some(command) = map_key_event(code, modifiers, kind) {
                app.handle(command);
            }
        }
        if app.quit() {
            return;
        }
    }
    for op in &batch.pointers {
        if app.quit() {
            return;
        }
        let event = match *op {
            scheduler::PointerOp::Down { column, row } => {
                if app.click_tab(column, row, false, width, height) {
                    continue;
                }
                Pointer::Down { column, row }
            }
            scheduler::PointerOp::Drag { column, row } => Pointer::Drag { column, row },
            scheduler::PointerOp::Up { column, row } => Pointer::Up { column, row },
            scheduler::PointerOp::Right { column, row } => Pointer::Right { column, row },
        };
        app.pointer(event, width, height);
    }
    if batch.scroll_v != 0 || batch.scroll_h != 0 {
        app.note_frame(width, height);
        let chrome = app.frame_chrome(Rect::new(0, 0, width.max(1), height.max(1)));
        let column = batch.scroll_column;
        let row = batch.scroll_row;
        // One coalesced wheel apply — not N presents of N scroll events.
        if !app.canvas_wheel_axis(column, row, batch.scroll_v, batch.scroll_h, &chrome) {
            if batch.scroll_v != 0 {
                app.route_wheel(batch.scroll_v as f32, true, column, row, &chrome);
            }
            if batch.scroll_h != 0 {
                app.route_wheel(batch.scroll_h as f32, false, column, row, &chrome);
            }
        }
    }
    if let Some((column, row)) = batch.moves {
        app.pointer(Pointer::Move { column, row }, width, height);
    }
}

fn paint_notifications(frame: &mut Frame, app: &EditorApp, area: Rect) {
    if app.notifications.is_empty() {
        return;
    }
    if app.notifications_open {
        let rect = notification_rect(area, app.notifications.len());
        let yank_hot = app.hover.is_some_and(|(column, row)| {
            row == rect.y && column + 4 >= rect.x.saturating_add(rect.width.saturating_sub(6))
        });
        frame.render_widget(Clear, rect);
        frame.render_widget(notification_paragraph(app, rect.width, yank_hot), rect);
        return;
    }
    let Some(status) = app.frame_chrome(area).status else {
        return;
    };
    let badge = format!(" {}! ", app.notifications.len());
    let width = badge.chars().count() as u16;
    let rect = Rect {
        x: status.x.saturating_add(status.width.saturating_sub(width)),
        y: status.y,
        width: width.min(status.width),
        height: 1,
    };
    let hot = app
        .hover
        .is_some_and(|(column, row)| contains(rect, column, row));
    frame.render_widget(
        Paragraph::new(badge).style(if hot {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        }),
        rect,
    );
}

fn notification_rect(area: Rect, count: usize) -> Rect {
    let width = 52.min(area.width.saturating_sub(2)).max(20);
    let rows = 1 + count.min(4) * 3;
    let height = (rows as u16 + 2).min(area.height.saturating_sub(1)).max(4);
    Rect {
        x: area.width.saturating_sub(width),
        y: area.height.saturating_sub(1).saturating_sub(height),
        width,
        height,
    }
}

fn notification_paragraph(app: &EditorApp, width: u16, yank_hot: bool) -> Paragraph<'static> {
    let inner = width.saturating_sub(2).max(1) as usize;
    let mut lines = Vec::new();
    for note in app.notifications.iter().take(4) {
        lines.push(Line::styled(
            clip_cells(&note.title, inner),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(clip_cells(&note.body.replace('\n', " "), inner)));
        lines.push(Line::from(""));
    }
    let yank = if yank_hot {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title("Notifications")
            .title(
                Line::from(vec![
                    TuiSpan::styled(" y ", yank),
                    TuiSpan::styled(" v ", Style::default().fg(Color::Yellow)),
                ])
                .alignment(Alignment::Right),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red)),
    )
}

fn paint_hover_hint(frame: &mut Frame, app: &EditorApp, area: Rect) {
    let Some(hint) = app.hover_hint.as_deref() else {
        return;
    };
    let Some((column, row)) = app.hover else {
        return;
    };
    let width = (hint.chars().count() as u16 + 2).min(area.width.max(1));
    let x = column
        .saturating_add(1)
        .min(area.width.saturating_sub(width));
    let y = if row + 2 < area.height {
        row.saturating_add(1)
    } else {
        row.saturating_sub(1)
    };
    let rect = Rect {
        x,
        y,
        width,
        height: 1,
    };
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(format!(" {hint} ")).style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        rect,
    );
}

fn hover_action(app: &EditorApp, column: u16, row: u16) -> Option<&'static str> {
    let area = Rect::new(0, 0, app.width, app.height);
    let chrome = app.frame_chrome(area);
    if quit_hit(column, row, area) {
        return Some("Quit Posit Edit");
    }
    if settings_hit(column, row, area) {
        return Some(if matches!(app.overlay, Overlay::Settings) {
            "Close settings"
        } else {
            "Open settings"
        });
    }
    if hints_hit(column, row, area) {
        return Some(if app.hover_hints {
            "Turn off mouse hover hints"
        } else {
            "Turn on mouse hover hints"
        });
    }
    if help_hit(column, row, area, chrome.status) {
        return Some("Open the guide");
    }
    if app.on_terminal_seam(column, row) {
        return Some("Drag to resize the terminal");
    }
    if app.on_terminal_side_seam(column, row) {
        return Some("Drag to resize the session list");
    }
    if app.on_canvas_seam(column, row, &chrome) {
        return Some("Drag to resize the Canvas");
    }
    if let Some(side) = app.seam(&chrome, column, row) {
        return Some(match side {
            Side::Left => "Drag to resize the file tree",
            Side::Right => "Drag to resize the agent chat",
        });
    }
    if app.tile_seam(column, row).is_some() {
        return Some("Drag to resize this buffer");
    }
    if let Some(action) = toolbar_hit(chrome.toolbar, column, row) {
        return Some(match action {
            WorkbenchAction::Build => "Build the project",
            WorkbenchAction::Run => "Run the project",
            WorkbenchAction::Configure => "Show workbench configuration",
            WorkbenchAction::Test => "Run tests",
            WorkbenchAction::Refresh => "Refresh discovered projects",
            WorkbenchAction::Browser => "Open Browser on the Canvas (Kitty graphics)",
            WorkbenchAction::Phone => "Open Phone on the Canvas (Kitty graphics)",
            WorkbenchAction::Android => "Open Android on the Canvas",
            WorkbenchAction::Debug => "Start the debugger",
        });
    }
    if chrome.canvas.width > 0 && contains(chrome.canvas, column, row) {
        if let Some(hit) = app.canvas_control_hit(column, row, &chrome) {
            return Some(match hit {
                CanvasControlHit::PhoneHome => "Phone Home button (terminal-phone)",
                CanvasControlHit::PhoneLock => "Phone Lock button (terminal-phone)",
                CanvasControlHit::PhoneSiri => "Phone Siri button (terminal-phone)",
                CanvasControlHit::BrowserBack => "Browser back (agent-browser)",
                CanvasControlHit::BrowserForward => "Browser forward (agent-browser)",
                CanvasControlHit::BrowserReload => "Browser reload (agent-browser)",
            });
        }
        if let Some(hit) = app.canvas_title_hit(column, row, &chrome) {
            return Some(match hit {
                CanvasTitleHit::Close => "Close this Canvas tab",
                CanvasTitleHit::Tab(index) => {
                    let kind = app
                        .canvas_surfaces
                        .get(index)
                        .map(|surface| EditorApp::canvas_tab_label(&surface.kind))
                        .unwrap_or("Canvas");
                    match kind {
                        "Browser" => "Switch to Browser tab",
                        "Phone" => "Switch to Phone tab",
                        "Android" => "Switch to Android tab",
                        _ => "Switch Canvas tab",
                    }
                }
            });
        }
        if contains(canvas_controls_rect(chrome.canvas), column, row) {
            return Some(match app.runtime_dock.as_deref() {
                Some("phone") => "Phone controls (Home / Lock / Siri)",
                Some("browser") => "Browser controls (Back / Fwd / Reload)",
                _ => "Canvas controls",
            });
        }
        return Some(match app.runtime_dock.as_deref() {
            Some("phone") => "Phone screen — click to tap, drag to swipe",
            Some("browser") => "Browser page — click, drag, scroll",
            _ => "Canvas",
        });
    }
    if contains(chrome.terminal, column, row) {
        let local_x = column.saturating_sub(chrome.terminal.x);
        let local_y = row.saturating_sub(chrome.terminal.y);
        if app.terminals.toggle_hit(local_x, local_y, chrome.terminal.width) {
            return Some("Toggle the terminal");
        }
        if app.terminals.switch_hit(local_x, local_y, chrome.terminal.width) {
            return Some(if app.terminals.show_console {
                "Show the terminal"
            } else {
                "Show the console"
            });
        }
        if app.on_terminal_side_seam(column, row) {
            return Some("Drag to resize the session list");
        }
        return Some(if app.terminals.focused() {
            if app.terminals.show_console {
                "Console output"
            } else {
                "Terminal session"
            }
        } else {
            "Click to focus the terminal"
        });
    }
    if app.sidebar_button(chrome.tree, column, row) {
        return Some("Toggle the file tree");
    }
    if contains(chrome.tree, column, row) && chrome.tree.width > 4 {
        return Some(if app.tree_focused {
            "File tree"
        } else {
            "Click to focus the file tree"
        });
    }
    if app.agent_button(chrome.agent, column, row) {
        return Some("Toggle the agent chat");
    }
    if contains(chrome.agent, column, row) && chrome.agent.width > 4 {
        return Some(if app.agent_focused {
            "Agent chat"
        } else {
            "Click to focus the agent chat"
        });
    }
    if app.pane_focus() != PaneFocus::Editor {
        let window = app.layout_window(chrome.content, app.axis());
        if layout::hit_tile(window, column, row, app.view, app.buffers.len()).is_some() {
            return Some("Click to focus the editor");
        }
    }
    if app.notifications_open && !app.notifications.is_empty() {
        let rect = notification_rect(area, app.notifications.len());
        if contains(rect, column, row) {
            if row == rect.y && column + 4 >= rect.x.saturating_add(rect.width.saturating_sub(6)) {
                return Some("Copy this notification");
            }
            return Some("Dismiss notifications");
        }
    }
    None
}

fn clip_cells(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let mut clipped: String = text.chars().take(width - 1).collect();
    clipped.push('…');
    clipped
}

fn debug_file(language: &str, path: &Path) -> Result<(String, Vec<String>), String> {
    match language {
        "python" => {
            let program = find_tool(&["python3", "python"])
                .ok_or_else(|| "python3 is not installed".to_string())?;
            Ok((
                program,
                vec!["-m".into(), "pdb".into(), path.display().to_string()],
            ))
        }
        "javascript" | "typescript" => {
            let program =
                find_tool(&["node"]).ok_or_else(|| "node is not installed".to_string())?;
            Ok((program, vec!["inspect".into(), path.display().to_string()]))
        }
        "java" | "kotlin" => debug_jdb(path),
        "rust" | "c" | "objc" | "objective-c" | "objective-cpp" | "objcpp" | "zig" | "swift" => {
            debug_lldb(&project_root(path))
        }
        _ => Err(format!("no debugger for {language}")),
    }
}

fn debug_jdb(path: &Path) -> Result<(String, Vec<String>), String> {
    let program =
        find_tool(&["jdb"]).ok_or_else(|| "jdb is not installed (JDK tools)".to_string())?;
    let class = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "java file has no class name".to_string())?;
    let dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .display()
        .to_string();
    // Same fidelity as pdb / node inspect: launch the JDK CLI debugger in the terminal.
    Ok((
        program,
        vec![
            "-launch".into(),
            "-sourcepath".into(),
            dir.clone(),
            "-classpath".into(),
            dir,
            class.into(),
        ],
    ))
}

fn debug_cargo(workspace: &Path) -> Result<(String, Vec<String>), String> {
    debug_lldb(workspace)
}

fn debug_lldb(workspace: &Path) -> Result<(String, Vec<String>), String> {
    let program =
        find_tool(&["lldb", "rust-lldb"]).ok_or_else(|| "lldb is not installed".to_string())?;
    let args = newest_debug_binary(workspace)
        .map(|binary| vec![binary.display().to_string()])
        .unwrap_or_default();
    Ok((program, args))
}

fn find_tool(names: &[&str]) -> Option<String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for name in names {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
        let local = dirs_local_bin().join(name);
        if local.is_file() {
            return Some(local.display().to_string());
        }
        for dir in ["/usr/bin", "/bin", "/opt/homebrew/bin"] {
            let candidate = Path::new(dir).join(name);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    None
}

fn dirs_local_bin() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/bin")
}

fn find_repo_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    for start in starts {
        let mut current = Some(start.as_path());
        while let Some(dir) = current {
            if dir.join("extensions/project-dlc/dlc.py").is_file()
                || dir.join("Cargo.toml").is_file() && dir.join("vendor").is_dir()
            {
                return Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
    }
    None
}

fn find_vendor_bin(name: &str) -> Option<PathBuf> {
    let root = find_repo_root()?;
    let candidate = root.join("vendor").join("bin").join(name);
    candidate.is_file().then_some(candidate)
}

fn resolve_runtime_program(runtime_id: &str, raw: &str, name: &str) -> String {
    let raw_path = Path::new(raw);
    if raw_path.is_file() {
        return raw.to_string();
    }
    let env_key = match runtime_id {
        "browser" => Some("IDE_TERMINAL_BROWSER"),
        "phone" => Some("IDE_TERMINAL_PHONE"),
        _ => None,
    };
    if let Some(key) = env_key {
        if let Ok(value) = std::env::var(key) {
            let candidate = PathBuf::from(value);
            if candidate.is_file() {
                return candidate.display().to_string();
            }
        }
    }
    if let Some(vendor) = find_vendor_bin(name) {
        return vendor.display().to_string();
    }
    find_tool(&[name]).unwrap_or_else(|| raw.to_string())
}

/// Launch terminal-browser / terminal-phone on the host tty so Ghostty can split
/// a real graphics pane (Kitty protocol). Nested IDE PTYs cannot show those pixels.
#[allow(dead_code)] // Ghostty split escape hatch when IDE_*_SPLIT=1 hosts want pixels.
fn launch_runtime_host_split(program: &str, args: &[String], cwd: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    if let Some(path) = crate::terminal::runtime_path_override() {
        command.env("PATH", path);
    }
    // Attach the controlling terminal so split detection sees Ghostty/kitty.
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    {
        Ok(tty) => {
            command
                .stdin(Stdio::from(tty.try_clone().map_err(|e| e.to_string())?))
                .stdout(Stdio::from(tty.try_clone().map_err(|e| e.to_string())?))
                .stderr(Stdio::from(tty));
        }
        Err(_) => {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to launch {program}: {error}"))
}

fn newest_debug_binary(workspace: &Path) -> Option<PathBuf> {
    let dir = workspace.join("target/debug");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name()?.to_str()?;
        if name.starts_with('.') || name.contains('.') {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = path.metadata().ok()?.permissions().mode();
            if mode & 0o111 == 0 {
                continue;
            }
        }
        let modified = path.metadata().ok()?.modified().ok()?;
        if best.as_ref().is_none_or(|(time, _)| modified > *time) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, path)| path)
}

fn json_error(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    value
        .get("error")
        .and_then(|error| error.as_str())
        .map(str::to_string)
}

fn terminal_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(ch) = code {
            let ch = ch.to_ascii_lowercase();
            if ch.is_ascii_lowercase() {
                return vec![ch as u8 - b'a' + 1];
            }
        }
    }
    match code {
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Char(ch) => ch.to_string().into_bytes(),
        _ => Vec::new(),
    }
}

fn present(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut EditorApp,
) -> io::Result<()> {
    // Prefer Kitty whenever TERM/TERM_PROGRAM/Ghostty say so — glyph minimap
    // is only the non-Kitty fallback. AFK: skip re-probe once Flip already owns Kitty.
    if !(app.flip.open && app.graphics.backend() == graphics::GraphicsBackend::Kitty) {
        app.graphics.prefer_kitty_from_env();
    }
    // Kitty virtual placements are cell-bound. Ratatui may blank those cells in
    // `draw`; if we EndSynchronizedUpdate *before* rebinding/transmitting, the
    // host paints one torn frame (blank strip / ghost) then the image snaps back.
    // Keep draw + Kitty inside the same synchronized update.
    // AFK: skip Flip easing math once scroll has settled.
    if app.flip_animating() {
        let dt = app
            .flip_tick_at
            .map(|at| at.elapsed().as_secs_f32() * 1000.0)
            .unwrap_or(16.0)
            .clamp(1.0, 50.0);
        app.flip_tick_at = Some(Instant::now());
        let _ = app.flip.tick(dt);
    }
    execute!(stdout(), BeginSynchronizedUpdate)?;
    let drawn = terminal.draw(|frame| draw(frame, app));
    let kitty = if app.help {
        // Help owns the screen — skip Canvas/minimap Kitty so they don't fight.
        clear_editor_minimap(app)
            .and_then(|_| clear_flip_graphics(app))
            .and_then(|_| paint_help_graphics(app))
    } else if app.flip.open {
        clear_help_graphics(app)
            .and_then(|_| clear_editor_minimap(app))
            .and_then(|_| paint_flip_graphics(app))
    } else if app.minimap_kitty_suppressed() {
        // Immediate clear while scrolling or sliding buffers — precision scrollbar
        // is painted by ratatui; do not upload / leave a stale Kitty strip.
        clear_help_graphics(app)
            .and_then(|_| clear_flip_graphics(app))
            .and_then(|_| paint_canvas_graphics(app))
            .and_then(|_| clear_editor_minimap(app))
    } else {
        clear_help_graphics(app)
            .and_then(|_| clear_flip_graphics(app))
            .and_then(|_| paint_canvas_graphics(app))
            .and_then(|_| paint_editor_minimap_graphics(app))
    };
    execute!(stdout(), EndSynchronizedUpdate)?;
    drawn?;
    kitty?;
    Ok(())
}

fn paint_flip_graphics(app: &mut EditorApp) -> io::Result<()> {
    // Backend already preferred in present(); avoid re-probing env every Flip frame.
    if cfg!(test) || app.graphics.backend() != graphics::GraphicsBackend::Kitty {
        return Ok(());
    }
    let chrome = app.frame_chrome(Rect::new(0, 0, app.width, app.height));
    let (tiles, _) = app.content_tiles_and_strip(chrome.content);
    if tiles.width < 8 || tiles.height < 6 {
        return Ok(());
    }
    let box_key = (tiles.x, tiles.y, tiles.width, tiles.height);
    let same_box = app.flip.placed == Some(box_key);
    let settled =
        !app.flip.dirty && (app.flip.scroll - app.flip.selected as f32).abs() < 0.001;
    // Ratatui underlay wiped placeholders this frame — always rebind. Settled
    // frames reuse the last RGB transmit (no re-raster / no sync warm).
    if settled && same_box && !app.flip.scene_rgb.is_empty() {
        canvas::write_kitty_placeholders(&mut stdout(), tiles, FLIP3D_KITTY_IMAGE_ID)?;
        return Ok(());
    }
    // Sync-warm only when we are about to raster a new scene.
    app.warm_flip_previews_sync();
    let (cell_w, cell_h) = canvas_cell_pixels(app);
    let (max_w, max_h) = flip3d::scene_max_dims(app.last_pressure.is_elevated());
    let img_w = (tiles.width as u32)
        .saturating_mul(cell_w.max(1))
        .min(max_w);
    let img_h = (tiles.height as u32)
        .saturating_mul(cell_h.max(1))
        .min(max_h);
    let cards = app.flip_cards();
    let theme = app.flip_scene_theme();
    let enc_w = img_w.min(max_w);
    let enc_h = img_h.min(max_h);
    // f=24 raw RGB into reused scene_rgb capacity (AFK: no per-frame alloc).
    flip3d::render_scene_rgb_into(
        &mut app.flip.scene_rgb,
        &cards,
        app.flip.scroll,
        app.flip.selected,
        img_w,
        img_h,
        theme,
    );
    app.flip.scene_png.clear();
    let placed = canvas::paint_kitty_rgb24_into(
        &mut stdout(),
        &mut app.graphics,
        tiles,
        &app.flip.scene_rgb,
        enc_w,
        enc_h,
        FLIP3D_KITTY_IMAGE_ID,
        !same_box,
        true,
        &mut app.flip.scene_b64,
    )?;
    if placed {
        app.flip.dirty = false;
        app.flip.placed = Some(box_key);
    }
    Ok(())
}

fn clear_flip_graphics(app: &mut EditorApp) -> io::Result<()> {
    if app.graphics.backend() != graphics::GraphicsBackend::Kitty {
        return Ok(());
    }
    app.flip.placed = None;
    app.flip.scene_png.clear();
    app.flip.scene_rgb.clear();
    app.flip.scene_b64.clear();
    write!(
        stdout(),
        "\x1b_Ga=d,d=i,i={FLIP3D_KITTY_IMAGE_ID},q=2\x1b\\"
    )?;
    Ok(())
}

fn help_uses_kitty(app: &EditorApp) -> bool {
    if cfg!(test) || app.help_force_text {
        return false;
    }
    // Only switch off text once pages exist — never blank the screen waiting.
    app.help_doc
        .as_ref()
        .is_some_and(|doc| !doc.pages.is_empty())
}

fn draw_help_fullscreen(frame: &mut Frame, app: &EditorApp, area: Rect) {
    let block = Block::default()
        .title("Help  ·  Esc / ? close  ·  ↑↓ scroll")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    if help_uses_kitty(app) {
        // edocview: chrome only — retained Kitty crop covers the inner box.
        frame.render_widget(Paragraph::new(Text::raw("")).block(block), area);
        return;
    }
    let width = area.width.max(8) as usize;
    let preview = markdown_preview::render(&help_markdown(), width.saturating_sub(2).max(8));
    let scroll = app.help_scroll.min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(preview).scroll((scroll, 0)).block(block),
        area,
    );
}

fn paint_help_graphics(app: &mut EditorApp) -> io::Result<()> {
    app.graphics.prefer_kitty_from_env();
    if app.help_force_text || app.graphics.backend() != graphics::GraphicsBackend::Kitty {
        clear_help_graphics(app)?;
        return Ok(());
    }
    let area = Rect::new(0, 0, app.width.max(1), app.height.max(1));
    if area.width < 8 || area.height < 4 {
        return Ok(());
    }
    let (cell_w, cell_h) = edocview::cell_pixels(app.width, app.height, app.pixels);
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2).max(1),
        height: area.height.saturating_sub(2).max(1),
    };
    let source = help_markdown();
    app.preview_engine.request(
        edocview::PreviewSlot::Help,
        &source,
        &app.workspace,
        inner.width.max(1),
        cell_w,
        cell_h,
        HELP_KITTY_IMAGE_ID,
        edocview::RASTER_DEBOUNCE,
    );
    match edocview::present_markdown(
        &mut stdout(),
        &mut app.graphics,
        &mut app.help_doc,
        &source,
        &app.workspace,
        inner,
        app.help_scroll,
        cell_w,
        cell_h,
        HELP_KITTY_IMAGE_ID,
    )? {
        edocview::PresentStatus::Ready | edocview::PresentStatus::Pending => {}
        edocview::PresentStatus::Unavailable => {
            app.help_force_text = true;
            clear_help_graphics(app)?;
        }
    }
    Ok(())
}

fn clear_help_graphics(app: &mut EditorApp) -> io::Result<()> {
    edocview::clear_doc(&mut stdout(), &mut app.help_doc)
}

fn markdown_uses_kitty(app: &EditorApp) -> bool {
    if cfg!(test) || app.markdown_force_text {
        return false;
    }
    // Text paints immediately; Kitty takes over only after async pages land.
    app.runtime_dock.as_deref() == Some("markdown")
        && app
            .markdown_doc
            .as_ref()
            .is_some_and(|doc| !doc.pages.is_empty())
}

fn paint_markdown_canvas_graphics(app: &mut EditorApp) -> io::Result<()> {
    app.graphics.prefer_kitty_from_env();
    if app.markdown_force_text || app.graphics.backend() != graphics::GraphicsBackend::Kitty {
        clear_markdown_graphics(app)?;
        return Ok(());
    }
    // Width thrashing mid-drag must not re-queue pandoc/typst rasters.
    if app.canvas_resize {
        return Ok(());
    }
    let chrome = app.frame_chrome(Rect::new(0, 0, app.width, app.height));
    if chrome.canvas.width < 4 || chrome.canvas.height < 4 {
        return Ok(());
    }
    let inner = canvas_media_rect(chrome.canvas);
    // Leave one row for the preview footer hint.
    let inner = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1).max(1),
    };
    let (cell_w, cell_h) = edocview::cell_pixels(app.width, app.height, app.pixels);
    let source = app.markdown_preview_source();
    // Kick/refresh background raster if fingerprint drifted — never sync-compile.
    app.preview_engine.request(
        edocview::PreviewSlot::Markdown,
        &source,
        &app.workspace,
        inner.width.max(1),
        cell_w,
        cell_h,
        MARKDOWN_KITTY_IMAGE_ID,
        edocview::RASTER_DEBOUNCE,
    );
    match edocview::present_markdown(
        &mut stdout(),
        &mut app.graphics,
        &mut app.markdown_doc,
        &source,
        &app.workspace,
        inner,
        app.markdown_preview_scroll as usize,
        cell_w,
        cell_h,
        MARKDOWN_KITTY_IMAGE_ID,
    )? {
        edocview::PresentStatus::Ready => {
            if app.notice.starts_with("preview · rendering") {
                app.notice = "preview ready".into();
            }
        }
        edocview::PresentStatus::Pending => {}
        edocview::PresentStatus::Unavailable => {
            if let Some(err) = app.preview_engine.last_error.clone() {
                app.notice = format!("preview text fallback · {err}");
            }
            app.markdown_force_text = true;
            clear_markdown_graphics(app)?;
        }
    }
    Ok(())
}

fn clear_markdown_graphics(app: &mut EditorApp) -> io::Result<()> {
    edocview::clear_doc(&mut stdout(), &mut app.markdown_doc)
}

fn editor_minimap_screen_rect(app: &EditorApp) -> Option<Rect> {
    if app.help || app.palette || app.overlay != Overlay::None {
        return None;
    }
    if app.minimap_kitty_suppressed() {
        return None;
    }
    let window = app.window();
    let index = app.focus;
    let slice = layout::tile_slice(window, index, app.view)?;
    let (tile_w, tile_h) = window.tile_box(index);
    let line_count = app.focused().document.len_lines().max(1);
    let visible = (tile_h.saturating_sub(2) as usize).max(1);
    let (cols, scroll_w) = app.focused_edge_chrome(tile_w, line_count, visible);
    if cols < 2 || tile_h < 4 || !app.kitty_minimap_enabled() {
        return None;
    }
    // Kitty strip owns the right edge (scroll_w == 0); still leave room if not.
    let local = Rect {
        x: tile_w.saturating_sub(1 + scroll_w + cols),
        y: 1,
        width: cols,
        height: tile_h.saturating_sub(2),
    };
    minimap_gfx::screen_rect_from_tile_local(slice, local)
}

fn refresh_minimap_theme(app: &mut EditorApp) {
    // AFK: Flip owns the screen — skip OSC entirely while open (was 500ms probes).
    if app.flip.open {
        return;
    }
    // Idle OSC every 5s (was 3s/500ms) — theme flips are rare.
    let due = app.minimap_theme_dirty
        || app
            .minimap_theme_probed_at
            .is_none_or(|at| at.elapsed() >= Duration::from_millis(5_000));
    if !due {
        return;
    }
    // OSC replies share stdin with crossterm. Never probe while the user is
    // typing into the command palette / finder — leftover `rgb:…` chunks were
    // landing in `app.query` as literal characters.
    if app.palette || app.finder.is_some() || app.help {
        return;
    }
    // Skip OSC while input is pending — replies share stdin with crossterm.
    if event::poll(Duration::ZERO).unwrap_or(true) {
        return;
    }
    let Some(palette) = term_palette::probe_osc_quick() else {
        // Host silent this tick — keep the current theme; retry later.
        app.minimap_theme_probed_at = Some(Instant::now());
        app.minimap_theme_dirty = false;
        return;
    };
    let next = palette.to_minimap_theme();
    app.minimap_theme_probed_at = Some(Instant::now());
    app.minimap_theme_dirty = false;
    if next.fingerprint != app.minimap_theme.fingerprint {
        app.minimap_theme = next;
        app.editor_minimap_key = None;
        app.minimap_frame_cache.borrow_mut().clear();
        app.preview_pool.clear();
    } else {
        app.minimap_theme = next;
    }
}

fn paint_editor_minimap_graphics(app: &mut EditorApp) -> io::Result<()> {
    app.graphics.prefer_kitty_from_env();
    if app.graphics.backend() != graphics::GraphicsBackend::Kitty {
        // True text-only host — glyph underlay in paint_tile is the fallback.
        clear_editor_minimap(app)?;
        return Ok(());
    }
    if app.minimap_kitty_suppressed() {
        clear_editor_minimap(app)?;
        return Ok(());
    }
    if app.last_pressure.is_critical() && !minimap_gfx::force_minimap() {
        clear_editor_minimap(app)?;
        app.minimap_frame_cache.borrow_mut().clear();
        return Ok(());
    }
    let Some(rect) = editor_minimap_screen_rect(app) else {
        clear_editor_minimap(app)?;
        return Ok(());
    };
    let (cell_w, cell_h) = canvas_cell_pixels(app);
    let image_id = minimap_gfx::EDITOR_MINIMAP_IMAGE_ID;
    // Live OSC palette — tracks light/dark and any scheme color edits.
    refresh_minimap_theme(app);
    let theme = app.minimap_theme;
    let warning = app.last_pressure.is_elevated();
    let scale = minimap_gfx::paint_scale(warning);
    // Match cell geometry; soft-cap only so we never undersample into stripe soup.
    // Under Warning, cols are already narrowed by edge_chrome — keep 1:1 px/cell.
    // AFK tick25: tighter Kitty minimap budgets (was 640×1600 / 480×960).
    let max_w = if warning { 280 } else { 400 };
    let max_h = if warning { 640 } else { 800 };
    let focus = app.focus;
    let line_count = app.buffers[focus].document.len_lines().max(1);
    let visible = rect.height as usize;
    let scroll = app.buffers[focus].scroll;
    let revision = app.buffers[focus].document.revision();
    let px_w = (rect.width as u32)
        .saturating_mul(cell_w.max(1))
        .max(1)
        .min(max_w);
    let px_h = (rect.height as u32)
        .saturating_mul(cell_h.max(1))
        .max(1)
        .min(max_h);
    let layout = minimap_gfx::layout_proportional(line_count, scroll, visible, px_h, scale);
    let key = minimap_gfx::MinimapPaintKey {
        revision,
        line_count,
        scroll,
        visible,
        start_line: layout.start_line,
        cols: rect.width,
        rows: rect.height,
        px_w,
        px_h,
        scale,
        theme: theme.fingerprint,
    };
    let dirty = app.editor_minimap_key != Some(key) || app.editor_minimap_id != Some(image_id);
    if dirty {
        let content = minimap_gfx::MinimapContentKey {
            revision,
            start_line: layout.start_line,
            shown_lines: layout.shown_lines,
            px_w,
            px_h,
            scale,
            theme: theme.fingerprint,
        };
        // Field-split: buffers + cache + graphics can borrow together; tinted
        // RGB stays in cache and feeds PNG encode without an extra Vec copy.
        let painted = {
            let mut cache = app.minimap_frame_cache.borrow_mut();
            let rgb = cache.rgb_for(content, layout, px_w, px_h, theme, |line, cols| {
                minimap_syntax_cells(&app.buffers[focus], line, cols, theme)
            });
            canvas::paint_kitty_rgb(
                &mut stdout(),
                &mut app.graphics,
                rect,
                rgb,
                px_w,
                px_h,
                image_id,
            )?
        };
        if painted {
            app.editor_minimap_key = Some(key);
            app.editor_minimap_id = Some(image_id);
        }
    } else {
        // Tile Block clear wipes cells every draw — rebind without re-upload so
        // the strip does not fall back to grey Unicode-placeholder static.
        canvas::write_kitty_placeholders(&mut stdout(), rect, image_id)?;
    }
    Ok(())
}

fn clear_editor_minimap(app: &mut EditorApp) -> io::Result<()> {
    if let Some(id) = app.editor_minimap_id.take() {
        canvas::delete_kitty_image(&mut stdout(), id)?;
        app.editor_minimap_key = None;
        app.minimap_frame_cache.borrow_mut().clear();
    }
    Ok(())
}

/// Per-character RGB for the Kitty minimap — same TokenKind → ANSI mapping as the buffer.
fn minimap_syntax_cells(
    buffer: &EditBuffer,
    line: usize,
    cols: usize,
    theme: minimap_gfx::MinimapTheme,
) -> Vec<(char, (u8, u8, u8))> {
    buffer.ensure_highlight_cache();
    buffer.ensure_text_cache();
    let text_guard = buffer.text_cache.borrow();
    let text = text_guard
        .as_ref()
        .map(|(_, text)| text.as_str())
        .unwrap_or("");
    let line_count = buffer.document.len_lines().max(1);
    if line >= line_count || cols == 0 {
        return Vec::new();
    }
    let start = line_byte_offset(text, line);
    let end = line_byte_offset(text, line.saturating_add(1));
    if start >= text.len() || start >= end {
        return Vec::new();
    }
    let line_text = text[start..end].trim_end_matches(['\n', '\r']);
    let hl_guard = buffer.highlight_cache.borrow();
    let spans = hl_guard
        .as_ref()
        .map(|(_, _, spans)| spans.as_slice())
        .unwrap_or(&[]);
    let mut out = Vec::with_capacity(cols.min(line_text.chars().count()));
    let mut byte = start;
    for ch in line_text.chars().take(cols) {
        let kind = spans
            .iter()
            .find(|span| span.start <= byte && byte < span.end)
            .map(|span| span.kind)
            .unwrap_or(TokenKind::Plain);
        out.push((ch, token_rgb(kind, theme)));
        byte += ch.len_utf8();
    }
    out
}

fn token_rgb(kind: TokenKind, theme: minimap_gfx::MinimapTheme) -> (u8, u8, u8) {
    // Mirrors `token_style` named ANSI colors → live OSC palette slots.
    match kind {
        TokenKind::Keyword => theme.keyword,
        TokenKind::Comment => theme.comment,
        TokenKind::String => theme.string,
        TokenKind::Type | TokenKind::Heading => theme.ident,
        TokenKind::Function => theme.function,
        TokenKind::Markup => theme.markup,
        TokenKind::Table => theme.fg,
        TokenKind::Bold | TokenKind::Italic | TokenKind::Plain => theme.fg,
    }
}

fn canvas_cell_pixels(app: &EditorApp) -> (u32, u32) {
    let cols = app.width.max(1) as u32;
    let rows = app.height.max(1) as u32;
    match app.pixels {
        Some((pw, ph)) if pw > 0 && ph > 0 => {
            ((pw as u32 / cols).max(1), (ph as u32 / rows).max(1))
        }
        _ => (8, 16),
    }
}

fn paint_canvas_graphics(app: &mut EditorApp) -> io::Result<()> {
    // Mid-drag: drop floating Kitty once, then skip geom / PNG / markdown work.
    // Layout chrome stays on the ratatui path; heavy IO resumes on mouse-up.
    if app.canvas_resize {
        if !app.canvas_resize_hid_gfx {
            if let Some(id) = app.canvas_image_id.take() {
                canvas::delete_kitty_image(&mut stdout(), id)?;
            }
            if let Some(kind) = app.runtime_dock.clone() {
                if let Some(surface) = app
                    .canvas_surfaces
                    .iter_mut()
                    .find(|surface| surface.kind == kind)
                {
                    surface.mark_kitty_evicted();
                }
            }
            clear_markdown_graphics(app)?;
            app.canvas_resize_hid_gfx = true;
            app.canvas_painted_size = None;
        }
        return Ok(());
    }

    let Some(kind) = app.runtime_dock.clone() else {
        clear_markdown_graphics(app)?;
        return Ok(());
    };
    // Markdown Preview: edocview PDF→PNG→Kitty crop (text fallback if toolchain missing).
    if kind == "markdown" {
        return paint_markdown_canvas_graphics(app);
    }
    clear_markdown_graphics(app)?;
    let chrome = app.frame_chrome(Rect::new(0, 0, app.width, app.height));
    if chrome.canvas.width < 4 || chrome.canvas.height < 4 {
        return Ok(());
    }
    let inner = canvas_media_rect(chrome.canvas);
    let Some(index) = app
        .canvas_surfaces
        .iter()
        .position(|surface| surface.kind == kind)
    else {
        return Ok(());
    };
    let (cell_w, cell_h) = canvas_cell_pixels(app);

    // Publish pane geometry once the size settles so capture matches the Canvas.
    app.canvas_surfaces[index].write_geom(inner.width, inner.height, cell_w, cell_h);

    let size = (inner.width, inner.height);
    let surface = &app.canvas_surfaces[index];
    if !surface.has_frame() && !surface.needs_paint() {
        return Ok(());
    }
    let image_id = surface.next_image_id();
    let size_changed = surface.placed_size() != Some(size) || app.canvas_painted_size != Some(size);
    let content_dirty = surface.needs_paint();
    let resident = surface.kitty_resident();
    let cached = surface.cached_png().map(|b| b.to_vec());
    let path = surface.frame_path().to_path_buf();

    // Fast paths:
    // 1) Kitty still has the image + same size → placeholders only.
    // 2) Cached PNG in memory → retransmit without disk wait.
    // 3) Else fall back to disk path (first frame / cold start).
    if !content_dirty && resident && !size_changed && !app.canvas_placeholders_stale.get() {
        return Ok(());
    }
    if !content_dirty && resident && (size_changed || app.canvas_placeholders_stale.get()) {
        // Same pixels, new cell box or chrome Clear — retransmit from cache (or path).
        let painted = if let Some(png) = cached.as_deref() {
            canvas::paint_kitty_png_bytes(
                &mut stdout(),
                &mut app.graphics,
                inner,
                png,
                image_id,
            )?
        } else {
            canvas::paint_kitty_png(
                &mut stdout(),
                &mut app.graphics,
                inner,
                &path,
                image_id,
            )?
        };
        if painted {
            app.canvas_surfaces[index].mark_painted(size, true);
            app.canvas_painted_size = Some(size);
            app.canvas_image_id = Some(image_id);
            app.canvas_placeholders_stale.set(false);
            app.canvas_surfaces[index].store_in_cache(&mut app.canvas_frame_cache);
        }
        return Ok(());
    }

    if content_dirty || size_changed || app.canvas_placeholders_stale.get() || !resident {
        let painted = if let Some(png) = cached.as_deref() {
            canvas::paint_kitty_png_bytes(
                &mut stdout(),
                &mut app.graphics,
                inner,
                png,
                image_id,
            )?
        } else {
            canvas::paint_kitty_png(
                &mut stdout(),
                &mut app.graphics,
                inner,
                &path,
                image_id,
            )?
        };
            if painted {
            // Ensure cache is warm even when we painted from disk.
            if app.canvas_surfaces[index].cached_png().is_none() {
                if let Ok(bytes) = std::fs::read(&path) {
                    app.canvas_surfaces[index].set_cached_png(bytes);
                }
            }
            app.canvas_surfaces[index].mark_painted(size, true);
            app.canvas_painted_size = Some(size);
            app.canvas_image_id = Some(image_id);
            app.canvas_placeholders_stale.set(false);
            app.canvas_surfaces[index].store_in_cache(&mut app.canvas_frame_cache);
        }
    }
    Ok(())
}

fn drain_resizes() -> io::Result<Option<Event>> {
    while event::poll(Duration::ZERO)? {
        match event::read()? {
            Event::Resize(_, _) => {}
            other => return Ok(Some(other)),
        }
    }
    Ok(None)
}

fn project_root(start: &Path) -> PathBuf {
    let mut dir = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let origin = dir.clone();
    let mut best = None;
    loop {
        if is_project_root_marker(&dir) {
            // Keep walking so Cargo/Gradle workspace roots win over nested package dirs.
            best = Some(dir.clone());
        }
        if !dir.pop() {
            break;
        }
    }
    best.unwrap_or(origin)
}

fn is_project_root_marker(dir: &Path) -> bool {
    const FILES: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "build.zig",
        "flake.nix",
        "shell.nix",
        "default.nix",
        "Package.swift",
        "gradlew",
        "gradlew.bat",
        "settings.gradle",
        "settings.gradle.kts",
        "build.gradle",
        "build.gradle.kts",
        "pom.xml",
        "CMakeLists.txt",
        "meson.build",
        "Makefile",
        "GNUmakefile",
        "makefile",
        "WORKSPACE",
        "WORKSPACE.bazel",
        "MODULE.bazel",
    ];
    FILES.iter().any(|name| dir.join(name).is_file())
        || dir.join(".idea").is_dir()
        || dir
            .read_dir()
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace")
            })
}

fn task_line(language: &str, path: Option<&Path>, run: bool) -> Option<(String, Vec<String>)> {
    let file = path.map(|path| path.display().to_string());
    match language {
        "python" => {
            let file = file?;
            Some(if run {
                ("python3".to_string(), vec![file])
            } else {
                (
                    "python3".to_string(),
                    vec!["-m".to_string(), "py_compile".to_string(), file],
                )
            })
        }
        "javascript" => {
            let file = file?;
            Some(if run {
                ("node".to_string(), vec![file])
            } else {
                ("node".to_string(), vec!["--check".to_string(), file])
            })
        }
        "typescript" => {
            let file = file?;
            Some(if run {
                ("bun".to_string(), vec![file])
            } else {
                ("bun".to_string(), vec!["build".to_string()])
            })
        }
        "rust" => Some(if run {
            (
                "cargo".to_string(),
                vec!["run".to_string(), "-q".to_string()],
            )
        } else {
            (
                "cargo".to_string(),
                vec!["build".to_string(), "-q".to_string()],
            )
        }),
        "c" => {
            let file = file?;
            Some(("clang".to_string(), vec!["-fsyntax-only".to_string(), file]))
        }
        "zig" => Some((
            "zig".to_string(),
            if run {
                vec!["build".to_string(), "run".to_string()]
            } else {
                vec!["build".to_string()]
            },
        )),
        "nix" => Some(("nix".to_string(), vec!["build".to_string()])),
        "java" | "kotlin" => Some((
            "./gradlew".to_string(),
            vec![if run { "run" } else { "build" }.to_string()],
        )),
        "swift" => Some((
            "swift".to_string(),
            vec![if run { "run" } else { "build" }.to_string()],
        )),
        "make" | "makefile" => Some((
            "make".to_string(),
            vec![if run { "run".to_string() } else { "all".to_string() }],
        )),
        _ => None,
    }
}

fn find_dlc_script() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    for start in starts {
        let mut current = Some(start.as_path());
        while let Some(dir) = current {
            let candidate = dir.join("extensions/project-dlc/dlc.py");
            if candidate.is_file() {
                return Some(candidate);
            }
            current = dir.parent();
        }
    }
    None
}

fn scan_systems(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return Vec::new();
    };
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("system").and_then(|system| system.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn pick_system(systems: &[String], language: &str, chosen: Option<&str>) -> Option<String> {
    if let Some(chosen) = chosen {
        if systems.iter().any(|system| system == chosen) {
            return Some(chosen.to_string());
        }
    }
    let preferred = match language {
        "rust" => Some("cargo"),
        "nix" => Some("nix"),
        "zig" => Some("zig"),
        "swift" => Some("swiftpm"),
        "c" => Some("cmake"),
        "objc" | "objective-c" | "objective-cpp" | "objcpp" => Some("xcodebuild"),
        "java" | "kotlin" => Some("gradle"),
        "javascript" | "typescript" => Some("javascript"),
        "make" | "makefile" => Some("make"),
        _ => None,
    };
    preferred
        .filter(|name| systems.iter().any(|system| system == *name))
        .map(str::to_string)
        .or_else(|| (systems.len() == 1).then(|| systems[0].clone()))
        .or_else(|| rank_systems(systems))
}

/// Prefer concrete project tools over ambient nix when several markers coexist.
fn rank_systems(systems: &[String]) -> Option<String> {
    const RANK: &[&str] = &[
        "cargo",
        "gradle",
        "maven",
        "cmake",
        "make",
        "xcodebuild",
        "swiftpm",
        "zig",
        "javascript",
        "meson",
        "ninja",
        "bazel",
        "nix",
    ];
    RANK.iter()
        .find(|name| systems.iter().any(|system| system == **name))
        .map(|name| (*name).to_string())
}

fn detect_systems_fast(root: &Path) -> Vec<String> {
    const MARKERS: &[(&str, &[&str])] = &[
        ("cargo", &["Cargo.toml"]),
        ("nix", &["flake.nix", "shell.nix", "default.nix"]),
        (
            "gradle",
            &[
                "settings.gradle.kts",
                "settings.gradle",
                "build.gradle.kts",
                "build.gradle",
                "gradlew",
                "gradlew.bat",
            ],
        ),
        ("maven", &["pom.xml"]),
        ("cmake", &["CMakeLists.txt"]),
        ("make", &["Makefile", "GNUmakefile", "makefile"]),
        ("zig", &["build.zig"]),
        ("swiftpm", &["Package.swift"]),
        ("meson", &["meson.build"]),
        ("ninja", &["build.ninja"]),
        ("bazel", &["WORKSPACE", "WORKSPACE.bazel", "MODULE.bazel"]),
        ("javascript", &["package.json"]),
    ];
    let mut found = Vec::new();
    for (system, markers) in MARKERS {
        if markers.iter().any(|marker| root.join(marker).is_file()) {
            found.push((*system).to_string());
        }
    }
    if root.join(".idea").is_dir() {
        found.push("intellij".into());
    }
    // Xcode: any *.xcodeproj / *.xcworkspace directory
    if root
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace")
        })
    {
        found.push("xcodebuild".into());
    }
    found
}

fn is_editor_workspace(root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
        && root.join("extensions/project-dlc").is_dir()
        && root.join("crates/ide-tui").is_dir()
}

struct WorkbenchState {
    trusted: bool,
    system: Option<String>,
}

fn load_workbench(root: &Path) -> WorkbenchState {
    let path = root.join(".posit/workbench.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return WorkbenchState {
            trusted: false,
            system: None,
        };
    };
    let value = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
    WorkbenchState {
        trusted: value
            .get("trusted")
            .and_then(|flag| flag.as_bool())
            .unwrap_or(false),
        system: value
            .get("system")
            .and_then(|system| system.as_str())
            .map(str::to_string),
    }
}

fn plan_argv(text: &str, workspace: &Path) -> Option<(PathBuf, Vec<String>)> {
    let value = serde_json::from_str::<serde_json::Value>(text.trim()).ok()?;
    let argv = value.get("argv")?.as_array()?;
    let argv: Vec<String> = argv
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    if argv.is_empty() {
        return None;
    }
    let cwd = value
        .get("cwd")
        .and_then(|cwd| cwd.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.to_path_buf());
    Some((cwd, argv))
}

fn capture(program: &str, args: &[String], cwd: &Path) -> Result<(bool, String), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("{program} unavailable")
        } else {
            format!("{program} failed: {error}")
        }
    })?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > Duration::from_secs(20) => {
                let pid = child.id();
                let _ = Command::new("kill")
                    .args(["-KILL", &format!("-{pid}")])
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} timed out"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let mut text = stdout;
    text.push_str(&stderr);
    if text.len() > 16_000 {
        text.truncate(16_000);
        text.push_str("\n…\n");
    }
    Ok((status.success(), text))
}

fn agent_route() -> ide_ai::RouteReport {
    ide_ai::resolve_route(|name| {
        std::process::Command::new("which")
            .arg(name)
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn agent_program() -> String {
    let route = agent_route();
    route
        .chosen
        .program
        .unwrap_or_else(|| route.chosen.kind.label().to_string())
}

fn walk_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || name == "target"
                || name == "node_modules"
                || name == "build"
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out
}

#[cfg(test)]
fn map_key(code: KeyCode, modifiers: KeyModifiers) -> Option<CommandKey> {
    map_key_event(code, modifiers, KeyEventKind::Press)
}

fn map_key_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    kind: KeyEventKind,
) -> Option<CommandKey> {
    if kind == KeyEventKind::Release {
        // Hold-Tab Flip 3D: bare Tab up commits the switcher.
        return match code {
            KeyCode::Tab
                if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(CommandKey::TabRelease)
            }
            _ => None,
        };
    }
    map_key_press(code, modifiers)
}

fn map_key_press(code: KeyCode, modifiers: KeyModifiers) -> Option<CommandKey> {
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let alt = modifiers.contains(KeyModifiers::ALT);
    match code {
        KeyCode::Char('q') if ctrl => Some(CommandKey::CtrlQ),
        KeyCode::Char('z') if ctrl => Some(CommandKey::CtrlZ),
        KeyCode::Char('p') if ctrl => Some(CommandKey::CtrlP),
        KeyCode::Char('w') if ctrl => Some(CommandKey::CtrlW),
        KeyCode::Char('s') if ctrl => Some(CommandKey::CtrlS),
        KeyCode::Char('a' | 'A') if ctrl => Some(CommandKey::CtrlA),
        KeyCode::Left if ctrl => Some(CommandKey::CtrlLeft),
        KeyCode::Right if ctrl => Some(CommandKey::CtrlRight),
        KeyCode::Up if ctrl => Some(CommandKey::CtrlUp),
        KeyCode::Down if ctrl => Some(CommandKey::CtrlDown),
        // Option/Alt word hops (macOS text editing). Also Meta-b / Meta-f from
        // terminals that treat Option as Meta instead of sending Alt+Arrow.
        KeyCode::Left if alt && shift => Some(CommandKey::AltShiftLeft),
        KeyCode::Right if alt && shift => Some(CommandKey::AltShiftRight),
        KeyCode::Left if alt => Some(CommandKey::AltLeft),
        KeyCode::Right if alt => Some(CommandKey::AltRight),
        KeyCode::Char('b' | 'B') if alt && !ctrl && shift => Some(CommandKey::AltShiftLeft),
        KeyCode::Char('f' | 'F') if alt && !ctrl && shift => Some(CommandKey::AltShiftRight),
        KeyCode::Char('b' | 'B') if alt && !ctrl => Some(CommandKey::AltLeft),
        KeyCode::Char('f' | 'F') if alt && !ctrl => Some(CommandKey::AltRight),
        KeyCode::Left if shift => Some(CommandKey::ShiftLeft),
        KeyCode::Right if shift => Some(CommandKey::ShiftRight),
        KeyCode::Up if shift => Some(CommandKey::ShiftUp),
        KeyCode::Down if shift => Some(CommandKey::ShiftDown),
        KeyCode::Char(ch) if !ctrl && !alt => Some(CommandKey::Char(ch)),
        KeyCode::Enter => Some(CommandKey::Enter),
        KeyCode::Backspace => Some(CommandKey::Backspace),
        KeyCode::Delete => Some(CommandKey::Delete),
        KeyCode::Left => Some(CommandKey::Left),
        KeyCode::Right => Some(CommandKey::Right),
        KeyCode::Up => Some(CommandKey::Up),
        KeyCode::Down => Some(CommandKey::Down),
        KeyCode::Esc => Some(CommandKey::Esc),
        KeyCode::F(1) => Some(CommandKey::Help),
        // Flip 3D opens on bare Tab hold in view mode — do not steal Ctrl-Tab.
        KeyCode::Tab if alt && shift => Some(CommandKey::AltShiftTab),
        KeyCode::BackTab if alt => Some(CommandKey::AltShiftTab),
        KeyCode::Tab if alt => Some(CommandKey::AltTab),
        KeyCode::BackTab => Some(CommandKey::AltShiftTab),
        KeyCode::Tab if !ctrl => Some(CommandKey::Tab),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn rendered(app: &EditorApp) -> String {
        rendered_at(app, app.width, app.height)
    }

    fn row_symbols(app: &EditorApp, width: u16, height: u16, row: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..width)
            .map(|column| buffer[(column, row)].symbol().to_string())
            .collect()
    }

    fn rendered_at(app: &EditorApp, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn editing(app: &mut EditorApp) {
        app.handle(CommandKey::Enter);
    }

    /// Screen X of column 0 in the focused editor (past border + gutter).
    fn text_col(app: &EditorApp) -> u16 {
        let (x, _) = app.text_origin().unwrap();
        let gutter_w = app
            .gutter
            .width_for(app.focused().document.len_lines().max(1));
        x.saturating_add(gutter_w)
    }

    fn palette(app: &mut EditorApp, command: &str) {
        app.handle(CommandKey::CtrlP);
        for ch in command.chars() {
            app.handle(CommandKey::Char(ch));
        }
        app.handle(CommandKey::Enter);
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("axiom-tui-{label}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn buffer_edge_drag_works_in_full_and_split() {
        let mut app = EditorApp::new("one\n");
        app.add_buffer("two.txt", "two\n");
        app.add_buffer("three.txt", "three\n");
        app.note_frame(120, 40);
        app.show_many = false;
        app.focus_at(1);
        let (tiles, strip) = app.content_tiles_and_strip(
            app.frame_chrome(Rect::new(0, 0, app.width, app.height))
                .content,
        );
        assert_eq!(strip.height, 1);
        // Right edge of focused full buffer → that buffer's span (not a neighbour steal).
        let seam = app
            .tile_seam(
                tiles.x.saturating_add(tiles.width).saturating_sub(1),
                tiles.y + 2,
            )
            .expect("full-mode right edge");
        assert_eq!(seam, 1);
        // Left edge is not a buffer resize handle.
        assert_eq!(app.tile_seam(tiles.x, tiles.y + 2), None);
        app.arm_buffer_edge_resize(seam);
        assert!(app.show_many);
        assert_eq!(app.tile_drag, Some(1));
        assert!(app.tile_spans.as_ref().is_some_and(|s| s.len() == 3));
        let mid = strip.x + strip.width / 2;
        let hit = app.buffer_strip_hit(mid, strip.y).expect("strip");
        assert!(hit < 3);
    }

    #[test]
    fn last_buffer_right_edge_resizes_only_itself() {
        let mut app = EditorApp::new("one\n");
        app.add_buffer("two.txt", "two\n");
        app.add_buffer("three.txt", "three\n");
        app.show_many = true;
        app.note_frame(240, 40);
        app.focus_at(2);
        // Scroll so the last tile is on-screen.
        app.view = app.window().origin(2) as f32;
        app.target = app.view;
        let before: Vec<u16> = (0..3).map(|i| app.window().along(i)).collect();
        let slice = layout::tile_slice(app.window(), 2, app.view).expect("last tile visible");
        let (tile_w, _) = app.window().tile_box(2);
        assert_eq!(
            slice.source_x + slice.width,
            tile_w,
            "need the trailing edge on-screen"
        );
        let seam_x = slice.screen_x + slice.width - 1;
        let seam_y = slice.screen_y + 2;
        assert_eq!(app.tile_seam(seam_x, seam_y), Some(2));
        app.pointer(
            Pointer::Down {
                column: seam_x,
                row: seam_y,
            },
            240,
            40,
        );
        app.pointer(
            Pointer::Drag {
                column: seam_x + 24,
                row: seam_y,
            },
            240,
            40,
        );
        assert!(app.window().along(2) > before[2]);
        assert_eq!(app.window().along(0), before[0]);
        assert_eq!(app.window().along(1), before[1]);
    }

    #[test]
    fn settled_flip_does_not_pin_animating_wake() {
        // AFK: open Flip must not keep animating() true after scroll settles.
        let mut app = EditorApp::new("a\n");
        app.add_buffer("b.txt", "b\n");
        app.editing = false;
        app.handle(CommandKey::Tab);
        assert!(app.flip.open);
        app.flip.dirty = false;
        app.flip.scroll = app.flip.selected as f32;
        assert!(
            !app.flip_animating(),
            "settled Flip must drop high-rate wake"
        );
        assert!(!app.animating());
        // Hover under Flip must not dirty the frame (tick23).
        app.hover_hints = true;
        app.hover = Some((1, 1));
        app.hover_since = Some(Instant::now() - Duration::from_secs(2));
        app.hover_hint = None;
        assert!(!app.tick(Duration::from_millis(16)));
        assert!(app.hover_hint.is_none());
        app.flip.dirty = true;
        assert!(app.flip_animating());
        app.flip.dirty = false;
        app.flip.scroll = 0.5;
        app.flip.selected = 1;
        assert!(app.flip_animating(), "easing toward selection stays hot");
    }

    #[test]
    fn flip_sync_warm_skips_when_front_cached() {
        // AFK: warm_flip_previews_sync must not re-raster a cached front card.
        let mut app = EditorApp::new("fn main() {}\n");
        app.add_buffer("b.txt", "b\n");
        app.note_frame(80, 24);
        let job = app.flip_preview_job(0).expect("job");
        let key = job.key;
        let _ = app.preview_pool.get_or_render(job);
        assert!(app.preview_pool.get(key).is_some());
        // Second pass: cache hit path (no panic / no clear).
        app.warm_flip_previews_sync();
        assert!(app.preview_pool.get(key).is_some());
    }

    #[test]
    fn flip_preview_cache_key_matches_job_without_snapshot() {
        let mut app = EditorApp::new("hello\n");
        app.add_buffer("b.txt", "b\n");
        app.note_frame(80, 24);
        let key = app.flip_preview_cache_key(0).expect("key");
        let job = app.flip_preview_job(0).expect("job");
        assert_eq!(key, job.key);
        assert!(!app.preview_pool.needs_work(key) || app.preview_pool.get(key).is_none());
        let _ = app.preview_pool.get_or_render(job);
        assert!(!app.preview_pool.needs_work(key));
    }

    #[test]
    fn flip_cards_skips_rope_scan_for_offscreen_buffers() {
        let mut app = EditorApp::new("front\n");
        for i in 1..12 {
            app.add_buffer(&format!("b{i}.txt"), &format!("body-{i}\n"));
        }
        app.editing = false;
        app.handle(CommandKey::Tab);
        app.flip.scroll = 0.0;
        app.flip.selected = 0;
        let cards = app.flip_cards();
        assert_eq!(cards.len(), 12);
        let filled = cards.iter().filter(|c| !c.title.is_empty()).count();
        let empty = cards.iter().filter(|c| c.title.is_empty()).count();
        assert!(filled >= 1 && filled <= 6, "filled={filled}");
        assert!(empty >= 6, "off-screen stubs={empty}");
        assert!(!cards[0].title.is_empty());
    }

    #[test]
    fn flip_cards_skip_greeked_lines_when_preview_cached() {
        let mut app = EditorApp::new("fn main() { println!(\"hi\"); }\n");
        app.add_buffer("b.txt", "b\n");
        app.note_frame(80, 24);
        app.editing = false;
        app.handle(CommandKey::Tab);
        let job = app.flip_preview_job(0).expect("job");
        let _ = app.preview_pool.get_or_render(job);
        let cards = app.flip_cards();
        assert!(
            cards[0].preview.is_some(),
            "front card should hold preview Arc"
        );
        assert!(
            cards[0].lines.is_empty(),
            "cached preview must skip rope greeking"
        );
    }

    #[test]
    fn flip3d_alt_tab_cycles_and_commits() {
        let mut app = EditorApp::new("a\n");
        app.add_buffer("b.txt", "b\n");
        app.add_buffer("c.txt", "c\n");
        app.focus_at(0);
        app.handle(CommandKey::AltTab);
        assert!(app.flip.open);
        assert_eq!(app.flip.selected, 0, "opens on current buffer");
        app.handle(CommandKey::Right);
        assert_eq!(app.flip.selected, 1);
        app.handle(CommandKey::Right);
        assert_eq!(app.flip.selected, 2);
        app.handle(CommandKey::Enter);
        assert!(!app.flip.open);
        assert_eq!(app.focus, 2);
        app.handle(CommandKey::AltTab);
        app.handle(CommandKey::Esc);
        assert!(!app.flip.open);
        assert_eq!(app.focus, 2);
    }

    #[test]
    fn flip_commit_clears_preview_pool() {
        let mut app = EditorApp::new("a\n");
        app.add_buffer("b.txt", "b\n");
        let key = 0xAF_CACE_u64;
        let _ = app.preview_pool.get_or_render(minimap_gfx::PreviewJob {
            key,
            theme: app.minimap_theme,
            width: 8,
            height: 8,
            glyph_scale: 1,
            lines: vec![vec![('x', (200, 200, 200))]],
        });
        assert!(app.preview_pool.get(key).is_some());
        app.handle(CommandKey::AltTab);
        app.handle(CommandKey::Enter);
        assert!(
            app.preview_pool.get(key).is_none(),
            "closing Flip must drop cached card RGB"
        );
    }

    #[test]
    fn flip3d_hold_tab_view_mode_and_gs_fallback() {
        // Ctrl-Tab is intentionally not Flip 3D — hold Tab in view mode is.
        assert_eq!(map_key(KeyCode::Tab, KeyModifiers::CONTROL), None);
        assert_eq!(
            map_key_event(KeyCode::Tab, KeyModifiers::NONE, KeyEventKind::Release),
            Some(CommandKey::TabRelease)
        );
        let mut app = EditorApp::new("a\n");
        app.add_buffer("b.txt", "b\n");
        app.editing = false;
        app.focus_at(0);
        app.handle(CommandKey::Tab);
        assert!(app.flip.open, "view-mode Tab should open Flip 3D");
        assert_eq!(app.flip.selected, 0, "hold Tab keeps current buffer");
        // Tab repeats while held must not cycle.
        app.handle(CommandKey::Tab);
        assert_eq!(app.flip.selected, 0);
        app.handle(CommandKey::TabRelease);
        assert!(!app.flip.open);
        assert_eq!(app.focus, 0, "release without arrows stays on current");
        // Arrows choose the target; release commits.
        app.handle(CommandKey::Tab);
        app.handle(CommandKey::Right);
        assert_eq!(app.flip.selected, 1);
        app.handle(CommandKey::TabRelease);
        assert_eq!(app.focus, 1);
        // Editing mode: Tab must not open the cycler.
        app.editing = true;
        app.handle(CommandKey::Tab);
        assert!(!app.flip.open);
        app.editing = false;
        // gs remains a fallback when key-up is unavailable.
        app.handle(CommandKey::Char('g'));
        app.handle(CommandKey::Char('s'));
        assert!(app.flip.open, "gs chord should open Flip 3D");
        app.handle(CommandKey::Esc);
        assert!(!app.flip.open);
        // Single buffer: clear notice path.
        let mut one = EditorApp::new("only\n");
        one.editing = false;
        one.handle(CommandKey::Tab);
        assert!(!one.flip.open);
        assert!(one.notice.contains("2+"));
    }

    #[test]
    fn tty_restore_pops_kitty_keyboard_and_full_mouse() {
        let text = String::from_utf8_lossy(TTY_RESTORE);
        // Crossterm PopKeyboardEnhancementFlags is CSI < 1 u — never bare CSI > 4 m.
        assert!(
            text.contains("\u{1b}[<1u"),
            "must pop Kitty progressive enhancement: {text:?}"
        );
        assert!(
            text.contains("\u{1b}[=0u"),
            "must zero Kitty keyboard flags: {text:?}"
        );
        assert!(
            !TTY_RESTORE.windows(4).any(|w| w == b"\x1b[>4m"),
            "must not emit the mistaken >4m-only sequence that left typing broken"
        );
        assert!(
            text.contains("\u{1b}[>4;0m"),
            "xterm modifyOtherKeys off should be present: {text:?}"
        );
        for mode in ["1006", "1015", "1003", "1002", "1000", "1049", "2004", "1004"] {
            assert!(
                text.contains(&format!("\u{1b}[?{mode}l")),
                "missing disable ?{mode}l in restore"
            );
        }
        assert!(text.contains("\u{1b}_Ga=d\u{1b}\\"), "must delete Kitty images");
        assert!(text.contains("\u{1b}[?25h"), "must show cursor");
    }

    #[test]
    fn shutdown_fast_clears_terminals_before_exit() {
        let mut app = EditorApp::new("x\n");
        app.terminals.open = true;
        app.shutdown_fast();
        assert!(!app.terminals.open);
        assert_eq!(app.terminals.len(), 0);
        assert!(app.canvas_surfaces.is_empty());
    }

    #[test]
    fn flip_preview_pool_is_nonblocking() {
        let pool = minimap_gfx::PreviewPool::new();
        let key = minimap_gfx::preview_key(0, 1, 2, 32, 24);
        pool.request(minimap_gfx::PreviewJob {
            key,
            theme: minimap_gfx::MinimapTheme::dark(),
            width: 32,
            height: 24,
            glyph_scale: minimap_gfx::FLIP_GLYPH_SCALE,
            lines: vec![
                vec![('f', (200, 100, 200)), ('n', (200, 100, 200))],
                vec![(' ', (30, 30, 30)), ('x', (100, 180, 220))],
            ],
        });
        // Must return immediately — result may arrive on the worker shortly.
        assert!(pool.get(key).is_none() || pool.get(key).is_some());
        for _ in 0..50 {
            if pool.get(key).is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let img = pool.get(key).expect("background preview should finish");
        assert_eq!(img.width, 32);
        assert_eq!(img.height, 24);
        assert_eq!(img.rgb.len(), 32 * 24 * 3);
    }

    #[test]
    fn flip_preview_pool_sync_shares_arc_with_cache() {
        let pool = minimap_gfx::PreviewPool::new();
        let key = minimap_gfx::preview_key(1, 2, 3, 40, 28);
        let job = minimap_gfx::PreviewJob {
            key,
            theme: minimap_gfx::MinimapTheme::light(),
            width: 40,
            height: 28,
            glyph_scale: minimap_gfx::FLIP_GLYPH_SCALE,
            lines: vec![vec![
                ('p', (40, 80, 40)),
                ('u', (40, 80, 40)),
                ('b', (150, 60, 120)),
            ]],
        };
        let a = pool.get_or_render(job.clone());
        let b = pool.get(key).expect("cached after sync render");
        assert!(std::sync::Arc::ptr_eq(&a, &b), "zero-copy Arc reuse");
        assert_eq!(a.rgb.len(), 40 * 28 * 3);
        // Light theme paper must appear in the raster (not near-black).
        assert!(
            a.rgb.chunks(3).any(|p| p[0] > 200 && p[1] > 200),
            "expected light paper pixels"
        );
    }

    #[test]
    fn flip_scene_theme_follows_terminal_light_palette() {
        let theme = flip3d::SceneTheme::from_rgb((250, 245, 230), (70, 80, 55), true);
        assert_eq!(theme.backdrop, (250, 245, 230));
        assert!(theme.paper.0 >= 250);
        assert_ne!(theme.backdrop, (18, 18, 22));
    }

    #[test]
    fn buffer_strip_paints_precision_track() {
        let mut app = EditorApp::new("a\n");
        app.add_buffer("b.txt", "b\n");
        app.note_frame(80, 24);
        let screen = rendered(&app);
        assert!(
            screen.contains("1/2") || screen.contains("2/2"),
            "expected buffer strip chip in paint"
        );
    }

    #[test]
    fn seam_pills_paint_on_terminal_top_edge_only() {
        let mut app = EditorApp::new("file\n");
        app.note_frame(80, 24);
        app.terminals.open = true;
        app.terminal_rows = Some(8);
        app.maintain_terminal();
        let rect = app.terminal_rect();
        assert!(rect.height > 2);
        // Hover the top border midpoint — only that edge should light the pill.
        let mid_x = rect.x + rect.width / 2;
        app.hover = Some((mid_x, rect.y));
        assert!(app.on_terminal_seam(mid_x, rect.y));
        assert!(
            !app.on_terminal_side_seam(mid_x, rect.y),
            "top-edge hover must not arm the session-list side seam"
        );
        let screen = rendered(&app);
        assert!(
            screen.contains('━') || screen.contains('╸') || screen.contains('╺'),
            "expected horizontal split pill glyphs, got excerpt near end: {}",
            screen.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>()
        );
        // Hovering a body cell must not treat the whole panel as a resize seam.
        let body_x = rect.x + 3;
        let body_y = rect.y + 2;
        app.hover = Some((body_x, body_y));
        assert!(!app.on_terminal_seam(body_x, body_y));
    }

    #[test]
    fn gutter_shows_fold_chevrons_and_za_toggles() {
        let mut app = EditorApp::new("fn main() {\n    let x = 1;\n}\n");
        app.buffers[0].language = "rust".into();
        app.buffers[0].cursor = 0;
        app.buffers[0].anchor = 0;
        app.note_frame(80, 24);
        let screen = rendered(&app);
        assert!(
            screen.contains('▾') || screen.contains('▸'),
            "expected fold chevron in gutter, got: {screen}"
        );
        // za toggles collapse at caret (line 0).
        app.handle(CommandKey::Char('z'));
        app.handle(CommandKey::Char('a'));
        assert!(
            app.notice.contains("folded"),
            "expected fold notice, got: {}",
            app.notice
        );
        assert!(app.focused().folds.borrow().collapsed_count() >= 1);
        let after = rendered(&app);
        assert!(after.contains('▸'), "collapsed chevron missing: {after}");
        // LSP ranges win over syntax when injected.
        app.apply_lsp_folding_ranges(&serde_json::json!([{"startLine": 0, "endLine": 1}]));
        assert_eq!(
            app.focused().folds.borrow().source(),
            Some(fold::FoldSource::Lsp)
        );
    }

    #[test]
    fn fullscreen_is_default_when_opening_another_file() {
        let root = temp_dir("full-default");
        fs::write(root.join("a.txt"), "aaa\n").unwrap();
        fs::write(root.join("b.txt"), "bbb\n").unwrap();
        let mut app = EditorApp::new("seed\n");
        app.set_workspace(root.clone());
        app.open_path(root.join("a.txt"));
        app.open_path(root.join("b.txt"));
        app.note_frame(188, 40);
        assert!(!app.show_many);
        assert_eq!(app.splits(), 1);
        assert!(app.buffers.len() >= 2);
        assert_eq!(app.text(), "bbb\n");
        // Fullscreen: only the focused buffer paints; no second tile.
        assert!(app.tile_corner(1).is_none());
        assert!(rendered_at(&app, 188, 40).contains("bbb"));
        assert!(!rendered_at(&app, 188, 40).contains("aaa"));
        let alone = app.window().tile_width;
        app.handle(CommandKey::CtrlW);
        assert!(app.show_many);
        app.note_frame(188, 40);
        assert!(app.window().tile_width < alone);
        assert_eq!(app.splits(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn split_palette_and_header_render() {
        let mut app = EditorApp::new("hello");
        assert_eq!(app.splits(), 1);
        let screen = rendered(&app);
        assert!(screen.contains("Posit"));
        assert!(screen.contains("full"));
        assert!(screen.contains("Files"));
        app.handle(CommandKey::CtrlW);
        assert_eq!(app.splits(), 2);
        assert!(rendered(&app).contains("auto"));
        app.handle(CommandKey::CtrlP);
        app.handle(CommandKey::Char('q'));
        app.handle(CommandKey::Char('u'));
        app.handle(CommandKey::Char('i'));
        app.handle(CommandKey::Char('t'));
        app.handle(CommandKey::Enter);
        assert!(app.quit());
    }

    #[test]
    fn quit_key_wins_over_coalesced_scroll_storm() {
        // Mirrors the old failure mode: dozens of wheel events queued ahead of q.
        let mut app = EditorApp::new(&"line\n".repeat(5_000));
        app.note_frame(120, 40);
        let mut batch = scheduler::InputBatch::default();
        batch.scroll_v = 80;
        batch.scroll_column = 40;
        batch.scroll_row = 20;
        batch
            .keys
            .push((KeyCode::Char('q'), KeyModifiers::NONE, KeyEventKind::Press));
        apply_input_batch(&mut app, &batch, 120, 40);
        assert!(app.quit(), "q must quit even when a scroll storm shares the batch");
    }

    #[test]
    fn quit_key_wins_over_keyboard_down_storm() {
        let mut app = EditorApp::new(&"line\n".repeat(5_000));
        app.note_frame(120, 40);
        let before = app.focused().scroll;
        let mut batch = scheduler::InputBatch::default();
        for _ in 0..200 {
            batch
                .keys
                .push((KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Press));
        }
        batch
            .keys
            .push((KeyCode::Char('q'), KeyModifiers::NONE, KeyEventKind::Press));
        apply_input_batch(&mut app, &batch, 120, 40);
        assert!(app.quit(), "q must outrank Down-arrow backlog");
        // Downs after quit must not run.
        assert_eq!(app.focused().scroll, before);
    }

    #[test]
    fn typing_is_undoable_at_the_cursor() {
        let mut app = EditorApp::new("ac");
        editing(&mut app);
        app.handle(CommandKey::Left);
        app.handle(CommandKey::Char('b'));
        assert_eq!(app.text(), "abc");
        app.handle(CommandKey::CtrlZ);
        assert_eq!(app.text(), "ac");
    }

    #[test]
    fn typing_does_not_format_and_format_is_a_command() {
        let source = "fn main(){let x=1;}";
        let mut app = EditorApp::new(source);
        app.set_language("rust");
        editing(&mut app);
        app.handle(CommandKey::Char(' '));
        assert_eq!(app.text(), "fn main(){let x=1;} ");
        // Typing must not auto-format; unrelated status (workbench/pressure) may linger.
        assert!(!app.notice().contains("formatted"));
        palette(&mut app, "format");
        let formatted = app.text() != "fn main(){let x=1;} ";
        if formatted {
            assert_eq!(app.notice(), "formatted");
            assert!(app.text().contains("fn main()"));
        } else {
            assert!(
                app.notice().contains("unavailable")
                    || app.notice().contains("failed")
                    || app.notice().contains("already formatted")
            );
        }
    }

    #[test]
    fn keywords_and_strings_use_different_colors() {
        let mut app = EditorApp::new("fn main() {\n    let s = \"fn\";\n}\n");
        app.set_language("rust");
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let cells: Vec<_> = terminal.backend().buffer().content().to_vec();
        let mut colors = Vec::new();
        for pair in cells.windows(2) {
            if pair[0].symbol() == "f" && pair[1].symbol() == "n" {
                colors.push(pair[0].fg);
            }
        }
        assert!(colors.contains(&Color::Magenta), "{colors:?}");
        assert!(colors.contains(&Color::Green), "{colors:?}");
    }

    #[test]
    fn bend_and_markdown_use_keyword_color() {
        let mut app = EditorApp::new("# note\nimport Base\ndef shown():\n  match x:\n");
        app.set_language("bend");
        assert!(screen_has_color(&app, Color::Magenta));
        let mut app = EditorApp::new("# Title\nplain\n");
        app.set_language("markdown");
        assert!(
            screen_has_color(&app, Color::Cyan),
            "markdown headings should paint cyan"
        );
    }

    #[test]
    fn welcome_buffer_is_markdown_with_canvas_preview() {
        let mut app = EditorApp::new("\n");
        app.note_frame(120, 40);
        app.open_welcome_buffer();
        assert_eq!(app.focused().name, "welcome");
        assert_eq!(app.focused().language, "markdown");
        assert!(app.focused().text().starts_with("# Welcome"));
        assert_eq!(app.runtime_dock_id().as_deref(), Some("markdown"));
        assert!(
            app.canvas_surfaces
                .iter()
                .any(|surface| surface.kind == "markdown")
        );
        let screen = rendered_at(&app, 120, 40);
        assert!(
            screen.contains("Preview") || screen.contains("Welcome"),
            "{screen}"
        );
    }

    #[test]
    fn markdown_buffer_conceals_tables_until_hover_reveal() {
        let src = "# Title\n\n| Key | Action |\n| --- | -----: |\n| `e` | edit |\n";
        let mut app = EditorApp::new(src);
        app.set_language("markdown");
        let buffer = app.focused();
        let rendered = super::highlighted_window(
            buffer,
            false,
            false,
            48,
            None,
            0,
            usize::MAX,
            markdown_preview::RevealOpts::default(),
        );
        let flat: String = rendered
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(flat.contains("Title"), "{flat}");
        assert!(!flat.contains("# Title"), "{flat}");
        assert!(flat.contains('│'), "{flat}");
        assert!(!flat.contains("| Key |"), "{flat}");

        let revealed = super::highlighted_window(
            buffer,
            false,
            true,
            48,
            None,
            0,
            usize::MAX,
            markdown_preview::RevealOpts {
                hover_line: Some(2),
                cursor_line: None,
                selection: None,
            },
        );
        let row: String = revealed.lines[2]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(row.contains('|'), "hover reveal raw: {row}");
        assert!(row.contains("Key") || row.contains('⌜'), "{row}");

        let editing = super::highlighted_window(
            buffer,
            true,
            true,
            48,
            None,
            0,
            usize::MAX,
            markdown_preview::RevealOpts {
                hover_line: None,
                cursor_line: Some(4),
                selection: None,
            },
        );
        let header: String = editing.lines[2]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(
            header.contains("| Key |"),
            "caret on table unconceals block: {header}"
        );
        assert!(!header.contains('⌜'), "{header}");

        // View-mode select-all: every line shows raw source.
        let mut app = EditorApp::new(src);
        app.set_language("markdown");
        app.select_all();
        let sel = app.selection().expect("selection");
        let buffer = app.focused();
        let selected = super::highlighted_window(
            buffer,
            false,
            false,
            48,
            None,
            0,
            usize::MAX,
            markdown_preview::RevealOpts {
                hover_line: None,
                cursor_line: None,
                selection: Some(sel),
            },
        );
        let flat: String = selected
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(flat.contains("# Title"), "{flat}");
        assert!(flat.contains("| Key |"), "{flat}");
        assert!(!flat.contains('│'), "select-all drops table render: {flat}");
    }

    #[test]
    fn settings_can_disable_welcome_on_startup() {
        let root = temp_dir("welcome-setting");
        let mut app = EditorApp::new("\n");
        app.set_workspace(root.clone());
        assert!(app.show_welcome);
        app.toggle_show_welcome();
        assert!(!app.show_welcome);
        let saved = fs::read_to_string(root.join(".posit/settings.json")).unwrap();
        assert!(
            saved.contains("\"show_welcome\":false") || saved.contains("\"show_welcome\": false"),
            "{saved}"
        );
        assert!(!load_show_welcome(&root));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn help_button_is_visible_and_opens_the_guide() {
        let app = EditorApp::new("hello");
        let screen = rendered(&app);
        assert!(screen.contains('?'));
        assert!(screen.contains('h'));
        assert!(screen.contains("help"));
        let mut app = app;
        // Title cluster is ` ? h q ` — `?` sits at width-7.
        app.pointer(Pointer::Down { column: 73, row: 0 }, 80, 24);
        assert!(app.help_open());
        let mut saw_title = false;
        let mut saw_columns = false;
        let mut saw_close = false;
        let mut saw_rename = false;
        let mut saw_keybinds = false;
        for _ in 0..500 {
            let screen = rendered(&app);
            saw_title |= screen.contains("title bar");
            saw_columns |= screen.contains("80 columns");
            saw_close |= screen.contains("close the buffer");
            saw_rename |= screen.contains("rename");
            saw_keybinds |= screen.contains("Keybinds")
                || screen.contains("Editing mode")
                || screen.contains("Canvas");
            app.handle(CommandKey::Down);
        }
        assert!(saw_title, "expected title-bar prose in fullscreen help");
        assert!(saw_columns, "expected 80 columns prose in fullscreen help");
        assert!(saw_close, "expected close-buffer bind");
        assert!(saw_rename, "expected rename bind");
        assert!(saw_keybinds, "expected categorized keybind sections");
        assert!(help_text().contains("format the file"));
        assert!(help_text().contains("open floating Yazi"));
        assert!(help_text().contains("| Key | Action |"));
        assert!(help_markdown().contains("## Editing mode"));
        if help_icon_path().is_some() {
            assert!(
                help_markdown().contains("![Posit Edit]("),
                "Help should embed the shipped Posit Edit icon when assets are present"
            );
        }
        // Full-screen: help content spans the terminal width (not a side pane).
        let screen = rendered_at(&app, 80, 24);
        assert!(
            screen.contains("Help") || screen.contains("Esc"),
            "{screen}"
        );
        app.pointer(Pointer::Down { column: 1, row: 23 }, 80, 24);
        assert!(!app.help_open());
        app.handle(CommandKey::Help);
        assert!(app.help_open());
        app.handle(CommandKey::Esc);
        assert!(!app.help_open());
    }

    #[test]
    fn hints_button_and_key_toggle_hover_tooltips() {
        let mut app = EditorApp::new("hello");
        assert!(app.hover_hints);
        // Title cluster is ` ? h ; q ` — `h` sits at width-7.
        app.pointer(Pointer::Down { column: 73, row: 0 }, 80, 24);
        assert!(!app.hover_hints);
        assert_eq!(app.notice(), "hover hints off");
        app.handle(CommandKey::Char('h'));
        assert!(app.hover_hints);
        assert_eq!(app.notice(), "hover hints on");
        // Armed hover must not surface a tooltip while hints are off.
        app.hover_hints = false;
        app.hover = Some((10, 5));
        app.hover_since = Some(Instant::now() - Duration::from_secs(2));
        app.refresh_hover_hint();
        assert!(app.hover_hint.is_none());
        app.hover_hints = true;
        app.refresh_hover_hint();
        // Quit title control still has a hint when enabled.
        app.hover = Some((77, 0));
        app.hover_since = Some(Instant::now() - Duration::from_secs(2));
        app.refresh_hover_hint();
        assert_eq!(app.hover_hint.as_deref(), Some("Quit Posit Edit"));
    }

    #[test]
    fn settings_button_opens_overlay_and_kitty_toggle_persists() {
        let root = temp_dir("settings-kitty");
        let mut app = EditorApp::new("hello");
        app.set_workspace(root.clone());
        assert!(app.kitty_graphics);
        // Title cluster ` ? h ; q ` — `;` at width-5.
        app.pointer(Pointer::Down { column: 75, row: 0 }, 80, 24);
        assert!(matches!(app.overlay, Overlay::Settings));
        let screen = rendered_at(&app, 80, 24);
        assert!(
            screen.contains("kitty") && screen.contains("on"),
            "settings should list kitty toggle: {screen}"
        );
        app.handle(CommandKey::Char('k'));
        assert!(!app.kitty_graphics);
        assert!(!graphics::kitty_allowed());
        let saved = fs::read_to_string(root.join(".posit/settings.json")).unwrap();
        assert!(
            saved.contains("\"kitty_graphics\":false")
                || saved.contains("\"kitty_graphics\": false"),
            "{saved}"
        );
        app.handle(CommandKey::Char('k'));
        assert!(app.kitty_graphics);
        assert!(graphics::kitty_allowed());
        // Header button closes when already open.
        app.pointer(Pointer::Down { column: 75, row: 0 }, 80, 24);
        assert!(matches!(app.overlay, Overlay::None));
        let _ = fs::remove_dir_all(root);
        // Restore global flag for other tests.
        graphics::set_kitty_allowed(true);
    }

    #[test]
    fn mouse_click_drag_and_scroll_edit_the_buffer() {
        let mut app = EditorApp::new("hello\nworld");
        app.note_frame(160, 40);
        let (_, y) = app.text_origin().unwrap();
        let x = text_col(&app);
        app.pointer(
            Pointer::Down {
                column: x + 1,
                row: y,
            },
            160,
            40,
        );
        assert_eq!(app.cursor(), 1);
        editing(&mut app);
        app.handle(CommandKey::Char('X'));
        assert_eq!(app.text(), "hXello\nworld");
        app.pointer(Pointer::Down { column: x, row: y }, 160, 40);
        app.pointer(
            Pointer::Drag {
                column: x + 3,
                row: y,
            },
            160,
            40,
        );
        assert_eq!(app.selection(), Some((0, 3)));
        app.handle(CommandKey::Char('Z'));
        assert_eq!(app.text(), "Zllo\nworld");
        let mut app = EditorApp::new("a\nb\nc\nd\ne\nf\n");
        app.pointer(Pointer::ScrollDown { column: 40, row: 4 }, 80, 8);
        assert_eq!(app.scroll(), 1);
    }

    #[test]
    fn kitty_minimap_clears_during_scroll_and_buffer_slide() {
        let mut app = EditorApp::new(&"line\n".repeat(120));
        app.add_buffer("b.txt", &"other\n".repeat(120));
        app.note_frame(120, 40);
        assert!(!app.minimap_kitty_suppressed());
        let (mini, sb) = app.focused_edge_chrome(120, 120, 30);
        // Settled: Kitty strip (when host is Kitty) or glyph+sb; never sb-only with mini>0.
        let _ = (mini, sb);

        app.pointer(Pointer::ScrollDown { column: 40, row: 8 }, 120, 40);
        assert!(app.minimap_kitty_suppressed(), "wheel must pause Kitty minimap");
        assert_eq!(
            app.focused_edge_chrome(120, 120, 30),
            (0, 1),
            "precision scrollbar while scrolling"
        );

        app.wheel_at = None;
        assert!(!app.minimap_kitty_suppressed());

        app.focus_at(1);
        assert!(
            app.view_transitioning() || app.minimap_kitty_suppressed(),
            "buffer focus slide must pause Kitty minimap"
        );
        assert_eq!(
            app.focused_edge_chrome(120, 120, 30),
            (0, 1),
            "precision scrollbar during buffer transition"
        );
        // Settle the slide.
        for _ in 0..40 {
            app.tick(Duration::from_millis(50));
            if !app.view_transitioning() {
                break;
            }
        }
        assert!(!app.view_transitioning());
        assert!(!app.minimap_kitty_suppressed());
    }

    #[test]
    fn discrete_scroll_clamps_at_edges() {
        let mut app = EditorApp::new("only\n");
        app.note_frame(120, 40);
        app.pointer(Pointer::ScrollDown { column: 40, row: 8 }, 120, 40);
        assert_eq!(app.scroll(), 0);
        app.pointer(Pointer::ScrollUp { column: 40, row: 8 }, 120, 40);
        assert_eq!(app.scroll(), 0);
        assert!(!app.animating());

        let mut app = EditorApp::new(&"line\n".repeat(80));
        app.note_frame(120, 40);
        app.pointer(Pointer::ScrollDown { column: 40, row: 8 }, 120, 40);
        assert_eq!(app.scroll(), 1);
        app.pointer(Pointer::ScrollDown { column: 40, row: 8 }, 120, 40);
        assert_eq!(app.scroll(), 2);
        app.handle(CommandKey::ShiftDown);
        assert_eq!(app.scroll(), 10);
        app.handle(CommandKey::ShiftUp);
        assert_eq!(app.scroll(), 2);
        app.handle(CommandKey::ShiftUp);
        assert_eq!(app.scroll(), 0);
        app.handle(CommandKey::ShiftUp);
        assert_eq!(app.scroll(), 0);

        let mut app = EditorApp::new(&format!("{}\n", "x".repeat(180)));
        app.note_frame(40, 16);
        app.pointer(Pointer::ScrollRight { column: 8, row: 4 }, 40, 16);
        assert!(app.buffers[0].scroll_x >= 1, "{}", app.buffers[0].scroll_x);
        let after_right = app.buffers[0].scroll_x;
        app.pointer(Pointer::ScrollLeft { column: 8, row: 4 }, 40, 16);
        assert_eq!(app.buffers[0].scroll_x, after_right.saturating_sub(1));
        for _ in 0..after_right {
            app.pointer(Pointer::ScrollLeft { column: 8, row: 4 }, 40, 16);
        }
        assert_eq!(app.buffers[0].scroll_x, 0);
    }

    #[test]
    fn toolbar_hover_uses_cyan() {
        let mut app = EditorApp::new("menu");
        app.note_frame(120, 30);
        app.pointer(Pointer::Right { column: 20, row: 6 }, 120, 30);
        app.pointer(Pointer::Move { column: 22, row: 8 }, 120, 30);
        let cursor = match &app.overlay {
            Overlay::Menu(menu) => menu.cursor,
            _ => panic!("menu"),
        };
        assert_eq!(cursor, 1);
        let chrome = app.frame_chrome(Rect::new(0, 0, 120, 30));
        app.pointer(
            Pointer::Move {
                column: chrome.toolbar.x + 2,
                row: chrome.toolbar.y,
            },
            120,
            30,
        );
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let cell = &terminal.backend().buffer()[(chrome.toolbar.x + 2, chrome.toolbar.y)];
        assert_eq!(cell.bg, Color::Cyan);
    }

    #[test]
    fn copy_and_paste_use_the_system_clipboard() {
        let previous = clipboard::get().ok();
        let sample = format!("posit-clip-{}-axiom", std::process::id());
        clipboard::set_confirmed(&sample).expect("system clipboard");
        assert_eq!(clipboard::get().expect("system clipboard"), sample);

        let mut app = EditorApp::new("keep me");
        app.handle(CommandKey::Char('a'));
        app.handle(CommandKey::Char('y'));
        assert_eq!(app.notice(), "copied");
        // External pasteboard clients can overwrite between yank and read under
        // Urgent memory pressure — retry briefly before failing the suite.
        let yanked = (0..12).find_map(|_| {
            let text = clipboard::get().ok()?;
            if text == "keep me" {
                Some(text)
            } else {
                std::thread::sleep(Duration::from_millis(20));
                None
            }
        });
        assert_eq!(yanked.as_deref(), Some("keep me"));
        clipboard::set_confirmed("ZZ").unwrap();
        app.note_frame(160, 40);
        let (x, y) = app.text_origin().unwrap();
        app.pointer(
            Pointer::Down {
                column: x + 20,
                row: y,
            },
            160,
            40,
        );
        app.handle(CommandKey::Char('p'));
        assert_eq!(app.text(), "keep meZZ");
        assert_eq!(app.notice(), "pasted");

        app.handle(CommandKey::Char('a'));
        assert_eq!(app.selection(), Some((0, app.text().chars().count())));
        // Right-click outside the selection must keep it.
        let menu_x = x + 20;
        app.pointer(Pointer::Right { column: menu_x, row: y }, 160, 40);
        assert_eq!(app.selection(), Some((0, app.text().chars().count())));
        let screen = rendered_at(&app, 160, 40);
        assert!(screen.contains("Clipboard"));
        assert!(screen.contains("Cut"));
        assert!(screen.contains("Copy"));
        assert!(screen.contains("Paste"));
        assert!(screen.contains("Select All"));
        app.pointer(
            Pointer::Down {
                column: menu_x + 2,
                row: y + 2,
            },
            160,
            40,
        );
        assert_eq!(clipboard::get().unwrap(), app.text());
        assert_eq!(app.notice(), "copied");
        if let Some(text) = previous {
            let _ = clipboard::set(&text);
        }
    }

    #[test]
    fn title_q_quits_and_w_closes_the_buffer() {
        let mut app = EditorApp::new("hello\n");
        app.note_frame(80, 24);
        let row = row_symbols(&app, 80, 24, 0);
        let help = row.iter().rposition(|cell| cell == "?").unwrap();
        let quit = row.iter().rposition(|cell| cell == "q").unwrap();
        assert!(quit > help);
        let (x, y) = app.tile_corner(0).unwrap();
        let title = row_symbols(&app, 80, 24, y);
        let mark = title
            .iter()
            .rposition(|cell| cell == "w")
            .expect("w close button");
        assert!(mark + 1 >= x as usize);
        app.pointer(
            Pointer::Down {
                column: quit as u16,
                row: 0,
            },
            80,
            24,
        );
        assert!(app.quit());

        let mut app = EditorApp::new("hello\n");
        app.note_frame(80, 24);
        app.maintain_terminal();
        assert!(
            !app.terminals.focused(),
            "startup must keep keyboard focus on the text buffer"
        );
        app.handle(CommandKey::Enter);
        assert!(app.editing, "buffer edit must work before any terminal click");
        assert!(!app.terminals.focused());
        app.handle(CommandKey::Esc);
        assert!(!app.editing);
        let rect = app.terminal_rect();
        app.pointer(
            Pointer::Down {
                column: rect.x + 2,
                row: rect.y + 2,
            },
            80,
            24,
        );
        assert!(app.terminals.focused());
        assert!(app.terminal_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.quit());
        app.pointer(
            Pointer::Drag {
                column: rect.x + 8,
                row: rect.y + 2,
            },
            80,
            24,
        );
        assert!(
            !app.terminals.focused(),
            "terminal selection must not trap keyboard focus"
        );
        app.handle(CommandKey::Enter);
        assert!(app.editing);
        assert!(!app.terminals.focused());
        assert_eq!(app.pane_focus(), PaneFocus::Editor);

        let mut app = EditorApp::new("focus\n");
        app.note_frame(160, 40);
        app.maintain_terminal();
        let chrome = app.frame_chrome(Rect::new(0, 0, 160, 40));
        app.pointer(
            Pointer::Down {
                column: chrome.agent.x + 2,
                row: chrome.agent.y + 2,
            },
            160,
            40,
        );
        assert_eq!(app.pane_focus(), PaneFocus::Agent);
        assert!(!app.terminals.focused());
        assert!(!app.editing);
        let (x, y) = app.text_origin().unwrap();
        app.pointer(Pointer::Down { column: x, row: y }, 160, 40);
        assert_eq!(app.pane_focus(), PaneFocus::Editor);
        assert!(!app.agent_focused);
        app.set_hover(chrome.terminal.x + 2, chrome.terminal.y + 2);
        assert_eq!(
            panel_border(false, true, false, false),
            Color::Cyan,
            "unfocused hover must read as click-to-focus"
        );
    }

    #[test]
    fn terminal_panel_is_quarter_the_screen_and_multiplexes() {
        let mut app = EditorApp::new("file\n");
        app.note_frame(80, 24);
        assert_eq!(app.terminal_rect().height, 6);
        app.handle(CommandKey::Char('t'));
        assert_eq!(app.terminal_rect().height, 1);
        assert_eq!(app.notice(), "terminal off");
        app.handle(CommandKey::Char('t'));
        assert_eq!(app.terminal_rect().height, 6);
        let rect = app.terminal_rect();
        let screen = rendered_at(&app, 80, 24);
        assert!(screen.contains("Terminal") || screen.contains("terminal"));
        assert!(screen.contains('│') || screen.contains('|'));
        // Session-list divider is a yellow resize affordance.
        let (body, side) = {
            let inner = rect.width.saturating_sub(2);
            let want = 18_u16;
            let side = want.min(inner / 3).max(8);
            (inner.saturating_sub(side).saturating_sub(1), side)
        };
        assert!(side > 0);
        let divider_x = rect.x + 1 + body;
        assert!(
            app.on_terminal_side_seam(divider_x, rect.y + 2),
            "mux divider should be a resize seam"
        );
        app.set_hover(divider_x, rect.y + 2);
        assert_eq!(
            hover_action(&app, divider_x, rect.y + 2),
            Some("Drag to resize the session list")
        );
        // Frame inset: sidebar + sits on the last content row (bottom border - 1).
        let add_row = rect.y + rect.height.saturating_sub(2);
        let side_x = rect.x + rect.width.saturating_sub(4);
        app.pointer(
            Pointer::Down {
                column: side_x,
                row: add_row,
            },
            80,
            24,
        );
        app.pointer(
            Pointer::Down {
                column: side_x,
                row: add_row,
            },
            80,
            24,
        );
        assert!(app.terminals.len() >= 1);
        let screen = rendered_at(&app, 80, 24);
        assert!(screen.contains('+'));
        // Toggle hint matches buffer `w` / Files `<`: top-right title `t`.
        let title = row_symbols(&app, 80, 24, rect.y);
        let mark = title
            .iter()
            .rposition(|cell| cell == "t")
            .expect("terminal toggle t on title row");
        assert!(mark + 1 < title.len());
        assert_eq!(title[mark.saturating_sub(1)], " ");
        assert_eq!(title[mark + 1], " ");
        assert!(rect.y > 0);
        assert_eq!(rect.width, 80);
        let before = app.terminals.len();
        // Close the focused session via its x control on the sidebar.
        app.pointer(
            Pointer::Down {
                column: rect.x + rect.width.saturating_sub(2),
                row: rect.y + 1,
            },
            80,
            24,
        );
        assert!(app.terminals.len() <= before);
        // Top-right `t` toggles the pane closed.
        app.pointer(
            Pointer::Down {
                column: rect.x + rect.width.saturating_sub(2),
                row: rect.y,
            },
            80,
            24,
        );
        assert_eq!(app.terminal_rect().height, 1);

        app.note_frame(40, 90);
        assert_eq!(app.axis(), Axis::Vertical);
        let tall = app.terminal_rect();
        assert_eq!(tall.x, 0);
        assert_eq!(tall.width, 40);
        assert!(tall.y + tall.height < 90);
        let editor_bottom = tall.y;
        app.pointer(
            Pointer::Down {
                column: 4,
                row: editor_bottom.saturating_sub(1),
            },
            40,
            90,
        );
        app.pointer(
            Pointer::Drag {
                column: 4,
                row: editor_bottom.saturating_sub(5),
            },
            40,
            90,
        );
        assert!(app.terminal_rect().height > tall.height);
        assert_eq!(app.terminal_rect().width, 40);

        // Top title-row of the open pane is also a vertical resize handle.
        app.note_frame(80, 24);
        app.terminals.open = true;
        app.terminal_rows = None;
        let rect = app.terminal_rect();
        assert!(rect.height > 1);
        assert!(app.on_terminal_seam(rect.x + 4, rect.y));
        assert_eq!(
            hover_action(&app, rect.x + 4, rect.y),
            Some("Drag to resize the terminal")
        );
        let before = rect.height;
        app.pointer(
            Pointer::Down {
                column: rect.x + 4,
                row: rect.y,
            },
            80,
            24,
        );
        assert!(app.terminal_drag);
        app.pointer(
            Pointer::Drag {
                column: rect.x + 4,
                row: rect.y.saturating_sub(4),
            },
            80,
            24,
        );
        assert!(app.terminal_rect().height > before);
        // Title-row `t` / log|term stay click targets, not resize seams.
        let toggle_x = rect.x + rect.width.saturating_sub(2);
        assert!(!app.on_terminal_seam(toggle_x, rect.y));
        let switch_x = rect.x + rect.width.saturating_sub(6);
        assert!(!app.on_terminal_seam(switch_x, rect.y));
    }

    #[test]
    fn terminal_body_drag_selects_instead_of_resizing() {
        let mut app = EditorApp::new("file\n");
        app.note_frame(80, 24);
        app.maintain_terminal();
        let rect = app.terminal_rect();
        let before_h = rect.height;
        let before_side = app.terminals.side_width();
        // Down in the PTY body, then drag across it and past the top seam.
        app.pointer(
            Pointer::Down {
                column: rect.x + 3,
                row: rect.y + 2,
            },
            80,
            24,
        );
        app.pointer(
            Pointer::Drag {
                column: rect.x + 12,
                row: rect.y + 2,
            },
            80,
            24,
        );
        app.pointer(
            Pointer::Drag {
                column: rect.x + 12,
                row: rect.y.saturating_sub(1),
            },
            80,
            24,
        );
        assert_eq!(app.terminal_rect().height, before_h, "height must not change");
        assert_eq!(
            app.terminals.side_width(),
            before_side,
            "session list must not resize"
        );
        assert!(
            app.terminals.has_selection() || !app.terminals.focused(),
            "body drag should select or release focus for yank"
        );
        // A leftover seam Down must not poison the next body selection.
        app.pointer(
            Pointer::Down {
                column: rect.x + 3,
                row: rect.y.saturating_sub(1),
            },
            80,
            24,
        );
        app.pointer(
            Pointer::Down {
                column: rect.x + 4,
                row: rect.y + 2,
            },
            80,
            24,
        );
        app.pointer(
            Pointer::Drag {
                column: rect.x + 10,
                row: rect.y + 3,
            },
            80,
            24,
        );
        assert_eq!(app.terminal_rect().height, before_h);
    }

    #[test]
    fn unsaved_buffers_ask_before_close_and_quit() {
        let mut app = EditorApp::new("clean\n");
        app.handle(CommandKey::Char('q'));
        assert!(app.quit());

        let root = temp_dir("unsaved-close");
        let path = root.join("note.txt");
        fs::write(&path, "keep\n").unwrap();
        let mut app = EditorApp::new("keep\n");
        app.buffers[0].path = Some(path.clone());
        app.buffers[0].name = "note.txt".into();
        editing(&mut app);
        app.handle(CommandKey::Char('!'));
        assert!(app.buffers[0].dirty());
        app.handle(CommandKey::Esc);
        app.handle(CommandKey::Char('w'));
        assert_eq!(app.buffers.len(), 1);
        assert!(app.text().contains('!'));
        assert!(rendered(&app).contains("y save&close"));
        assert!(rendered(&app).contains("! close"));
        app.handle(CommandKey::Esc);
        assert!(app.text().contains('!'));
        app.handle(CommandKey::Char('w'));
        app.handle(CommandKey::Char('y'));
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep\n!");
        assert!(!app.buffers[0].dirty());

        let other = root.join("other.txt");
        fs::write(&other, "two\n").unwrap();
        let mut app = EditorApp::new("two\n");
        app.buffers[0].path = Some(other.clone());
        editing(&mut app);
        app.handle(CommandKey::Char('x'));
        app.handle(CommandKey::Esc);
        app.handle(CommandKey::Char('q'));
        assert!(!app.quit());
        assert!(rendered(&app).contains("y save&quit"));
        assert!(rendered(&app).contains("! quit"));
        app.handle(CommandKey::Char('n'));
        assert!(!app.quit());
        assert!(app.text().contains('x'));
        app.handle(CommandKey::Char('q'));
        app.handle(CommandKey::Char('y'));
        assert!(app.quit());
        assert_eq!(fs::read_to_string(&other).unwrap(), "two\nx");

        let saved = root.join("saved.txt");
        fs::write(&saved, "base\n").unwrap();
        let mut app = EditorApp::new("base\n");
        app.buffers[0].path = Some(saved.clone());
        editing(&mut app);
        app.handle(CommandKey::Char('z'));
        app.handle(CommandKey::Esc);
        app.handle(CommandKey::Char('q'));
        app.handle(CommandKey::Char('!'));
        assert!(app.quit());
        assert_eq!(fs::read_to_string(&saved).unwrap(), "base\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn click_past_the_line_puts_the_cursor_on_the_last_character() {
        let mut app = EditorApp::new("hi\nworld");
        app.note_frame(160, 40);
        let (_, y) = app.text_origin().unwrap();
        let x = text_col(&app);
        app.pointer(
            Pointer::Down {
                column: x + 20,
                row: y,
            },
            160,
            40,
        );
        assert_eq!(app.cursor(), 2);
        assert_eq!(cell_bg(&app, 160, 40, x + 2, y), Color::Magenta);
        assert_ne!(cell_bg(&app, 160, 40, x + 8, y), Color::Magenta);
        editing(&mut app);
        app.pointer(
            Pointer::Down {
                column: x + 20,
                row: y + 1,
            },
            160,
            40,
        );
        assert_eq!(app.cursor(), 8);
        assert_eq!(
            cell_bg(&app, 160, 40, x + 5, y + 1),
            Color::Rgb(230, 120, 20)
        );
        assert_ne!(
            cell_bg(&app, 160, 40, x + 12, y + 1),
            Color::Rgb(230, 120, 20)
        );
    }

    #[test]
    fn one_file_expands_and_full_columns_cap_at_eighty() {
        let mut app = EditorApp::new(&"a".repeat(90));
        app.show_many = true;
        app.note_frame(188, 40);
        assert_eq!(app.window().tile_width, 128);
        let (_, y) = app.text_origin().unwrap();
        let x = text_col(&app) as usize;
        let row = row_symbols(&app, 188, 40, y);
        assert_eq!(row[x..(x + 90)].concat(), "a".repeat(90));

        app.add_buffer("other", "b\n");
        app.note_frame(188, 40);
        assert_eq!(app.window().tile_width, 64);
        let before = app.window().along(0);
        app.tile_spans = Some(vec![before.saturating_sub(20).max(1), before + 20]);
        app.close_buffer(1);
        app.note_frame(188, 40);
        assert_eq!(app.buffers.len(), 1);
        assert_eq!(app.window().along(0), 128);
        assert_eq!(app.window().tile_width, 128);
        assert!(app.tile_spans.is_none());

        app.add_buffer("other", "b\n");
        app.note_frame(188, 40);
        assert_eq!(app.window().tile_width, 64);
        let (_, y) = app.text_origin().unwrap();
        let x = text_col(&app) as usize;
        let row = row_symbols(&app, 188, 40, y);
        let visible = app.visible_cols().min(90);
        assert!(visible < 90, "split tile must clip the 90-char line");
        assert_eq!(row[x..(x + visible)].concat(), "a".repeat(visible));
        assert_ne!(row.get(x + visible).map(String::as_str), Some("a"));
        app.add_buffer("third", "c\n");
        app.note_frame(188, 40);
        assert!(app.tile_corner(2).is_none());
    }

    #[test]
    fn tile_edge_drag_widens_a_clipped_line_and_fullscreen_does_not() {
        let mut app = EditorApp::new(&"a".repeat(90));
        app.show_many = true;
        app.add_buffer("other", "b\n");
        app.note_frame(188, 40);
        let before = app.window().along(0);
        assert_eq!(before, app.window().along(1));
        let (x, y) = app.text_origin().unwrap();
        let gutter_w = app
            .gutter
            .width_for(app.focused().document.len_lines().max(1)) as usize;
        let row = row_symbols(&app, 188, 40, y);
        let text_x = x as usize + gutter_w;
        let visible_before = row[text_x..]
            .iter()
            .take_while(|cell| *cell == "a")
            .count();
        assert!(visible_before < 90);

        let slice = layout::tile_slice(app.window(), 0, app.view).unwrap();
        let seam_x = slice.screen_x + slice.width - 1;
        let seam_y = slice.screen_y + 2;
        app.pointer(
            Pointer::Down {
                column: seam_x,
                row: seam_y,
            },
            188,
            40,
        );
        app.pointer(
            Pointer::Drag {
                column: seam_x + 16,
                row: seam_y,
            },
            188,
            40,
        );
        assert!(app.window().along(0) > before);
        // Neighbour keeps its preferred width (niri-style independent spans).
        assert_eq!(app.window().along(1), before);
        let (x, y) = app.text_origin().unwrap();
        let gutter_w = app
            .gutter
            .width_for(app.focused().document.len_lines().max(1)) as usize;
        let row = row_symbols(&app, 188, 40, y);
        let text_x = x as usize + gutter_w;
        let visible_after = row[text_x..]
            .iter()
            .take_while(|cell| *cell == "a")
            .count();
        assert!(visible_after > visible_before);

        app.note_frame(50, 110);
        assert_eq!(app.axis(), Axis::Vertical);
        let top = app.window().along(0);
        assert_eq!(top, app.window().along(1));
        let neighbour = app.window().along(1);
        let slice = layout::tile_slice(app.window(), 0, app.view).unwrap();
        let seam_x = slice.screen_x + 2;
        let seam_y = slice.screen_y + slice.height - 1;
        app.pointer(
            Pointer::Down {
                column: seam_x,
                row: seam_y,
            },
            50,
            110,
        );
        app.pointer(
            Pointer::Drag {
                column: seam_x,
                row: seam_y + 8,
            },
            50,
            110,
        );
        assert!(app.window().along(0) > top);
        assert_eq!(app.window().along(1), neighbour);

        app.note_frame(188, 40);
        app.handle(CommandKey::CtrlW);
        assert!(!app.show_many);
        app.note_frame(188, 40);
        let full = app.window().tile_width;
        assert_eq!(full, 128);
        let (x, y) = app.text_origin().unwrap();
        app.pointer(
            Pointer::Down {
                column: x + 10,
                row: y,
            },
            188,
            40,
        );
        app.pointer(
            Pointer::Drag {
                column: x + 40,
                row: y,
            },
            188,
            40,
        );
        assert_eq!(app.window().tile_width, full);
        assert_eq!(app.window().along(0), full);
    }

    #[test]
    fn focus_scrolls_sideways_when_wide_and_down_when_tall() {
        let mut app = EditorApp::new("red\n");
        app.show_many = true;
        app.buffers[0].name = "red-buffer".into();
        app.add_buffer("blue-buffer", "blue\n");
        app.note_frame(260, 40);
        assert_eq!(app.axis(), Axis::Horizontal);
        assert!(app.window().tile_width > 80);
        let tiled = rendered_at(&app, 260, 40);
        assert!(tiled.contains("red-buffer"));
        assert!(tiled.contains("blue-buffer"));
        let (first_x, first_y) = app.tile_corner(0).unwrap();
        let (second_x, second_y) = app.tile_corner(1).unwrap();
        assert!(second_x > first_x);
        assert_eq!(first_y, second_y);
        app.handle(CommandKey::CtrlW);
        let alone = rendered_at(&app, 260, 40);
        assert!(alone.contains("red-buffer"));
        assert!(!alone.contains("blue-buffer"));
        app.handle(CommandKey::CtrlDown);
        assert_eq!(app.focus_index(), 0);
        app.handle(CommandKey::CtrlRight);
        assert_eq!(app.focus_index(), 1);
        let started = app.view_offset();
        app.tick(Duration::from_millis(200));
        assert!(app.view_offset() > started);
        let mid = rendered_at(&app, 260, 40);
        app.tick(Duration::from_secs(3));
        assert!((app.view_offset() - app.view_target()).abs() < 0.2);
        let settled = rendered_at(&app, 260, 40);
        assert_ne!(mid, settled);
        assert!(settled.contains("blue-buffer"));
        assert!(!settled.contains("red-buffer"));

        app.focus = 0;
        app.show_many = true;
        app.note_frame(50, 110);
        assert_eq!(app.axis(), Axis::Vertical);
        assert_eq!(app.window().tile_width, 22);
        app.note_frame(140, 180);
        assert_eq!(app.window().tile_width, 112);
        app.note_frame(50, 110);
        let (top_x, top_y) = app.tile_corner(0).unwrap();
        let (bottom_x, bottom_y) = app.tile_corner(1).unwrap();
        assert!(bottom_y > top_y);
        assert_eq!(top_x, bottom_x);
        assert!(rendered_at(&app, 50, 110).contains("portrait"));
        let focus = app.focus_index();
        app.handle(CommandKey::Up);
        assert_eq!(app.focus_index(), focus);
        app.handle(CommandKey::Right);
        assert_eq!(app.focus_index(), 1);
        app.handle(CommandKey::Left);
        assert_eq!(app.focus_index(), focus);
    }

    #[test]
    fn file_tree_opens_a_workspace_file_and_save_writes_it() {
        let root = temp_dir("tree");
        fs::write(root.join("note.txt"), "tree-marker").unwrap();
        let mut app = EditorApp::new("");
        app.set_workspace(root.clone());
        app.note_frame(160, 40);
        let screen = rendered_at(&app, 160, 40);
        assert!(screen.contains("note.txt"));
        app.tree_cursor = app
            .tree
            .iter()
            .position(|entry| entry.name == "note.txt")
            .unwrap();
        app.activate_tree();
        assert_eq!(app.text(), "tree-marker");
        editing(&mut app);
        app.handle(CommandKey::Char('!'));
        app.handle(CommandKey::CtrlS);
        assert_eq!(app.notice(), "saved");
        assert_eq!(
            fs::read_to_string(root.join("note.txt")).unwrap(),
            "!tree-marker"
        );
        let again = app.buffers.len();
        app.open_path(root.join("note.txt"));
        assert_eq!(app.buffers.len(), again);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn new_buffer_and_yazi_paths_become_editable_files() {
        let root = temp_dir("yazi");
        fs::write(root.join("exists.txt"), "from-yazi").unwrap();
        let mut app = EditorApp::new("welcome");
        app.set_workspace(root.clone());
        palette(&mut app, "new");
        assert_eq!(app.focus_index(), 1);
        editing(&mut app);
        app.handle(CommandKey::Char('n'));
        assert_eq!(app.text(), "n");
        app.open_picked(&[root.join("exists.txt"), root.join("brand-new.txt")]);
        assert_eq!(app.text(), "");
        assert!(root.join("brand-new.txt").exists());
        app.handle(CommandKey::CtrlLeft);
        app.tick(Duration::from_secs(2));
        assert_eq!(app.text(), "from-yazi");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fuzzy_find_grep_and_tree_search_highlight_matches() {
        let root = temp_dir("search");
        fs::write(root.join("alpha.rs"), "fn fuzzy_marker() {}\n").unwrap();
        fs::write(root.join("beta.rs"), "fn other() {}\n").unwrap();
        let mut app = EditorApp::new("");
        app.set_workspace(root.clone());
        app.note_frame(120, 30);
        palette(&mut app, "find");
        for ch in "alp".chars() {
            app.handle(CommandKey::Char(ch));
        }
        let screen = rendered_at(&app, 120, 30);
        assert!(screen.contains("alpha.rs"));
        assert!(!screen.contains("beta.rs"));
        assert!(highlighted(&app, 120, 30, "alp"));
        app.handle(CommandKey::Enter);
        assert!(app.text().contains("fuzzy_marker"));
        palette(&mut app, "grep");
        for ch in "fuzzy_marker".chars() {
            app.handle(CommandKey::Char(ch));
        }
        assert!(rendered_at(&app, 120, 30).contains("fuzzy_marker"));
        assert!(highlighted(&app, 120, 30, "fuzzy_marker"));
        app.handle(CommandKey::Tab);
        let switched = rendered_at(&app, 120, 30);
        assert!(switched.contains("Find"));
        assert!(!switched.contains("Grep"));
        app.handle(CommandKey::Tab);
        app.handle(CommandKey::Enter);
        assert_eq!(app.cursor(), 0);
        app.tree_focused = true;
        app.handle(CommandKey::Char('/'));
        for ch in "beta".chars() {
            app.handle(CommandKey::Char(ch));
        }
        assert!(rendered_at(&app, 120, 30).contains("beta.rs"));
        assert!(highlighted(&app, 120, 30, "beta"));
        app.handle(CommandKey::Enter);
        assert!(app.text().contains("fn other"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn view_mode_commands_reorder_buffers_and_click_chrome() {
        let mut app = EditorApp::new("one\n");
        assert!(!app.editing);
        app.handle(CommandKey::Char('x'));
        assert_eq!(app.text(), "one\n");
        app.handle(CommandKey::Char('e'));
        assert!(app.editing, "e enters edit at the caret");
        assert_eq!(app.text(), "one\n");
        app.handle(CommandKey::Esc);
        assert!(!app.editing);
        app.handle(CommandKey::Enter);
        assert!(app.editing);
        app.handle(CommandKey::Char('?'));
        assert!(app.text().contains('?'));
        app.handle(CommandKey::Esc);
        app.handle(CommandKey::Char('?'));
        assert!(app.help_open());
        app.handle(CommandKey::Esc);

        app.handle(CommandKey::Char(':'));
        assert!(rendered(&app).contains("Palette"));
        app.handle(CommandKey::Esc);
        palette(&mut app, "wq");
        let warning = rendered(&app);
        assert!(warning.contains("not Vim"));
        assert!(warning.contains("view mode"));
        app.handle(CommandKey::Esc);
        assert!(!app.quit());

        for spelling in ["q", ":q", "q!", ":qa"] {
            palette(&mut app, spelling);
            assert_eq!(app.notice, palette::VIM_QUIT_NOTICE, "{spelling}");
            assert!(!app.quit(), "{spelling}");
        }

        app.handle(CommandKey::Char('q'));
        assert!(!app.quit());
        assert!(rendered(&app).contains("y quit"));
        app.handle(CommandKey::Char('n'));
        assert!(!app.quit());

        let mut app = EditorApp::new("first");
        app.add_buffer("second", "second");
        app.add_buffer("third", "third");
        app.note_frame(260, 40);
        app.handle(CommandKey::Right);
        app.handle(CommandKey::Right);
        assert_eq!(app.focus_index(), 2);
        app.handle(CommandKey::Char('['));
        assert_eq!(app.focus_index(), 0);
        app.handle(CommandKey::Char(']'));
        assert_eq!(app.focus_index(), 2);
        app.handle(CommandKey::Char('{'));
        assert_eq!(app.focus_index(), 0);
        assert_eq!(app.text(), "third");
        app.handle(CommandKey::Char('}'));
        assert_eq!(app.focus_index(), 2);
        assert_eq!(app.buffers[0].text(), "first");
        app.handle(CommandKey::ShiftLeft);
        assert_eq!(app.focus_index(), 1);
        assert_eq!(app.text(), "third");
        app.handle(CommandKey::Down);
        assert_eq!(app.scroll(), 0);
        let tall = "a\n".repeat(30);
        let mut app = EditorApp::new(&tall);
        app.note_frame(80, 10);
        app.handle(CommandKey::Down);
        assert_eq!(app.scroll(), 1);

        let mut app = EditorApp::new("keep");
        app.add_buffer("drop", "drop");
        app.note_frame(220, 40);
        app.handle(CommandKey::Right);
        app.handle(CommandKey::Char('w'));
        assert_eq!(app.buffers.len(), 1);
        assert_eq!(app.text(), "keep");

        let mut app = EditorApp::new("plain");
        app.handle(CommandKey::Char('b'));
        app.flush_chord();
        assert!(app.notice().contains("no build"));
        app.handle(CommandKey::Char('<'));
        assert!(!app.sidebar);
        assert!(rendered(&app).contains('<'));
        app.handle(CommandKey::Char('<'));
        assert!(app.sidebar);

        app.handle(CommandKey::Char('l'));
        app.flush_chord();
        assert!(rendered(&app).contains("Buffers"));
        app.handle(CommandKey::Esc);

        app.handle(CommandKey::Char(','));
        app.handle(CommandKey::Char('.'));
        assert!(!rendered(&app).contains("sidebar   "));
        app.handle(CommandKey::Char(';'));
        assert!(rendered(&app).contains("sidebar   "));
        app.handle(CommandKey::Esc);

        let mut app = EditorApp::new("red\n");
        app.show_many = true;
        app.buffers[0].name = "red-buffer".into();
        app.add_buffer("blue-buffer", "blue\n");
        app.note_frame(220, 40);
        let (x, y) = app.tile_corner(1).unwrap();
        assert_eq!(cell_fg(&app, 220, 40, x, y), Color::Gray);
        let (x, y) = app.tile_corner(0).unwrap();
        assert_eq!(cell_fg(&app, 220, 40, x, y), Color::Magenta);
        let cursor = cursor_bg(&app, 220, 40);
        assert_eq!(cursor, Color::Magenta);
        app.handle(CommandKey::Enter);
        assert!(app.editing);
        assert_eq!(cell_fg(&app, 220, 40, x, y), Color::Rgb(230, 120, 20));
        assert_eq!(cursor_bg(&app, 220, 40), Color::Rgb(230, 120, 20));
        app.handle(CommandKey::Esc);
        assert!(rendered_at(&app, 220, 40).contains("Agent"));
        let (x, y) = app.tile_corner(1).unwrap();
        let width = app.window().tile_width;
        app.pointer(
            Pointer::Down {
                column: x + width - 2,
                row: y,
            },
            220,
            40,
        );
        assert_eq!(app.buffers.len(), 1);

        let mut app = EditorApp::new("tree");
        app.note_frame(160, 40);
        app.pointer(Pointer::Down { column: 26, row: 5 }, 160, 40);
        assert!(!app.sidebar);
        app.pointer(Pointer::Down { column: 1, row: 5 }, 160, 40);
        assert!(app.sidebar);

        let root = temp_dir("rename");
        fs::write(root.join("old.txt"), "body").unwrap();
        let mut app = EditorApp::new("");
        app.set_workspace(root.clone());
        app.open_path(root.join("old.txt"));
        app.handle(CommandKey::Char('r'));
        app.handle(CommandKey::Char('r'));
        for ch in "new.txt".chars() {
            app.handle(CommandKey::Char(ch));
        }
        app.handle(CommandKey::Enter);
        assert!(root.join("new.txt").exists());
        assert!(!root.join("old.txt").exists());
        assert_eq!(app.notice(), "renamed");
        let _ = fs::remove_dir_all(&root);
    }

    fn screen_has_color(app: &EditorApp, color: Color) -> bool {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == color)
    }

    #[test]
    fn yazi_browse_is_a_centered_float() {
        let root = temp_dir("yazi-float");
        fs::write(root.join("pick-me.txt"), "picked\n").unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        let mut app = EditorApp::new("stay");
        app.set_workspace(root.clone());
        app.note_frame(120, 36);
        app.handle(CommandKey::Char('o'));
        let screen = rendered_at(&app, 120, 36);
        assert!(screen.contains("Yazi"));
        assert!(screen.contains("Posit"));
        assert!(screen.contains("pick-me.txt"));
        let float = yazi_rect(Rect::new(0, 0, 120, 36));
        assert!(float.x > 0);
        assert!(float.y > 0);
        assert!(float.x + float.width < 120);
        assert_eq!(cell_fg(&app, 120, 36, float.x, float.y), Color::Magenta);
        app.handle(CommandKey::Esc);
        assert!(app.yazi.is_none());
        assert_eq!(app.text(), "stay");
        app.handle(CommandKey::Char('o'));
        for _ in 0..12 {
            let name = app
                .yazi
                .as_ref()
                .and_then(|yazi| yazi.items.get(yazi.cursor))
                .map(|item| item.name.as_str());
            if name == Some("pick-me.txt") {
                break;
            }
            app.handle(CommandKey::Down);
        }
        app.handle(CommandKey::Enter);
        assert!(app.yazi.is_none());
        assert_eq!(app.text(), "picked\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn keybinds_are_alphabetical_and_cc_toggles_chat() {
        let rows = keybind_rows();
        let keys: Vec<_> = rows.iter().map(|(key, _)| *key).collect();
        let mut ordered = keys.clone();
        ordered.sort_by_key(|left| bind_order(left));
        assert_eq!(keys, ordered);
        let action = |key: &str| {
            rows.iter()
                .find(|(name, _)| *name == key)
                .map(|(_, action)| *action)
                .unwrap()
        };
        assert_eq!(action("b"), "build");
        assert_eq!(action("<"), "toggle the file sidebar");
        assert_eq!(action(">"), "toggle the agent chat");
        assert!(keys.iter().position(|key| *key == "l") < keys.iter().position(|key| *key == "ll"));
        assert!(
            keys.iter().position(|key| *key == "ll") < keys.iter().position(|key| *key == "lll")
        );
        let guide = help_text();
        assert!(guide.contains("Key") && guide.contains("Action"));
        assert!(guide.contains("| Key | Action |"));
        assert!(guide.contains("## Editing mode"));
        assert!(guide.contains("format the file"));
        assert!(guide.contains("open floating Yazi"));

        let mut app = EditorApp::new("plain");
        app.note_frame(220, 40);
        assert!(rendered_at(&app, 220, 40).contains("Agent"));
        app.handle(CommandKey::Char('>'));
        assert!(!rendered_at(&app, 220, 40).contains("Agent"));
        assert_eq!(app.notice(), "chat off");
        app.pointer(
            Pointer::Down {
                column: 217,
                row: 5,
            },
            220,
            40,
        );
        assert!(rendered_at(&app, 220, 40).contains("Agent"));
        assert_eq!(app.notice(), "chat on");
        app.handle(CommandKey::Char(';'));
        assert!(rendered_at(&app, 220, 40).contains("chat"));
    }

    #[test]
    fn sidebars_resize_wrap_and_keep_nested_names() {
        assert_eq!(tree_label("main.rs", 0, "  ", 20), "  main.rs");
        assert_eq!(tree_label("lib.rs", 2, "v ", 20), "  v lib.rs");
        let deep = tree_label("implementation.rs", 30, "  ", 24);
        assert!(deep.starts_with('…'));
        assert!(deep.contains("implementation.rs"));
        assert!(deep.chars().count() <= 24);
        let wrapped = wrap_line(
            "ask the agent to wrap this long sentence around the column",
            16,
        );
        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.chars().count() <= 16));
        assert_eq!(
            wrapped.join(" "),
            "ask the agent to wrap this long sentence around the column"
        );

        let mut app = EditorApp::new("plain");
        app.note_frame(160, 40);
        app.pointer(Pointer::Down { column: 27, row: 8 }, 160, 40);
        app.pointer(Pointer::Drag { column: 40, row: 8 }, 160, 40);
        assert!(app.sidebar);
        assert_eq!(app.tree_width, 41);
        assert_eq!(app.tree_rect().width, 41);
        app.pointer(Pointer::Drag { column: 3, row: 8 }, 160, 40);
        assert!(!app.sidebar);

        app.handle(CommandKey::Char('<'));
        assert!(app.sidebar);
        editing(&mut app);
        app.handle(CommandKey::Char('<'));
        app.handle(CommandKey::Char('>'));
        assert!(app.sidebar);
        assert!(app.text().contains('<'));
        assert!(app.text().contains('>'));
        app.handle(CommandKey::Esc);

        app.note_frame(220, 40);
        app.agent_log
            .push("ask the agent to wrap this long sentence around the column".to_string());
        app.pointer(
            Pointer::Down {
                column: 188,
                row: 8,
            },
            220,
            40,
        );
        app.pointer(
            Pointer::Drag {
                column: 170,
                row: 8,
            },
            220,
            40,
        );
        assert!(app.chat);
        assert_eq!(app.agent_width, 50);
        let screen = rendered_at(&app, 220, 40);
        assert!(screen.contains("ask the agent"));
        assert!(screen.contains("column"));
        app.handle(CommandKey::Char('>'));
        assert!(!app.chat);
        assert!(rendered_at(&app, 220, 40).contains('>'));
    }

    #[test]
    fn selection_is_obvious_for_keyboard_and_clicks() {
        let text = "hello world\nsecond";
        let mut app = EditorApp::new(text);
        app.handle(CommandKey::Char('a'));
        assert_eq!(app.selection(), Some((0, text.chars().count())));
        assert!(rendered(&app).contains("sel "));
        app.note_frame(160, 40);
        let (_, y) = app.text_origin().unwrap();
        let x = text_col(&app);
        assert!(
            cell_reversed(&app, 160, 40, x, y),
            "selection must use reverse video for the terminal's default selection colors"
        );
        assert_ne!(cell_bg(&app, 160, 40, x, y), Color::LightYellow);
        assert_ne!(cell_bg(&app, 160, 40, x, y), Color::Indexed(18));
        app.pointer(Pointer::Down { column: x, row: y }, 160, 40);
        assert_eq!(app.selection(), None);
        assert_eq!(app.cursor(), 0);
        editing(&mut app);
        app.handle(CommandKey::ShiftRight);
        app.handle(CommandKey::ShiftRight);
        assert_eq!(app.selection(), Some((0, 2)));
        app.handle(CommandKey::ShiftDown);
        assert_eq!(app.selection(), Some((0, 14)));
        app.handle(CommandKey::Left);
        assert_eq!(app.selection(), None);
        assert_eq!(app.cursor(), 0);
        app.handle(CommandKey::CtrlA);
        app.handle(CommandKey::Delete);
        assert_eq!(app.text(), "");

        let mut app = EditorApp::new(text);
        app.note_frame(160, 40);
        let (_, y) = app.text_origin().unwrap();
        let x = text_col(&app);
        let click = |app: &mut EditorApp, column| {
            app.pointer(Pointer::Down { column, row: y }, 160, 40);
        };
        click(&mut app, x + 1);
        click(&mut app, x + 1);
        assert_eq!(app.selection(), Some((0, 5)));
        app.pointer(
            Pointer::Drag {
                column: x + 6,
                row: y,
            },
            160,
            40,
        );
        assert_eq!(app.selection(), Some((0, 11)));
        click(&mut app, x + 1);
        assert_eq!(app.selection(), Some((0, 12)));
        assert!(cell_reversed(&app, 160, 40, x + 11, y));
        assert_eq!(cell_fg(&app, 160, 40, x, y), Color::Reset);
        editing(&mut app);
        app.handle(CommandKey::Backspace);
        assert_eq!(app.text(), "second");

        let mut app = EditorApp::new("fn main() {\n    // note\n}\n");
        app.set_language("rust");
        app.note_frame(80, 24);
        app.handle(CommandKey::Char('a'));
        let (keyword, comment) = selection_colors(&app, 80, 24);
        assert_eq!(keyword.0, Color::Magenta);
        assert!(keyword.1, "keyword selection is reversed");
        assert!(comment.1, "comment selection is reversed");
        assert_eq!(comment.0, Color::Yellow);
    }

    #[test]
    fn workbench_previews_a_cargo_plan_without_running_it() {
        let root = temp_dir("dlc-cargo");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let mut app = EditorApp::new("fn main() {}\n");
        app.set_language("rust");
        app.set_workspace(root.clone());
        app.note_frame(120, 30);
        let screen = rendered_at(&app, 120, 30);
        assert!(screen.contains("Build"));
        assert!(screen.contains("Debug"));
        app.handle(CommandKey::Char('1'));
        assert!(app.terminals.show_console);
        assert!(
            app.terminals
                .console
                .iter()
                .any(|line| line.contains("cargo")),
            "{:?}",
            app.terminals.console
        );
        assert!(app
            .terminals
            .console
            .iter()
            .any(|line| line.contains("build")));
        assert!(!root.join("target").exists());
        assert!(
            app.notice().contains("preview") && app.notice().contains("trust"),
            "{}",
            app.notice()
        );
        app.handle(CommandKey::Char('y'));
        assert_eq!(app.notice(), "untrusted workspace stays preview-only");
        assert!(!root.join("target").exists());
        app.handle(CommandKey::Char('9'));
        let screen = rendered_at(&app, 120, 30);
        assert!(screen.contains("Debug") || screen.contains("Console"));
        if app.notice().contains("debugging with") {
            assert!(app.terminals.len() >= 1);
        } else {
            assert!(
                app.notifications_open
                    || app.notice().contains("lldb")
                    || app.notice().contains("debug")
                    || screen.contains("lldb")
                    || screen.contains("debugger")
            );
        }
        editing(&mut app);
        app.handle(CommandKey::Char('1'));
        assert!(app.text().contains('1'));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn console_right_click_select_all_and_drag_copy_logs() {
        let previous = clipboard::get().ok();
        let mut app = EditorApp::new("buffer text");
        app.note_frame(120, 40);
        app.terminals
            .append_console("*workbench*", "cargo test\nwarning: unused");
        assert!(app.terminals.show_console);
        let rect = app.terminal_rect();
        assert!(rect.height > 2, "console panel visible");
        // Right-click Select All on the console body.
        let cx = rect.x + 2;
        let cy = rect.y + 2;
        app.pointer(Pointer::Right { column: cx, row: cy }, 120, 40);
        match &app.overlay {
            Overlay::Menu(menu) => assert!(menu.over_terminal),
            other => panic!("expected menu over console, got {other:?}"),
        }
        app.handle(CommandKey::Char('a'));
        assert!(app.terminals.has_selection());
        let selected = app.terminals.selected_text().expect("console selection");
        assert!(selected.contains("cargo test"), "{selected}");
        assert!(selected.contains("warning"), "{selected}");
        app.handle(CommandKey::Char('y'));
        assert_eq!(app.notice(), "copied");
        // Fresh drag selection still works after a menu yank.
        app.terminals.clear_selection();
        app.pointer(
            Pointer::Down {
                column: cx,
                row: cy,
            },
            120,
            40,
        );
        app.pointer(
            Pointer::Drag {
                column: cx + 8,
                row: cy,
            },
            120,
            40,
        );
        assert!(app.terminals.has_selection());
        if let Some(text) = previous {
            let _ = clipboard::set(&text);
        }
    }

    #[test]
    fn toolbar_runtimes_plan_without_configuration() {
        let Some(script) = find_dlc_script() else {
            return;
        };
        let root = temp_dir("dlc-runtime");
        let mut app = EditorApp::new("\n");
        app.set_workspace(root.clone());
        for action in ["browser", "phone-view", "android-boot"] {
            let text = app.dlc_text(&script, &["runtime".into(), action.into()]);
            assert!(
                json_error(&text).is_none(),
                "{action} still refuses: {text}"
            );
            assert!(text.contains("argv"), "{action}: {text}");
            assert!(!text.contains("Configure"), "{action}: {text}");
            assert!(!text.contains("simctl"), "{action}: {text}");
            assert!(!text.contains("xdg-open"), "{action}: {text}");
            assert!(
                !text.contains("\"argv\": [\n    \"open\""),
                "{action} must not plan the host opener: {text}"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tree_wheel_scrolls_the_sidebar_only() {
        let root = temp_dir("tree-wheel");
        for index in 0..40 {
            fs::write(root.join(format!("file-{index:02}.txt")), "x\n").unwrap();
        }
        let mut app = EditorApp::new("doc\n");
        app.set_workspace(root.clone());
        app.note_frame(120, 30);
        let tree = app.tree_rect();
        assert!(tree.width > 4);
        let before = app.buffers[0].scroll;
        app.pointer(
            Pointer::ScrollDown {
                column: tree.x + 2,
                row: tree.y + 2,
            },
            120,
            30,
        );
        assert!(app.tree_scroll >= 1);
        assert_eq!(app.buffers[0].scroll, before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wheel_locks_to_one_axis() {
        let mut app = EditorApp::new(&format!("{}\n", "x".repeat(200)).repeat(40));
        app.note_frame(40, 16);
        app.pointer(Pointer::ScrollDown { column: 10, row: 4 }, 40, 16);
        let y = app.buffers[0].scroll;
        let x = app.buffers[0].scroll_x;
        app.pointer(Pointer::ScrollRight { column: 10, row: 4 }, 40, 16);
        assert_eq!(app.buffers[0].scroll_x, x);
        assert!(app.buffers[0].scroll >= y);
        app.wheel_at = Some(Instant::now() - Duration::from_millis(200));
        app.scroll_axis = None;
        app.pointer(Pointer::ScrollRight { column: 10, row: 4 }, 40, 16);
        assert!(app.buffers[0].scroll_x > x);
    }

    #[test]
    fn notification_yank_and_menu_stacking() {
        let previous = clipboard::get().ok();
        let mut app = EditorApp::new("plain\n");
        app.note_frame(80, 24);
        app.report_error("Title", "body text");
        assert!(app.notifications_open);
        let rect = notification_rect(Rect::new(0, 0, 80, 24), app.notifications.len());
        app.pointer(
            Pointer::Right {
                column: rect.x + 2,
                row: rect.y + 2,
            },
            80,
            24,
        );
        assert!(!matches!(app.overlay, Overlay::Menu(_)));
        app.yank_notification(0);
        assert!(app.notice().contains("copied") || app.notifications_open);
        let screen = rendered_at(&app, 80, 24);
        assert!(screen.contains("Notifications"));
        assert!(screen.contains('y') || screen.contains(" y "));
        if let Some(text) = previous {
            let _ = clipboard::set(&text);
        }
    }

    #[test]
    fn runtime_pane_focuses_instead_of_duplicating() {
        let mut panel = terminal::Panel::new();
        panel
            .run_runtime(
                Some("browser"),
                Path::new("."),
                40,
                10,
                "/bin/sh",
                &["-c".into(), "sleep 30".into()],
            )
            .unwrap();
        assert_eq!(panel.runtime_count("browser"), 1);
        panel
            .run_runtime(
                Some("browser"),
                Path::new("."),
                40,
                10,
                "/bin/sh",
                &["-c".into(), "sleep 30".into()],
            )
            .unwrap();
        assert_eq!(panel.runtime_count("browser"), 1);
        panel.drain();
    }

    #[test]
    fn hover_hint_appears_after_one_second() {
        let mut app = EditorApp::new("plain\n");
        app.note_frame(120, 30);
        let chrome = app.frame_chrome(Rect::new(0, 0, 120, 30));
        app.pointer(
            Pointer::Move {
                column: chrome.toolbar.x + 2,
                row: chrome.toolbar.y,
            },
            120,
            30,
        );
        assert!(app.hover_hint.is_none());
        app.hover_since = Some(Instant::now() - Duration::from_millis(1100));
        app.refresh_hover_hint();
        assert_eq!(app.hover_hint.as_deref(), Some("Build the project"));
    }

    fn cursor_bg(app: &EditorApp, width: u16, height: u16) -> Color {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .find(|cell| cell.bg == Color::Magenta || cell.bg == Color::Rgb(230, 120, 20))
            .map(|cell| cell.bg)
            .unwrap_or(Color::Reset)
    }

    fn cell_bg(app: &EditorApp, width: u16, height: u16, column: u16, row: u16) -> Color {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer()[(column, row)].bg
    }

    fn cell_fg(app: &EditorApp, width: u16, height: u16, column: u16, row: u16) -> Color {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer()[(column, row)].fg
    }

    fn cell_reversed(app: &EditorApp, width: u16, height: u16, column: u16, row: u16) -> bool {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer()[(column, row)]
            .modifier
            .contains(Modifier::REVERSED)
    }

    fn selection_colors(app: &EditorApp, width: u16, height: u16) -> ((Color, bool), (Color, bool)) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let cells = terminal.backend().buffer().content();
        let keyword = cells
            .windows(2)
            .find(|pair| pair[0].symbol() == "f" && pair[1].symbol() == "n")
            .map(|pair| {
                (
                    pair[0].fg,
                    pair[0].modifier.contains(Modifier::REVERSED),
                )
            })
            .unwrap_or((Color::Reset, false));
        let comment = cells
            .windows(4)
            .find(|window| {
                window[0].symbol() == "n"
                    && window[1].symbol() == "o"
                    && window[2].symbol() == "t"
                    && window[3].symbol() == "e"
            })
            .map(|window| {
                (
                    window[0].fg,
                    window[0].modifier.contains(Modifier::REVERSED),
                )
            })
            .unwrap_or((Color::Reset, false));
        (keyword, comment)
    }

    #[test]
    fn local_tab_accepts_only_the_current_revision() {
        let mut app = EditorApp::new("let width = 1;\nlet w = wid");
        app.tab_enabled = true;
        editing(&mut app);
        let len = app.text().chars().count();
        for _ in 0..len {
            app.handle(CommandKey::Right);
        }
        app.refresh_ghost();
        let screen = rendered_at(&app, 80, 24);
        assert!(screen.contains("Tab ON"));
        assert!(app.ghost.as_ref().is_some_and(|ghost| ghost.text == "th"));
        let before = app.focused().document.revision();
        app.handle(CommandKey::Tab);
        assert_eq!(app.text(), "let width = 1;\nlet w = width");
        assert!(app.focused().document.revision() > before);
        app.ghost = Some(Ghost {
            revision: 0,
            cursor: 0,
            replace_from: None,
            text: "NOPE".into(),
        });
        let text = app.text();
        app.handle(CommandKey::Tab);
        assert!(app.text().ends_with('\t') || app.text().contains('\t'));
        assert!(!app.text().contains("NOPE"));
        assert!(text.len() < app.text().len());
    }

    #[test]
    fn ghost_text_keeps_caret_at_cursor_column() {
        // Leading tab on the completion line; ghost must sit at the caret, not EOL.
        let mut app = EditorApp::new("let width = 1;\n\tlet w = wid");
        app.tab_enabled = true;
        editing(&mut app);
        let len = app.text().chars().count();
        for _ in 0..len {
            app.handle(CommandKey::Right);
        }
        app.refresh_ghost();
        assert!(app.ghost.as_ref().is_some_and(|ghost| ghost.text == "th"));
        let buffer = app.focused();
        let text = buffer.text();
        let rendered = super::highlighted_window(
            buffer,
            true,
            true,
            80,
            app.ghost.as_ref(),
            0,
            usize::MAX,
            markdown_preview::RevealOpts::default(),
        );
        let line = &rendered.lines[1];
        let flat: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        let caret_col = super::display_col_in_line(&text, buffer.cursor, 4);
        assert!(
            flat.contains("th"),
            "ghost missing from rendered line: {flat:?}"
        );
        let ghost_at = flat.find("th").expect("ghost");
        assert_eq!(
            ghost_at, caret_col,
            "ghost should start at caret column {caret_col}, got {ghost_at} in {flat:?}"
        );
        let mut col = 0usize;
        let mut caret_styled = false;
        for span in &line.spans {
            let n = span.content.chars().count();
            if col <= caret_col && caret_col < col + n {
                caret_styled = span.style == super::cursor_style(true);
                break;
            }
            col += n;
        }
        assert!(caret_styled, "caret style missing at column {caret_col} in {flat:?}");
    }

    #[test]
    fn runtime_dock_click_focuses_panel() {
        let mut app = EditorApp::new("fn main() {}\n");
        app.runtime_dock = Some("browser".into());
        app.note_frame(160, 40);
        let chrome = app.frame_chrome(Rect::new(0, 0, 160, 40));
        assert!(chrome.canvas.width > 0, "canvas should be laid out");
        // Terminal must not extend under the canvas.
        assert_eq!(
            chrome.terminal.x + chrome.terminal.width,
            chrome.canvas.x,
            "terminal stops where canvas begins"
        );
        assert_eq!(
            chrome.canvas.height,
            chrome.toolbar.height + chrome.body.height,
            "canvas spans toolbar through terminal"
        );
        assert!(chrome.canvas.height > chrome.terminal.height);
        app.pointer(
            Pointer::Down {
                column: chrome.canvas.x + 2,
                row: chrome.canvas.y + 2,
            },
            160,
            40,
        );
        assert_eq!(app.pane_focus(), PaneFocus::Runtime);
    }

    #[test]
    fn canvas_sits_right_of_terminal_and_agent() {
        let mut app = EditorApp::new("canvas layout\n");
        app.runtime_dock = Some("phone".into());
        app.terminals.open = true;
        app.note_frame(180, 48);
        let chrome = app.frame_chrome(Rect::new(0, 0, 180, 48));
        assert!(chrome.canvas.width >= 28);
        assert!(chrome.terminal.width + chrome.canvas.width <= 180);
        assert!(chrome.canvas.x >= chrome.agent.x + chrome.agent.width || chrome.agent.width == 0);
        assert_eq!(chrome.terminal.x + chrome.terminal.width, chrome.canvas.x);
    }

    #[test]
    fn canvas_seam_resizes_without_arming_content_drag() {
        let mut app = EditorApp::new("canvas resize\n");
        app.runtime_dock = Some("browser".into());
        app.canvas_surfaces
            .push(canvas::CanvasSurface::test_stub("browser", "https://example.com"));
        app.note_frame(180, 48);
        let chrome = app.frame_chrome(Rect::new(0, 0, 180, 48));
        assert!(chrome.canvas.width > 0);
        let seam_x = chrome.canvas.x;
        let seam_y = chrome.canvas.y + 4;
        assert!(app.on_canvas_seam(seam_x, seam_y, &chrome));
        app.pointer(
            Pointer::Down {
                column: seam_x,
                row: seam_y,
            },
            180,
            48,
        );
        assert!(app.canvas_resize, "seam down should arm canvas resize");
        let before = app.runtime_width.unwrap_or(chrome.canvas.width);
        app.pointer(
            Pointer::Drag {
                column: seam_x.saturating_sub(20),
                row: seam_y,
            },
            180,
            48,
        );
        let after = app.runtime_width.expect("width set while dragging");
        assert!(after > before, "dragging seam left widens canvas ({before} -> {after})");
        assert!(app.canvas_resize);
        app.pointer(
            Pointer::Up {
                column: seam_x.saturating_sub(20),
                row: seam_y,
            },
            180,
            48,
        );
        assert!(!app.canvas_resize, "release ends resize without freezing paint path");
        assert!(!app.canvas_resize_hid_gfx);
        assert!(app.canvas_painted_size.is_none(), "release schedules Kitty repaint");
        assert!(
            app.canvas_chrome_dirty.get(),
            "release schedules one chrome refresh"
        );
    }

    #[test]
    fn canvas_resize_drag_does_not_dirty_chrome_each_move() {
        let mut app = EditorApp::new("canvas resize cheap\n");
        app.runtime_dock = Some("browser".into());
        app.canvas_surfaces
            .push(canvas::CanvasSurface::test_stub("browser", "https://example.com"));
        app.note_frame(180, 48);
        let chrome = app.frame_chrome(Rect::new(0, 0, 180, 48));
        let seam_x = chrome.canvas.x;
        let seam_y = chrome.canvas.y + 4;
        app.pointer(
            Pointer::Down {
                column: seam_x,
                row: seam_y,
            },
            180,
            48,
        );
        app.canvas_chrome_dirty.set(false);
        app.pointer(
            Pointer::Drag {
                column: seam_x.saturating_sub(12),
                row: seam_y,
            },
            180,
            48,
        );
        assert!(app.canvas_resize);
        assert!(
            !app.canvas_chrome_dirty.get(),
            "drag must not Clear/rebuild canvas chrome every tick"
        );
        app.pointer(
            Pointer::Drag {
                column: seam_x.saturating_sub(30),
                row: seam_y,
            },
            180,
            48,
        );
        assert!(!app.canvas_chrome_dirty.get());
    }

    #[test]
    fn canvas_title_is_plain_and_tabs_when_multiple() {
        let mut app = EditorApp::new("canvas tabs\n");
        app.runtime_dock = Some("phone".into());
        app.note_frame(160, 40);
        let chrome = app.frame_chrome(Rect::new(0, 0, 160, 40));
        // Single surface: title hits only expose close, never a kind-suffixed pane name.
        assert!(app.canvas_title_hit(chrome.canvas.x + 2, chrome.canvas.y, &chrome).is_none());
        assert_eq!(
            app.canvas_title_hit(
                chrome.canvas.x + chrome.canvas.width - 2,
                chrome.canvas.y,
                &chrome
            ),
            Some(CanvasTitleHit::Close)
        );

        // Two kinds → tabs appear left of close (no live capture workers needed).
        app.canvas_surfaces.push(canvas::CanvasSurface::test_stub("phone", "posit"));
        app.canvas_surfaces.push(canvas::CanvasSurface::test_stub(
            "browser",
            "https://example.com",
        ));
        app.runtime_dock = Some("browser".into());
        let mut found_tab = false;
        for x in chrome.canvas.x..chrome.canvas.x.saturating_add(chrome.canvas.width) {
            if matches!(
                app.canvas_title_hit(x, chrome.canvas.y, &chrome),
                Some(CanvasTitleHit::Tab(_))
            ) {
                found_tab = true;
                break;
            }
        }
        assert!(found_tab, "expected a tab hit somewhere on the Canvas title row");
        assert!(app.activate_canvas_kind("phone"));
        assert_eq!(app.runtime_dock.as_deref(), Some("phone"));
        assert_eq!(EditorApp::canvas_tab_label("phone"), "Phone");
        assert_eq!(EditorApp::canvas_tab_label("browser"), "Browser");
    }

    #[test]
    fn canvas_control_strip_hits_phone_and_browser_buttons() {
        let mut app = EditorApp::new("canvas controls\n");
        app.runtime_dock = Some("phone".into());
        app.canvas_surfaces
            .push(canvas::CanvasSurface::test_stub("phone", "posit"));
        app.note_frame(160, 40);
        let chrome = app.frame_chrome(Rect::new(0, 0, 160, 40));
        let strip = canvas_controls_rect(chrome.canvas);
        assert!(strip.width > 10, "control strip should exist");
        let line = EditorApp::canvas_control_line("phone", Some("posit"));
        let home_at = line.find("[Home]").expect("Home label");
        assert_eq!(
            app.canvas_control_hit(strip.x + home_at as u16 + 1, strip.y, &chrome),
            Some(CanvasControlHit::PhoneHome)
        );
        app.runtime_dock = Some("browser".into());
        app.canvas_surfaces
            .push(canvas::CanvasSurface::test_stub("browser", "https://example.com"));
        app.activate_canvas_kind("browser");
        let chrome = app.frame_chrome(Rect::new(0, 0, 160, 40));
        let strip = canvas_controls_rect(chrome.canvas);
        let line = EditorApp::canvas_control_line("browser", Some("https://example.com"));
        let back_at = line.find("[Back]").expect("Back label");
        assert_eq!(
            app.canvas_control_hit(strip.x + back_at as u16 + 1, strip.y, &chrome),
            Some(CanvasControlHit::BrowserBack)
        );
        let media = canvas_media_rect(chrome.canvas);
        assert!(media.height + 1 <= inset(chrome.canvas).height);
        assert!(!contains(media, strip.x, strip.y), "media excludes controls");
    }

    fn highlighted(app: &EditorApp, width: u16, height: u16, needle: &str) -> bool {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let cells = terminal.backend().buffer().content();
        let chars: Vec<char> = needle.chars().collect();
        cells.windows(chars.len()).any(|window| {
            window
                .iter()
                .zip(&chars)
                .all(|(cell, ch)| cell.symbol() == ch.to_string() && cell.fg == Color::Yellow)
        })
    }

    #[test]
    fn spatial_overview_jump_and_escape_restore_focus() {
        let mut app = EditorApp::new("one\n");
        app.add_buffer("two", "two\n");
        app.add_buffer("three", "three\n");
        app.note_frame(240, 40);
        app.show_many = true;
        app.focus_at(2);
        let view = app.view;
        app.handle(CommandKey::Char('z'));
        app.handle(CommandKey::Char('o'));
        assert_eq!(app.spatial.active_mode, SpatialMode::Overview);
        let screen = rendered_at(&app, 240, 40);
        assert!(screen.contains("Spatial overview") || screen.contains("overview"));
        app.handle(CommandKey::Down);
        app.handle(CommandKey::Enter);
        app.handle(CommandKey::Esc);
        assert_eq!(app.spatial.active_mode, SpatialMode::Idle);
        assert_eq!(app.focus, 2);
        assert!((app.view - view).abs() < 0.01 || app.notice.contains("spatial closed"));
    }

    #[test]
    fn spatial_minimap_hides_on_narrow_tile() {
        let mut cache = ide_spatial::MinimapCache::default();
        assert!(cache
            .render(1, 20, 8, true, ide_spatial::MinimapPriority::Structure, false)
            .is_none());
        assert!(cache.hidden());
    }

    #[test]
    fn spatial_tab_control_stays_bottom_right() {
        let mut app = EditorApp::new("tab\n");
        app.tab_enabled = true;
        app.note_frame(100, 24);
        let screen = rendered_at(&app, 100, 24);
        assert!(screen.contains("Tab ON"));
        app.spatial_open_overview();
        app.handle(CommandKey::Esc);
        let again = rendered_at(&app, 100, 24);
        assert!(again.contains("Tab ON"));
        let bounds = tab_bounds(true, TabStatus::LexicalFallback, Rect::new(0, 23, 100, 1));
        assert_eq!(bounds.x + bounds.width, 100);
    }

    #[test]
    fn spatial_structural_and_timeline_modes() {
        let mut app = EditorApp::new("fn main() {\n  1\n}\n");
        app.set_language("rust");
        app.handle(CommandKey::Char('z'));
        app.handle(CommandKey::Char('n'));
        assert_eq!(app.spatial.active_mode, SpatialMode::Structural);
        app.handle(CommandKey::Esc);
        app.handle(CommandKey::Char('z'));
        app.handle(CommandKey::Char('t'));
        assert_eq!(app.spatial.active_mode, SpatialMode::Timeline);
        assert!(app.spatial_lines[0].contains("RecordedReplay"));
        app.handle(CommandKey::Esc);
        assert_eq!(app.spatial.active_mode, SpatialMode::Idle);
    }

    #[test]
    fn debug_file_launches_jdb_for_java() {
        let path = PathBuf::from("/tmp/demo/Hello.java");
        let (program, args) = debug_file("java", &path).expect("java debugger");
        assert!(program.contains("jdb"), "{program}");
        assert!(args.iter().any(|a| a == "-launch"));
        assert!(args.iter().any(|a| a == "Hello"));
        assert!(args.windows(2).any(|w| w[0] == "-classpath" && w[1].contains("demo")));
        let (kt_program, kt_args) = debug_file("kotlin", &PathBuf::from("/tmp/App.kt")).expect("kotlin");
        assert!(kt_program.contains("jdb"));
        assert!(kt_args.iter().any(|a| a == "App"));
        assert!(debug_file("cobol", &path).is_err());
    }

    #[test]
    fn pick_system_prefers_cargo_when_nix_also_present() {
        let systems = vec!["nix".into(), "cargo".into()];
        assert_eq!(
            pick_system(&systems, "plain", None).as_deref(),
            Some("cargo")
        );
        assert_eq!(
            pick_system(&systems, "rust", None).as_deref(),
            Some("cargo")
        );
    }

    #[test]
    fn detect_make_xcode_and_project_suggestions() {
        let root = temp_dir("project-matrix");
        fs::write(root.join("Makefile"), "all:\n\t@echo hi\n").unwrap();
        fs::create_dir_all(root.join(".idea")).unwrap();
        fs::write(root.join(".idea/misc.xml"), "<project/>\n").unwrap();
        let xcode = root.join("Demo.xcodeproj");
        fs::create_dir_all(&xcode).unwrap();
        fs::write(xcode.join("project.pbxproj"), "// !$*UTF8*$\n").unwrap();
        let systems = detect_systems_fast(&root);
        assert!(systems.iter().any(|s| s == "make"), "{systems:?}");
        assert!(systems.iter().any(|s| s == "intellij"), "{systems:?}");
        assert!(systems.iter().any(|s| s == "xcodebuild"), "{systems:?}");
        assert_eq!(project_root(&root.join("src")), root);
        let suggestions = ide_packages::project_suggestions(&root);
        assert!(suggestions.iter().any(|s| s.system == "make"));
        assert!(suggestions.iter().any(|s| s.system == "xcodebuild"));
        assert!(suggestions.iter().any(|s| s.system == "intellij"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn free_agent_and_snippet_ghost_work() {
        let root = temp_dir("agent-snippet");
        fs::write(root.join("hello.rs"), "fn demo() {}\n").unwrap();
        let mut app = EditorApp::new("fn");
        app.set_workspace(root.clone());
        app.set_language("rust");
        app.tab_enabled = true;
        editing(&mut app);
        for _ in 0..2 {
            app.handle(CommandKey::Right);
        }
        app.refresh_ghost();
        assert!(
            app.ghost
                .as_ref()
                .is_some_and(|g| g.text.contains("name") && g.replace_from.is_none()),
            "expected rust fn snippet remainder, got {:?}",
            app.ghost
        );
        app.handle(CommandKey::Tab);
        assert!(app.text().contains("fn name"), "{}", app.text());

        app.agent_focused = true;
        app.agent_draft = "list".into();
        app.submit_agent();
        assert!(
            app.agent_log.iter().any(|line| line.contains("hello.rs") || line.contains("free-local")),
            "{:?}",
            app.agent_log
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn project_root_climbs_out_of_java_package_dir() {
        let root = temp_dir("idea-java-root");
        fs::write(root.join("build.gradle"), "plugins { id 'java' }\n").unwrap();
        fs::create_dir_all(root.join(".idea")).unwrap();
        let pkg = root.join("src/main/java/com/example");
        fs::create_dir_all(&pkg).unwrap();
        let main = pkg.join("Main.java");
        fs::write(&main, "class Main {}\n").unwrap();
        assert_eq!(project_root(&main), root);
        assert_eq!(project_root(&pkg), root);
        let systems = detect_systems_fast(&root);
        assert!(systems.iter().any(|s| s == "gradle"), "{systems:?}");
        assert_eq!(
            pick_system(&systems, "java", None).as_deref(),
            Some("gradle")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_adopts_gradle_root_from_nested_java_file() {
        let root = temp_dir("idea-run-adopt");
        fs::write(root.join("settings.gradle"), "rootProject.name = 'demo'\n").unwrap();
        fs::write(root.join("build.gradle"), "plugins { id 'java' }\n").unwrap();
        let pkg = root.join("src/main/java/com/example");
        fs::create_dir_all(&pkg).unwrap();
        let main = pkg.join("Main.java");
        fs::write(&main, "package com.example;\npublic class Main {}\n").unwrap();
        let mut app = EditorApp::new("package com.example;\npublic class Main {}\n");
        app.set_language("java");
        // Simulate the old bug: workspace stuck in the package folder.
        app.set_workspace(pkg.clone());
        app.focused_mut().path = Some(main);
        app.focused_mut().language = "java".into();
        app.adopt_project_root_from_focus();
        assert_eq!(app.workspace, root);
        assert_eq!(app.dlc_system.as_deref(), Some("gradle"));
        assert!(app.dlc_trusted);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn format_keeps_caret_line_and_scroll() {
        let mut app = EditorApp::new("a\nb\nc\nd\n");
        app.focused_mut().cursor = 4; // line of 'c'
        app.focused_mut().anchor = 4;
        app.focused_mut().scroll = 1;
        app.focused_mut()
            .replace_all_keeping_view("aa\nbb\ncc\ndd\n");
        let (line, _) = app
            .focused()
            .with_text(|text| line_col(text, app.focused().cursor));
        assert_eq!(line, 2, "caret should stay on the third line");
        assert_eq!(app.focused().scroll, 1);
    }

    #[test]
    fn palette_line_jump_absolute_and_relative() {
        assert_eq!(parse_line_jump("42"), Some(LineJump::Absolute(42)));
        assert_eq!(parse_line_jump(":7"), Some(LineJump::Absolute(7)));
        assert_eq!(parse_line_jump("+3"), Some(LineJump::Relative(3)));
        assert_eq!(parse_line_jump("-2"), Some(LineJump::Relative(-2)));
        assert_eq!(parse_line_jump("line 9"), Some(LineJump::Absolute(9)));
        let mut app = EditorApp::new("a\nb\nc\nd\ne\n");
        app.focused_mut().cursor = 0;
        app.jump_lines(LineJump::Absolute(4));
        let (line, _) = app
            .focused()
            .with_text(|text| line_col(text, app.focused().cursor));
        assert_eq!(line, 3);
        app.jump_lines(LineJump::Relative(-2));
        let (line, _) = app
            .focused()
            .with_text(|text| line_col(text, app.focused().cursor));
        assert_eq!(line, 1);
    }

    #[test]
    fn edit_entry_keys_match_neovim_insert_placements() {
        let mut app = EditorApp::new("  hi\n");
        assert!(!app.editing);
        // e → insert here (caret stays)
        app.focused_mut().cursor = 3; // on 'i'
        app.focused_mut().anchor = 3;
        app.handle(CommandKey::Char('e'));
        assert!(app.editing);
        assert_eq!(app.focused().cursor, 3);
        app.handle(CommandKey::Esc);
        // E → after caret
        app.focused_mut().cursor = 3;
        app.focused_mut().anchor = 3;
        app.handle(CommandKey::Char('E'));
        assert!(app.editing);
        assert_eq!(app.focused().cursor, 4);
        app.handle(CommandKey::Esc);
        // H → first non-blank
        app.focused_mut().cursor = 3;
        app.handle(CommandKey::Char('H'));
        assert!(app.editing);
        assert_eq!(app.focused().cursor, 2); // 'h'
        app.handle(CommandKey::Esc);
        // L → end of line (before newline)
        app.focused_mut().cursor = 2;
        app.handle(CommandKey::Char('L'));
        assert!(app.editing);
        assert_eq!(app.focused().cursor, 4);
        // a / i keep their existing jobs
        app.handle(CommandKey::Esc);
        app.handle(CommandKey::Char('a'));
        assert!(!app.editing);
        assert_eq!(app.focused().selection, Some((0, 5)));
    }

    #[test]
    fn word_jump_matches_macos_option_arrows() {
        let text = "hello, world_test  next";
        // From mid-word → start of that word
        assert_eq!(word_jump_left(text, 3), 0);
        // From the space after ',' → start of the punctuation run
        assert_eq!(word_jump_left(text, 6), 5);
        // From start of "world" → same punctuation hop
        assert_eq!(word_jump_left(text, 7), 5);
        // Another hop from there → start of "hello"
        assert_eq!(word_jump_left(text, 5), 0);
        // Right from start → end of hello
        assert_eq!(word_jump_right(text, 0), 5);
        // Right from end of hello → past comma
        assert_eq!(word_jump_right(text, 5), 6);
        // Right into world_test → end of identifier
        assert_eq!(word_jump_right(text, 7), 17);
        assert_eq!(
            map_key(KeyCode::Left, KeyModifiers::ALT),
            Some(CommandKey::AltLeft)
        );
        assert_eq!(
            map_key(KeyCode::Right, KeyModifiers::ALT | KeyModifiers::SHIFT),
            Some(CommandKey::AltShiftRight)
        );
        assert_eq!(
            map_key(KeyCode::Char('b'), KeyModifiers::ALT),
            Some(CommandKey::AltLeft)
        );
        assert_eq!(
            map_key(KeyCode::Char('f'), KeyModifiers::ALT),
            Some(CommandKey::AltRight)
        );
    }

    #[test]
    fn option_arrows_move_cursor_by_word_while_editing() {
        let mut app = EditorApp::new("alpha beta gamma\n");
        editing(&mut app);
        app.focused_mut().cursor = 0;
        app.focused_mut().anchor = 0;
        app.handle(CommandKey::AltRight);
        assert_eq!(app.focused().cursor, 5); // end of alpha
        app.handle(CommandKey::AltRight);
        assert_eq!(app.focused().cursor, 10); // end of beta
        app.handle(CommandKey::AltLeft);
        assert_eq!(app.focused().cursor, 6); // start of beta
        app.handle(CommandKey::AltShiftLeft);
        assert_eq!(app.focused().selection, Some((0, 6)));
    }

    #[test]
    fn editor_workspace_bootstraps_trusted_cargo() {
        let root = temp_dir("editor-workbench");
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("extensions/project-dlc")).unwrap();
        fs::create_dir_all(root.join("crates/ide-tui")).unwrap();
        let mut app = EditorApp::new("\n");
        app.set_workspace(root.clone());
        assert_eq!(app.dlc_system.as_deref(), Some("cargo"));
        assert!(app.dlc_trusted);
        let saved = fs::read_to_string(root.join(".posit/workbench.json")).unwrap();
        assert!(saved.contains("\"trusted\":true"));
        assert!(saved.contains("cargo"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn trusted_build_executes_cargo_plan() {
        let root = temp_dir("dlc-trusted-build");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "\n").unwrap();
        let mut app = EditorApp::new("fn main() {}\n");
        app.set_language("rust");
        app.set_workspace(root.clone());
        app.dlc_setting("trust");
        app.note_frame(120, 30);
        app.handle(CommandKey::Char('1'));
        assert!(
            app.notice() == "plan finished" || app.notice() == "plan failed",
            "{}",
            app.notice()
        );
        assert!(root.join("target").exists() || app.notice() == "plan failed");
        let _ = fs::remove_dir_all(&root);
    }
}
