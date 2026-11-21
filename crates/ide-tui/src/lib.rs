//! Terminal editor with scrolling 80-column tiles, a file tree, and Yazi.
//! Drawing and commands are testable without a tty.
//! The interactive loop restores the terminal on return and on drop.
mod clipboard;
mod files;
mod finder;
mod layout;
mod terminal;

use std::collections::HashSet;
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
    EnterAlternateScreen, LeaveAlternateScreen,
};
use files::ChooseError;
use ide_core::{Document, Edit};
use ide_dlc_ui::{authorize, toolbar_hit, Trust, TrustAction, WorkbenchAction, WorkbenchToolbar};
use ide_navigation::{FuzzyHit, GrepHit};
use ide_syntax::{format_manually, highlight, FormatOutcome, LanguageProfile, TokenKind};
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
    scroll_pos: f32,
    scroll_vel: f32,
    anchor: usize,
    selection: Option<(usize, usize)>,
    language: String,
    saved: String,
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
    terminals: terminal::Panel,
    notifications: Vec<Notice>,
    notifications_open: bool,
    tile_spans: Option<Vec<u16>>,
    tile_drag: Option<usize>,
    terminal_rows: Option<u16>,
    terminal_drag: bool,
    pixels: Option<(u16, u16)>,
}

const HELP_INTRO: &str = "\
How to use Posit Edit

Posit IDE is this terminal editor.
The yellow ? in the title bar opens and closes this guide.
The q beside it quits. Click either one, or the ? help label on the status line.
F1 opens this guide. Esc closes it.

Buffers
  A wide screen lays files out left to right. Columns cap at 80 columns
  once enough files are open to fill the width.
  80 columns or less: one buffer, as wide as the window.
  81 to 160: two columns, when two files are open.
  161 to 240: three columns. Wider windows add another every 80.
  Fewer open files grow to fill the window instead of leaving empty space.
  Files beyond the visible columns stay on the strip. Focus scrolls to them.
  Drag the border between buffers to widen one when a line runs past the edge.
  Fullscreen keeps a single buffer, so that border is not there.

  When the screen is taller than it is wide, files stack from top to bottom
  and use the full width. A terminal cell is about twice as tall as it is wide,
  so this follows the window, not the raw row count.
  Left and right move between buffers. Up and down scroll the file.
  Those keys do not trade jobs when the files are stacked.

Editing
  The editor starts in view mode. Letters run commands.
  Enter on the focused buffer starts editing. Esc returns to view mode.
  Click places the cursor. Drag selects. Double-click selects a word.
  Triple-click selects the line. Shift+arrows select while editing.
  Ctrl-A selects all. In view mode, a does too.
  y copies the selection. p pastes. x cuts. Right-click opens the same actions.
  A second letter extends a bind. Wait, and the short bind runs.

Sidebars
  < opens and closes the file tree. > opens and closes the agent chat.
  Both are view-mode keys. Drag the inner edge of a sidebar to resize it.
  Deep folders keep the file name visible instead of indenting it away.

Terminal
  t opens and closes the terminal. It always sits on the bottom of the screen.
  Drag its top edge to change the height.
  Sessions are a list on the right, including debug. x on a row closes it.
  + adds a shell. exit closes that session. Esc returns to the editor.
  Ctrl-C goes to the focused session and does not quit Posit Edit.

Closing
  w closes the focused buffer. The w on a buffer does the same.
  q quits. The q in the title bar does the same.
  A modified buffer asks first. s saves and closes it. Esc cancels.
  Quit asks y or n only when a buffer has unsaved edits. y saves them and quits.

Notifications
  Errors collect at the bottom right. i, or the badge, opens and closes them.
";

fn keybind_rows() -> Vec<(&'static str, &'static str)> {
    let mut rows = vec![
        (";", "open settings"),
        (":", "open the command line"),
        ("?", "open this guide"),
        ("[", "focus the first buffer"),
        ("]", "focus the last buffer"),
        ("{", "move this buffer to the start"),
        ("}", "move this buffer to the end"),
        ("b", "build"),
        ("a", "select all"),
        ("<", "toggle the file sidebar"),
        (">", "toggle the agent chat"),
        ("Ctrl-A", "select all"),
        ("Ctrl-Down", "scroll the file"),
        ("Ctrl-Left", "focus the previous buffer"),
        ("Ctrl-P", "open the command line"),
        ("Ctrl-Q", "quit"),
        ("Ctrl-Right", "focus the next buffer"),
        ("Ctrl-S", "save"),
        ("Ctrl-Up", "scroll the file"),
        ("Ctrl-W", "toggle fullscreen"),
        ("Ctrl-Z", "undo the last edit"),
        ("Down", "scroll the file"),
        ("Enter", "enter editing on the focused buffer"),
        ("Esc", "leave editing and return to view mode"),
        ("f", "toggle fullscreen"),
        ("ff", "format the file"),
        ("F1", "open this guide"),
        ("i", "toggle notifications"),
        ("l", "search open buffers"),
        ("Left", "focus the previous buffer"),
        ("ll", "search files"),
        ("lll", "live grep"),
        ("o", "open floating Yazi"),
        ("q", "quit when buffers are saved"),
        ("r", "run"),
        ("Right", "focus the next buffer"),
        ("rr", "rename the file"),
        ("s", "save"),
        ("t", "toggle the terminal"),
        ("Shift-Down", "view: scroll down. edit: select down"),
        (
            "Shift-Left",
            "view: swap with the previous buffer. edit: select left",
        ),
        (
            "Shift-Right",
            "view: swap with the next buffer. edit: select right",
        ),
        ("Shift-Up", "view: scroll up. edit: select up"),
        ("Up", "scroll the file"),
        ("w", "close the buffer"),
        ("x", "cut the selection"),
        ("y", "copy the selection"),
        ("p", "paste"),
        ("1", "preview a build plan"),
        ("2", "preview a run plan"),
        ("3", "show the workbench configuration"),
        ("4", "preview a test plan"),
        ("5", "refresh detected projects"),
        ("6", "preview the browser runtime"),
        ("7", "preview the phone runtime"),
        ("8", "preview the Android runtime"),
        ("9", "report debugger availability"),
    ];
    rows.sort_by_key(|left| bind_order(left.0));
    rows
}

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

fn help_text() -> String {
    let mut text = String::from(HELP_INTRO);
    text.push_str("\nKeybinds\n\n");
    text.push_str(&format!("{:<13}{}\n", "key", "action"));
    for (key, action) in keybind_rows() {
        text.push_str(&format!("{key:<13}{action}\n"));
    }
    text
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
    ScrollUp { column: u16, row: u16 },
    ScrollDown { column: u16, row: u16 },
    Right { column: u16, row: u16 },
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
    Esc,
    Help,
    Tab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectBy {
    Char,
    Word,
    Line,
}

/// ANSI 11 is base02 in a base16 terminal theme: the selection background,
/// a lighter step of the theme background rather than a fixed yellow.
const SELECTION_BG: Color = Color::LightYellow;

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
        Self {
            path: None,
            name: name.into(),
            document: Document::new(text),
            cursor,
            scroll: 0,
            scroll_pos: 0.0,
            scroll_vel: 0.0,
            anchor: cursor,
            selection: None,
            language: language.into(),
            saved: text.to_string(),
        }
    }

    fn text(&self) -> String {
        self.document.to_text()
    }

    fn dirty(&self) -> bool {
        self.text() != self.saved
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
        let revision = self.document.revision();
        let edit = Edit {
            start: 0,
            end: self.document.len_chars(),
            replacement: text.to_string(),
        };
        if self.document.apply(revision, &[edit]).is_ok() {
            self.cursor = self.document.len_chars();
            self.anchor = self.cursor;
            self.selection = None;
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

    fn move_vertical(&mut self, delta: isize, visible: usize, select: bool) {
        if !select {
            self.selection = None;
            self.anchor = self.cursor;
        }
        let text = self.document.to_text();
        let (line, column) = line_col(&text, self.cursor);
        let next = if delta < 0 {
            line.saturating_sub(delta.unsigned_abs())
        } else {
            line + delta as usize
        };
        self.cursor = char_at(&text, next, column);
        if select {
            self.sync_selection();
        } else {
            self.anchor = self.cursor;
        }
        self.reveal(visible);
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

    fn reveal(&mut self, visible: usize) {
        let (line, _) = line_col(&self.document.to_text(), self.cursor);
        if line < self.scroll {
            self.scroll = line;
        } else if visible > 0 && line >= self.scroll + visible {
            self.scroll = line + 1 - visible;
        }
        self.scroll_pos = self.scroll as f32;
        self.scroll_vel = 0.0;
    }

    fn move_to_line(&mut self, line: usize, visible: usize) {
        let text = self.document.to_text();
        self.cursor = char_at(&text, line.saturating_sub(1), 0);
        self.anchor = self.cursor;
        self.selection = None;
        self.reveal(visible);
    }
}

impl EditorApp {
    pub fn new(text: &str) -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut open_dirs = HashSet::new();
        open_dirs.insert(workspace.clone());
        let mut app = Self {
            buffers: vec![EditBuffer::from_text("untitled", text, "plain")],
            focus: 0,
            show_many: true,
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
            tree_focused: false,
            tree_cursor: 0,
            tree_scroll: 0,
            tree: Vec::new(),
            open_dirs,
            workspace,
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
            terminals: terminal::Panel::new(),
            notifications: Vec::new(),
            notifications_open: false,
            tile_spans: None,
            tile_drag: None,
            terminal_rows: None,
            terminal_drag: false,
            pixels: None,
        };
        app.reload_tree();
        app.note_frame(80, 24);
        app
    }

    pub fn language(&self) -> &str {
        &self.focused().language
    }

    pub fn set_language(&mut self, language: impl Into<String>) {
        self.focused_mut().language = language.into();
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

    pub fn help_open(&self) -> bool {
        self.help
    }

    pub fn cursor(&self) -> usize {
        self.focused().cursor
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.focused().selection
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

    pub fn animating(&self) -> bool {
        (self.target - self.view).abs() >= 0.05 || self.scroll_live()
    }

    pub fn note_frame(&mut self, width: u16, height: u16) {
        self.width = width.max(1);
        self.height = height.max(1);
        let orientation = layout::axis_of(self.width, self.height, self.pixels);
        if orientation != self.axis
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
    }

    pub fn tick(&mut self, dt: Duration) {
        let delta = self.target - self.view;
        if delta.abs() < 0.05 {
            self.view = self.target;
        } else {
            let step = 1.0 - (-dt.as_secs_f32() / 0.12).exp();
            self.view += delta * step;
        }
        self.tick_scroll(dt);
    }

    pub fn handle(&mut self, key: CommandKey) {
        self.tile_drag = None;
        self.stop_scroll_glide();
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
                if contains(chrome.terminal, column, row) && chrome.terminal.height > 1 {
                    self.terminals.wheel(
                        -1,
                        column.saturating_sub(chrome.terminal.x),
                        chrome.terminal.width,
                    );
                } else if self.yazi.is_some() {
                    self.move_yazi(-1);
                } else if self.help {
                    self.scroll_help(-1);
                } else {
                    self.wheel_buffer(-1.0);
                }
            }
            Pointer::ScrollDown { column, row } => {
                if contains(chrome.terminal, column, row) && chrome.terminal.height > 1 {
                    self.terminals.wheel(
                        1,
                        column.saturating_sub(chrome.terminal.x),
                        chrome.terminal.width,
                    );
                } else if self.yazi.is_some() {
                    self.move_yazi(1);
                } else if self.help {
                    self.scroll_help(1);
                } else {
                    self.wheel_buffer(1.0);
                }
            }
            Pointer::Right { column, row } => {
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
                if drag && self.terminal_drag {
                    self.resize_terminal(row);
                    return;
                }
                if !drag && self.click_terminal(column, row) {
                    self.agent_focused = false;
                    return;
                }
                self.terminals.blur();
                if !drag {
                    self.resize = None;
                    self.tile_drag = None;
                    self.terminal_drag = false;
                }
                if self.on_terminal_seam(column, row) {
                    self.terminal_drag = true;
                    if drag {
                        self.resize_terminal(row);
                    }
                    return;
                }
                if quit_hit(column, row, area) {
                    if !drag {
                        self.request_quit();
                    }
                    return;
                }
                if help_hit(column, row, area, chrome.status) {
                    if !drag {
                        self.help = !self.help;
                        if self.help {
                            self.help_scroll = 0;
                        }
                    }
                    return;
                }
                if self.help {
                    return;
                }
                if matches!(self.overlay, Overlay::Settings) {
                    if !drag {
                        let inner = inset(chrome.body);
                        if row == inner.y.saturating_add(2) {
                            self.sidebar = !self.sidebar;
                        } else if row == inner.y.saturating_add(3) {
                            self.toggle_chat();
                        } else if row == inner.y.saturating_add(4) {
                            self.show_many = !self.show_many;
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
                        self.agent_focused = true;
                        self.tree_focused = false;
                    }
                    return;
                }
                if chrome.tree.width > 0 && contains(chrome.tree, column, row) {
                    if !drag && self.sidebar_button(chrome.tree, column, row) {
                        self.sidebar = !self.sidebar;
                    } else if !drag {
                        self.tree_click(row, chrome);
                    }
                    return;
                }
                if self.show_many && self.buffers.len() >= 2 {
                    if drag {
                        if let Some(index) = self.tile_drag {
                            self.resize_tile(index, column, row);
                            return;
                        }
                    } else if let Some(index) = self.tile_seam(column, row) {
                        self.tile_drag = Some(index);
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
                    self.agent_focused = false;
                    self.focus_at(index);
                }
                let screen_column = column;
                let Some((column, line_in_view)) =
                    layout::cell_in_tile(window, index, self.view, screen_column, row)
                else {
                    return;
                };
                let line = self.focused().scroll + line_in_view;
                let visible = self.visible_rows();
                self.text_pointer(line, column, drag, screen_column, row);
                self.focused_mut().reveal(visible);
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
        self.focused_mut().select_all();
        let visible = self.visible_rows();
        self.focused_mut().reveal(visible);
    }

    fn copy_selection(&mut self) {
        let Some(text) = self.focused().selected_text() else {
            self.notice = "nothing selected".to_string();
            return;
        };
        match clipboard::set(&text) {
            Ok(()) => self.notice = "copied".to_string(),
            Err(detail) => self.report_error("Clipboard", &detail),
        }
    }

    fn cut_selection(&mut self) {
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
                self.focused_mut().insert_text(text);
                let visible = self.visible_rows();
                self.focused_mut().reveal(visible);
                self.notice = "pasted".to_string();
            }
            Err(detail) => self.report_error("Clipboard", &detail),
        }
    }

    fn menu_action(&mut self, index: usize) {
        match index {
            0 => self.cut_selection(),
            1 => self.copy_selection(),
            2 => self.paste_clipboard(),
            3 => self.select_all(),
            _ => {}
        }
    }

    fn open_menu(&mut self, column: u16, row: u16) {
        let window = self.window();
        if let Some(index) = layout::hit_tile(window, column, row, self.view, self.buffers.len()) {
            if index != self.focus {
                self.focus_at(index);
            }
            if let Some((local_column, line_in_view)) =
                layout::cell_in_tile(window, index, self.view, column, row)
            {
                let line = self.focused().scroll + line_in_view;
                let pos = char_at(&self.focused().text(), line, local_column);
                let inside = self
                    .focused()
                    .selection
                    .is_some_and(|(start, end)| pos >= start && pos < end);
                if !inside {
                    self.focused_mut().place(line, local_column, false);
                }
            }
        }
        let cursor = if self.focused().selection.is_some() {
            1
        } else {
            2
        };
        self.overlay = Overlay::Menu(MenuState {
            x: column,
            y: row,
            cursor,
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
            self.overlay = Overlay::None;
            self.menu_action(index);
        }
    }

    fn seam(&self, chrome: &Chrome, column: u16, row: u16) -> Option<Side> {
        if edge_hit(chrome.tree, column, row, true) {
            Some(Side::Left)
        } else if edge_hit(chrome.agent, column, row, false) {
            Some(Side::Right)
        } else {
            None
        }
    }

    fn tile_seam(&self, column: u16, row: u16) -> Option<usize> {
        if !self.show_many || self.buffers.len() < 2 {
            return None;
        }
        let window = self.window();
        for index in 0..self.buffers.len().saturating_sub(1) {
            let Some(slice) = layout::tile_slice(window, index, self.view) else {
                continue;
            };
            let (tile_w, tile_h) = window.tile_box(index);
            let on_title = slice.source_y == 0 && row == slice.screen_y;
            let hit = match window.axis {
                Axis::Horizontal => {
                    slice.source_x + slice.width == tile_w
                        && column + 1 == slice.screen_x + slice.width
                        && row >= slice.screen_y
                        && row < slice.screen_y.saturating_add(slice.height)
                        && !on_title
                }
                Axis::Vertical => {
                    slice.source_y + slice.height == tile_h
                        && row + 1 == slice.screen_y + slice.height
                        && column >= slice.screen_x
                        && column < slice.screen_x.saturating_add(slice.width)
                }
            };
            if hit {
                return Some(index);
            }
        }
        None
    }

    fn resize_tile(&mut self, index: usize, column: u16, row: u16) {
        if !self.show_many || index + 1 >= self.buffers.len() {
            return;
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
        let total = spans[index].saturating_add(spans[index + 1]);
        let min = match window.axis {
            Axis::Horizontal => 12,
            Axis::Vertical => 6,
        };
        if total < min * 2 {
            return;
        }
        let (cursor, start) = match window.axis {
            Axis::Horizontal => (column as i32, window.viewport.x as i32),
            Axis::Vertical => (row as i32, window.viewport.y as i32),
        };
        let wanted = (cursor - start - window.origin(index) + self.view.round() as i32 + 1)
            .clamp(min as i32, (total - min) as i32) as u16;
        spans[index] = wanted;
        spans[index + 1] = total - wanted;
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

    fn click_notifications(&mut self, column: u16, row: u16) -> bool {
        let area = Rect::new(0, 0, self.width, self.height);
        if self.notifications_open && !self.notifications.is_empty() {
            let rect = notification_rect(area, self.notifications.len());
            if contains(rect, column, row) {
                let local_y = row.saturating_sub(rect.y);
                if local_y <= 1 {
                    self.notifications_open = false;
                    return true;
                }
                let index = (local_y as usize - 2) / 2;
                let local_x = column.saturating_sub(rect.x);
                if index < self.notifications.len() && local_x + 3 >= rect.width {
                    self.notifications.remove(index);
                    if self.notifications.is_empty() {
                        self.notifications_open = false;
                    }
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

    fn maintain_terminal(&mut self) {
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
    }

    fn on_terminal_seam(&self, column: u16, row: u16) -> bool {
        let rect = self.terminal_rect();
        rect.height > 0
            && row + 1 == rect.y
            && column >= rect.x
            && column < rect.x.saturating_add(rect.width)
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
            return;
        }
        self.terminals.open = true;
        self.terminal_rows = Some(height);
    }

    pub fn terminal_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if !self.terminals.focused() {
            return false;
        }
        if code == KeyCode::Esc && modifiers.is_empty() {
            self.terminals.blur();
            return true;
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

    fn click_terminal(&mut self, column: u16, row: u16) -> bool {
        let rect = self.terminal_rect();
        if rect.height == 0 || !contains(rect, column, row) {
            return false;
        }
        let local_x = column.saturating_sub(rect.x);
        let local_y = row.saturating_sub(rect.y);
        match self
            .terminals
            .click(local_x, local_y, rect.width, rect.height)
        {
            terminal::Click::Add => {
                let (cols, rows) = self.terminals.grid_size(rect.width, rect.height);
                if let Err(detail) = self.terminals.add(&self.workspace, cols, rows) {
                    self.notice = format!("terminal: {detail}");
                }
            }
            terminal::Click::Toggle => {
                self.notice = if self.terminals.open {
                    "terminal on".to_string()
                } else {
                    "terminal off".to_string()
                };
            }
            terminal::Click::Close | terminal::Click::Focus => {}
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
        if self.overlay != Overlay::None && !self.help && !self.palette {
            self.handle_overlay(key);
            return;
        }
        if self.help {
            match key {
                CommandKey::Help | CommandKey::Esc => self.help = false,
                CommandKey::CtrlQ => self.request_quit(),
                CommandKey::CtrlP => {
                    self.help = false;
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
                    self.tree_focused = false;
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
            _ if self.editing => self.edit_key(key),
            _ => self.view_key(key),
        }
    }

    fn edit_key(&mut self, key: CommandKey) {
        match key {
            CommandKey::Esc => self.editing = false,
            CommandKey::Left => self.edit_arrow(-1, 0, false),
            CommandKey::Right => self.edit_arrow(1, 0, false),
            CommandKey::Up => self.edit_arrow(0, -1, false),
            CommandKey::Down => self.edit_arrow(0, 1, false),
            CommandKey::ShiftLeft => self.edit_arrow(-1, 0, true),
            CommandKey::ShiftRight => self.edit_arrow(1, 0, true),
            CommandKey::ShiftUp => self.edit_arrow(0, -1, true),
            CommandKey::ShiftDown => self.edit_arrow(0, 1, true),
            CommandKey::Backspace => self.focused_mut().backspace(),
            CommandKey::Delete => self.focused_mut().delete_forward(),
            CommandKey::Enter => self.focused_mut().insert('\n'),
            CommandKey::Char(ch) => self.focused_mut().insert(ch),
            CommandKey::Tab => self.focused_mut().insert('\t'),
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
            CommandKey::Enter => self.begin_edit(),
            _ => {}
        }
    }

    fn edit_arrow(&mut self, column: isize, line: isize, select: bool) {
        let visible = self.visible_rows();
        if column != 0 {
            self.focused_mut().move_horizontal(column, select);
            self.focused_mut().reveal(visible);
        }
        if line != 0 {
            self.focused_mut().move_vertical(line, visible, select);
        }
    }

    fn begin_edit(&mut self) {
        self.editing = true;
        self.tree_focused = false;
        self.agent_focused = false;
        self.clear_chord();
    }

    fn handle_agent(&mut self, key: CommandKey) {
        match key {
            CommandKey::CtrlQ => self.request_quit(),
            CommandKey::Esc => self.agent_focused = false,
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
        if self.agent_program.is_empty() {
            self.agent_log.push("no agent CLI on PATH".to_string());
            return;
        }
        if cfg!(test) {
            self.agent_log.push(format!("{} ready", self.agent_program));
            return;
        }
        let args = agent_args(&self.agent_program, &prompt);
        match capture(&self.agent_program, &args, &self.workspace) {
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
                    self.agent_log
                        .push(format!("{} finished", self.agent_program));
                } else {
                    self.agent_log.extend(lines);
                }
            }
            Err(detail) => self.agent_log.push(detail),
        }
        if self.agent_log.len() > 40 {
            let extra = self.agent_log.len() - 40;
            self.agent_log.drain(0..extra);
        }
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
            "b" | "r" | "f" | "l" | "ll" => {
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
            'q' => self.request_quit(),
            's' => self.save(),
            'w' => self.request_close(self.focus),
            'o' => self.open_yazi(),
            'a' => self.select_all(),
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
            ';' => self.overlay = Overlay::Settings,
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
            'b' | 'r' | 'f' | 'l' => {
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
        let delta = match key {
            CommandKey::Up | CommandKey::ShiftUp => -1,
            _ => 1,
        };
        let scroll = if delta < 0 {
            self.focused().scroll.saturating_sub(1)
        } else {
            self.focused().scroll + 1
        };
        self.snap_scroll(scroll);
    }

    fn handle_overlay(&mut self, key: CommandKey) {
        match &self.overlay.clone() {
            Overlay::Quit => match key {
                CommandKey::Char('y') => self.save_all_and_quit(),
                CommandKey::Char('n') | CommandKey::Esc => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Close(index) => {
                let index = *index;
                match key {
                    CommandKey::Char('s') => self.save_and_close(index),
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
                    self.overlay = Overlay::None;
                    self.menu_action(cursor);
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
                    self.overlay = Overlay::None;
                    self.select_all();
                }
                _ => {}
            },
            Overlay::None => {}
        }
    }

    fn open_help(&mut self) {
        self.help = true;
        self.help_scroll = 0;
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
            _ => {}
        }
    }

    fn handle_palette(&mut self, key: CommandKey) {
        match key {
            CommandKey::CtrlQ | CommandKey::CtrlP | CommandKey::Esc => self.palette = false,
            CommandKey::Backspace => {
                self.query.pop();
            }
            CommandKey::Char(ch) => self.query.push(ch),
            CommandKey::Enter => {
                let command = self.query.trim().to_string();
                self.query.clear();
                self.palette = false;
                if let Some(setting) = command.strip_prefix("dlc ") {
                    self.dlc_setting(setting.trim());
                    return;
                }
                match command.as_str() {
                    "quit" => self.request_quit(),
                    "split" => self.show_many = !self.show_many,
                    "undo" => self.focused_mut().undo(),
                    "format" => self.format_now(),
                    "help" => {
                        self.help = true;
                        self.help_scroll = 0;
                    }
                    "files" | "open" | "yazi" => self.open_yazi(),
                    "find" => self.open_finder(FinderKind::Files),
                    "grep" => self.open_finder(FinderKind::Grep),
                    "new" => self.new_buffer(),
                    "save" => self.save(),
                    "wq" | ":wq" => {
                        self.overlay = Overlay::Vim;
                    }
                    _ => self.notice = format!("unknown command: {command}"),
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
        self.quit = true;
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
                self.notice = "workspace trusted for reviewed plans".to_string();
            }
            "untrust" => {
                self.dlc_trusted = false;
                self.pending_plan = None;
                self.notice = "workspace untrusted; plans stay previews".to_string();
            }
            "" => self.notice = "dlc needs trust, untrust, or a system name".to_string(),
            system => {
                self.dlc_system = Some(system.to_string());
                self.notice = format!("workbench system {system}");
            }
        }
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
                self.start_debug();
            }
            WorkbenchAction::Refresh => {
                self.pending_plan = None;
                let text = self.dlc_text(&script, &["scan".into()]);
                self.show_output("*workbench*", &text);
                self.notice = "projects refreshed".to_string();
            }
            WorkbenchAction::Browser => self.launch_runtime(&script, "browser"),
            WorkbenchAction::Phone => self.launch_runtime(&script, "phone-view"),
            WorkbenchAction::Android => self.launch_runtime(&script, "android-boot"),
            WorkbenchAction::Configure => {
                self.pending_plan = None;
                let text = self.dlc_text(&script, &["config".into()]);
                self.show_output("*workbench*", &text);
                self.notice = "configuration preview".to_string();
            }
            WorkbenchAction::Build => self.plan_action(&script, "build"),
            WorkbenchAction::Run => self.plan_action(&script, "run"),
            WorkbenchAction::Test => self.plan_action(&script, "test"),
        }
    }

    fn start_debug(&mut self) {
        let language = self.language().to_string();
        let command = if self.workspace.join("Cargo.toml").is_file() {
            debug_cargo(&self.workspace)
        } else if let Some(path) = self.focused().path.clone() {
            debug_file(&language, &path)
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
        let text = self.dlc_text(script, &["runtime".into(), action.into()]);
        if let Some(error) = json_error(&text) {
            self.report_error("Runtime", &error);
            return;
        }
        let Some((cwd, argv)) = plan_argv(&text, &self.workspace) else {
            self.report_error("Runtime", "runtime plan has no command");
            return;
        };
        let program = argv[0].clone();
        let args = argv[1..].to_vec();
        let name = Path::new(&program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&program);
        if name == "simctl" {
            if let Err(detail) = spawn_detached(&program, &args, &cwd) {
                self.report_error("Runtime", &detail);
                return;
            }
            match capture("open", &["-a".into(), "Simulator".into()], &cwd) {
                Ok((true, _)) => self.notice = "iPhone simulator".to_string(),
                Ok((false, detail)) => self.report_error("Runtime", &detail),
                Err(detail) => self.report_error("Runtime", &detail),
            }
            return;
        }
        if name == "open" || name == "xdg-open" {
            match capture(&program, &args, &cwd) {
                Ok((true, _)) => {
                    let target = args.last().cloned().unwrap_or_default();
                    self.notice = format!("opened {target}");
                }
                Ok((false, detail)) => self.report_error("Runtime", &detail),
                Err(detail) => self.report_error("Runtime", &detail),
            }
            return;
        }
        self.terminals.open = true;
        let rect = self.terminal_rect();
        let (cols, rows) = self
            .terminals
            .grid_size(rect.width.max(20), rect.height.max(8));
        match self
            .terminals
            .run_program(&cwd, cols, rows, &program, &args)
        {
            Ok(()) => self.notice = format!("{action} in the terminal"),
            Err(detail) => self.report_error("Runtime", &detail),
        }
    }

    fn plan_action(&mut self, script: &Path, action: &str) {
        let scan = self.dlc_text(script, &["scan".into()]);
        let systems = scan_systems(&scan);
        let Some(system) = pick_system(&systems, self.language(), self.dlc_system.as_deref())
        else {
            self.pending_plan = None;
            self.show_output("*workbench*", &scan);
            self.notice = "choose a system with : dlc cargo".to_string();
            return;
        };
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
        self.notice = match authorize(trust) {
            TrustAction::Execute if self.pending_plan.is_some() => {
                format!("{system} {action} plan ready; y executes")
            }
            TrustAction::Preview if self.pending_plan.is_some() => {
                format!("{system} {action} preview; : dlc trust before y")
            }
            _ => format!("{system} {action} has no executable plan"),
        };
        self.show_output("*workbench*", &text);
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
        if let Some(index) = self.buffers.iter().position(|buffer| buffer.name == title) {
            self.buffers[index].replace_all(text);
            self.buffers[index].mark_clean();
            self.focus_at(index);
            return;
        }
        let mut buffer = EditBuffer::from_text(title, text, "plain");
        buffer.cursor = 0;
        buffer.anchor = 0;
        self.buffers.push(buffer);
        self.focus_at(self.buffers.len() - 1);
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
            let leaving = self.focused_mut();
            leaving.scroll_pos = leaving.scroll as f32;
            leaving.scroll_vel = 0.0;
            self.wheel_at = None;
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
        layout::chrome(
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
        )
    }

    fn metrics_for(&self, width: u16, height: u16, orientation: Axis) -> (Chrome, Window) {
        let chrome = layout::chrome(
            Rect::new(0, 0, width, height),
            self.blocks_status(),
            self.sidebar,
            self.chat,
            self.tree_width,
            self.agent_width,
            layout::TerminalPane {
                open: self.terminals.open,
                rows: self.terminal_rows,
            },
        );
        let window = self.layout_window(chrome.content, orientation);
        (chrome, window)
    }

    fn layout_window(&self, content: Rect, orientation: Axis) -> Window {
        let custom = (self.show_many && orientation == self.axis)
            .then_some(self.tile_spans.as_deref())
            .flatten();
        match custom {
            Some(spans) => layout::window_spans(
                content,
                orientation,
                self.show_many,
                self.buffers.len(),
                Some(spans),
            ),
            None => layout::window(content, orientation, self.show_many, self.buffers.len()),
        }
    }

    fn window(&self) -> Window {
        self.metrics_for(self.width, self.height, self.axis).1
    }

    fn visible_rows(&self) -> usize {
        self.window().tile_box(self.focus).1.saturating_sub(2) as usize
    }

    fn scroll_limit(&self) -> f32 {
        let visible = self.visible_rows().max(1);
        let lines = self.focused().text().split('\n').count();
        lines.saturating_sub(visible) as f32
    }

    fn scroll_live(&self) -> bool {
        let buffer = self.focused();
        buffer.scroll_vel.abs() > 0.35 || (buffer.scroll_pos - buffer.scroll as f32).abs() > 0.04
    }

    fn wheel_buffer(&mut self, delta: f32) {
        let now = Instant::now();
        let gap = self
            .wheel_at
            .map(|at| now.saturating_duration_since(at).as_secs_f32())
            .unwrap_or(1.0);
        self.wheel_at = Some(now);
        let max = self.scroll_limit();
        let dim = self.visible_rows().max(1) as f32;
        let buffer = self.focused_mut();
        buffer.scroll_pos = track_scroll(buffer.scroll_pos, delta, max, dim);
        let sample = if gap > 0.08 {
            0.0
        } else {
            delta / gap.clamp(0.008, 0.05)
        };
        buffer.scroll_vel = if gap > 0.08 {
            sample
        } else {
            buffer.scroll_vel * 0.4 + sample * 0.6
        };
        buffer.scroll_vel = buffer.scroll_vel.clamp(-90.0, 90.0);
        buffer.scroll = buffer.scroll_pos.clamp(0.0, max).round().max(0.0) as usize;
    }

    fn tick_scroll(&mut self, dt: Duration) {
        let idle = {
            let buffer = self.focused();
            buffer.scroll_vel == 0.0 && (buffer.scroll_pos - buffer.scroll as f32).abs() < 0.001
        };
        if idle {
            self.wheel_at = None;
            return;
        }
        let tracking = self
            .wheel_at
            .is_some_and(|at| at.elapsed() < Duration::from_millis(48));
        let max = self.scroll_limit();
        let buffer = self.focused_mut();
        let (pos, vel) = step_scroll(
            buffer.scroll_pos,
            buffer.scroll_vel,
            dt.as_secs_f32(),
            max,
            tracking,
        );
        buffer.scroll_pos = pos;
        buffer.scroll_vel = vel;
        buffer.scroll = pos.clamp(0.0, max).round().max(0.0) as usize;
        if vel == 0.0 && (0.0..=max).contains(&pos) {
            self.wheel_at = None;
        }
    }

    fn stop_scroll_glide(&mut self) {
        let max = self.scroll_limit();
        let buffer = self.focused_mut();
        if buffer.scroll_vel == 0.0 && (0.0..=max).contains(&buffer.scroll_pos) {
            return;
        }
        let pos = buffer.scroll_pos.clamp(0.0, max).round().clamp(0.0, max);
        buffer.scroll_pos = pos;
        buffer.scroll_vel = 0.0;
        buffer.scroll = pos as usize;
        self.wheel_at = None;
    }

    fn snap_scroll(&mut self, line: usize) {
        let max = self.scroll_limit() as usize;
        let line = line.min(max);
        let buffer = self.focused_mut();
        buffer.scroll = line;
        buffer.scroll_pos = line as f32;
        buffer.scroll_vel = 0.0;
        self.wheel_at = None;
    }

    fn scroll_page(&mut self, delta: isize) {
        let page = self.visible_rows().max(1) as isize;
        let current = self.focused().scroll as isize;
        let next = (current + delta * page).max(0) as usize;
        self.snap_scroll(next);
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
        let body = layout::chrome(
            Rect::new(0, 0, self.width, self.height),
            false,
            self.sidebar,
            self.chat,
            self.tree_width,
            self.agent_width,
            layout::TerminalPane {
                open: self.terminals.open,
                rows: self.terminal_rows,
            },
        )
        .editor;
        let visible = inset(body).height as usize;
        let max_scroll = help_text().lines().count().saturating_sub(visible.max(1));
        self.help_scroll = self.help_scroll.min(max_scroll);
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
        if let Some(parent) = path.parent() {
            if parent.is_dir() {
                self.workspace = parent.to_path_buf();
                self.file_index.clear();
                self.open_dirs.insert(self.workspace.clone());
                self.reload_tree();
            }
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
            self.reload_tree();
            self.notice = "workspace changed".to_string();
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
        self.file_index.clear();
        self.open_dirs.insert(self.workspace.clone());
        self.tree_cursor = 0;
        self.tree_scroll = 0;
        self.reload_tree();
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
            self.tree_focused = true;
        } else {
            self.open_path(entry.path);
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
        self.tree_focused = true;
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
            self.focused_mut().move_to_line(line, visible);
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
    let chrome = app.frame_chrome(area);
    let gaze = if app.editing { "EDIT" } else { "VIEW" };
    let header = Paragraph::new(format!(
        "Posit {}  {}  {gaze}",
        app.language(),
        app.focused().name
    ))
    .block(header_block())
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
    toolbar.render(frame, chrome.toolbar);
    if app.help {
        let body = Paragraph::new(help_text())
            .scroll((app.help_scroll.min(u16::MAX as usize) as u16, 0))
            .block(Block::default().title("Help").borders(Borders::ALL));
        frame.render_widget(body, chrome.editor);
    } else if app.finder.is_some() {
        frame.render_widget(finder_paragraph(app), chrome.editor);
    } else if matches!(app.overlay, Overlay::Settings) {
        frame.render_widget(settings_paragraph(app), chrome.editor);
    } else if matches!(app.overlay, Overlay::Vim) {
        frame.render_widget(vim_paragraph(), chrome.editor);
    } else {
        if chrome.tree.width > 0 {
            if app.sidebar && chrome.tree.width > 4 {
                frame.render_widget(tree_paragraph(app), chrome.tree);
            } else {
                frame.render_widget(sidebar_rail(), chrome.tree);
            }
        }
        if chrome.agent.width > 4 {
            frame.render_widget(agent_paragraph(app, chrome.agent), chrome.agent);
        } else if chrome.agent.width > 0 {
            frame.render_widget(agent_rail(), chrome.agent);
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
    }
    if chrome.terminal.height > 0 {
        frame.render_widget(Clear, chrome.terminal);
        frame.render_widget(
            app.terminals
                .paragraph(chrome.terminal.width, chrome.terminal.height),
            chrome.terminal,
        );
    }
    if let Overlay::Menu(menu) = app.overlay {
        let rect = menu_rect(menu.x, menu.y, area);
        frame.render_widget(Clear, rect);
        frame.render_widget(menu_paragraph(menu.cursor), rect);
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
    let status = format!(
        "? help   {gaze}  {axis}  {mode}  {}  rev {}  {}  chars {}{selected}{pending}{notice}",
        app.focused().name,
        app.focused().document.revision(),
        app.language(),
        app.focused().document.len_chars()
    );
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
        frame.render_widget(Paragraph::new(status), status_rect);
    }
    if app.yazi.is_some() {
        let float = yazi_rect(area);
        frame.render_widget(Clear, float);
        frame.render_widget(yazi_paragraph(app, float.width), float);
    }
    paint_notifications(frame, app, area);
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
        return Paragraph::new(Text::from(lines)).block(files_block());
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
        }
        lines.push(Line::styled(
            tree_label(&entry.name, entry.depth, marker, inner.max(1)),
            style,
        ));
    }
    if lines.is_empty() {
        lines.push(Line::from("(empty)"));
    }
    Paragraph::new(Text::from(lines))
        .scroll((app.tree_scroll.min(u16::MAX as usize) as u16, 0))
        .block(files_block())
}

fn files_block() -> Block<'static> {
    Block::default()
        .title("Files")
        .title(
            Line::from(TuiSpan::styled(" < ", Style::default().fg(Color::Yellow)))
                .alignment(Alignment::Right),
        )
        .borders(Borders::ALL)
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
    let border = if app.agent_focused {
        focus_color(false)
    } else {
        Color::Gray
    };
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title("Agent")
            .title(
                Line::from(TuiSpan::styled(" > ", Style::default().fg(Color::Yellow)))
                    .alignment(Alignment::Right),
            )
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

fn edge_hit(pane: Rect, column: u16, row: u16, left: bool) -> bool {
    if pane.width == 0 || row < pane.y || row >= pane.y.saturating_add(pane.height) {
        return false;
    }
    let right = pane.x.saturating_add(pane.width);
    if pane.width <= 4 {
        return column >= pane.x && column < right;
    }
    if left {
        column + 1 == right || column == right
    } else {
        column == pane.x || column + 1 == pane.x
    }
}

fn agent_rail() -> Paragraph<'static> {
    Paragraph::new(">")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        )
}

fn sidebar_rail() -> Paragraph<'static> {
    Paragraph::new("<")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        )
}

fn settings_paragraph(app: &EditorApp) -> Paragraph<'static> {
    let sidebar = if app.sidebar { "on" } else { "off" };
    let chat = if app.chat { "on" } else { "off" };
    let columns = if app.show_many { "auto" } else { "full" };
    Paragraph::new(Text::from(vec![
        Line::from("Settings"),
        Line::from(""),
        Line::from(format!("sidebar   {sidebar}    < or drag")),
        Line::from(format!(
            "terminal  {}    t",
            if app.terminals.open { "on" } else { "off" }
        )),
        Line::from(format!("chat      {chat}       > or drag")),
        Line::from(format!("columns   {columns}    f or click")),
        Line::from(""),
        Line::from("Esc or ; closes"),
    ]))
    .block(Block::default().title("Settings").borders(Borders::ALL))
}

fn prompt_line(app: &EditorApp) -> Option<String> {
    match &app.overlay {
        Overlay::Quit => Some("unsaved changes. y saves and quits, n or Esc stays".to_string()),
        Overlay::Close(_) => Some("unsaved changes. s saves and closes, Esc cancels".to_string()),
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

fn track_scroll(pos: f32, delta: f32, max: f32, dim: f32) -> f32 {
    let max = max.max(0.0);
    let mut pos = pos;
    let mut remaining = delta;
    if remaining > 0.0 && pos < max {
        let step = remaining.min(max - pos);
        pos += step;
        remaining -= step;
    } else if remaining < 0.0 && pos > 0.0 {
        let step = remaining.max(-pos);
        pos += step;
        remaining -= step;
    }
    if remaining.abs() > f32::EPSILON {
        let bound = if remaining > 0.0 { max } else { 0.0 };
        let shown = (pos - bound).abs();
        let pulled = rubber(rubber_inverse(shown, dim) + remaining.abs(), dim);
        pos = if remaining > 0.0 {
            bound + pulled
        } else {
            bound - pulled
        };
    }
    pos
}

fn rubber(over: f32, dim: f32) -> f32 {
    let dim = dim.max(1.0);
    (1.0 - 1.0 / (over.max(0.0) * 0.55 / dim + 1.0)) * dim
}

fn rubber_inverse(shown: f32, dim: f32) -> f32 {
    let dim = dim.max(1.0);
    let shown = shown.clamp(0.0, dim * 0.92);
    if shown <= 0.0 {
        return 0.0;
    }
    dim / 0.55 * (1.0 / (1.0 - shown / dim) - 1.0)
}

fn step_scroll(mut pos: f32, mut vel: f32, dt: f32, max: f32, tracking: bool) -> (f32, f32) {
    if tracking {
        return (pos, vel);
    }
    let max = max.max(0.0);
    let mut left = dt.clamp(0.0, 0.05);
    while left > 0.0 {
        let step = left.min(0.008);
        if pos < 0.0 || pos > max {
            let target = pos.clamp(0.0, max);
            let offset = pos - target;
            let accel = -220.0 * offset - 22.0 * vel;
            vel += accel * step;
            pos += vel * step;
            let offset = if pos < 0.0 {
                pos
            } else if pos > max {
                pos - max
            } else {
                0.0
            };
            if offset.abs() < 0.05 && vel.abs() < 0.4 {
                return (pos.clamp(0.0, max), 0.0);
            }
        } else if vel.abs() > 0.35 {
            pos += vel * step;
            vel *= (-2.4 * step).exp();
        } else {
            let snapped = pos.round().clamp(0.0, max);
            if (pos - snapped).abs() < 0.04 {
                return (snapped, 0.0);
            }
            pos += (snapped - pos) * (1.0 - (-step / 0.05).exp());
            vel = 0.0;
        }
        left -= step;
    }
    (pos, vel)
}

fn elastic_frame(mut text: Text<'static>, pos: f32, max: f32) -> (Text<'static>, u16) {
    let max = max.max(0.0);
    if pos < -0.5 {
        let rows = ((-pos).round() as usize).clamp(1, 8);
        let mut lines = Vec::with_capacity(text.lines.len() + rows);
        lines.extend((0..rows).map(|_| Line::raw("")));
        lines.append(&mut text.lines);
        return (Text::from(lines), 0);
    }
    if pos > max + 0.5 {
        let rows = ((pos - max).round() as usize).clamp(1, 8);
        text.lines.extend((0..rows).map(|_| Line::raw("")));
        let scroll = (max.round().max(0.0) as usize + rows).min(u16::MAX as usize) as u16;
        return (text, scroll);
    }
    let scroll = pos.clamp(0.0, max).round().max(0.0).min(u16::MAX as f32) as u16;
    (text, scroll)
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
    let border = if index == app.focus {
        focus_color(app.editing)
    } else {
        Color::Gray
    };
    let text = highlighted(
        buffer,
        index == app.focus,
        app.editing,
        tile_w.saturating_sub(2) as usize,
    );
    let visible = tile_h.saturating_sub(2) as usize;
    let max = text.lines.len().saturating_sub(visible.max(1)) as f32;
    let (text, scroll) = elastic_frame(text, buffer.scroll_pos, max);
    let paragraph = Paragraph::new(text).scroll((scroll, 0)).block(
        Block::default()
            .title(buffer.name.clone())
            .title(
                Line::from(TuiSpan::styled(
                    " w ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(Alignment::Right),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border)),
    );
    paragraph.render(area, &mut screen);
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
                Style::default().add_modifier(Modifier::REVERSED)
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

fn header_block() -> Block<'static> {
    Block::default()
        .title("Posit")
        .title(
            Line::from(TuiSpan::styled(
                " ? q ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Right),
        )
        .borders(Borders::ALL)
}

fn help_hit(column: u16, row: u16, area: Rect, status: Option<Rect>) -> bool {
    let title = row == area.y && column == area.x + area.width.saturating_sub(5);
    let label = status.is_some_and(|status| {
        row == status.y && column >= status.x && column < status.x.saturating_add(8)
    });
    title || label
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

fn highlighted(
    buffer: &EditBuffer,
    show_cursor: bool,
    editing: bool,
    content_width: usize,
) -> Text<'static> {
    let text = buffer.text();
    let raw = highlight(&buffer.language, &text);
    let mut pieces = Vec::new();
    let mut cursor = 0;
    for span in raw {
        if span.start > cursor {
            pieces.push((cursor, span.start, TokenKind::Plain));
        }
        let start = span.start.max(cursor);
        if span.end > start {
            pieces.push((start, span.end.min(text.len()), span.kind));
            cursor = span.end.min(text.len());
        }
    }
    if cursor < text.len() {
        pieces.push((cursor, text.len(), TokenKind::Plain));
    }
    let selection = buffer
        .selection
        .map(|(start, end)| (char_to_byte(&text, start), char_to_byte(&text, end)));
    let cursor_byte = char_to_byte(&text, buffer.cursor);
    let mut lines = vec![Line::default()];
    for (start, end, kind) in pieces {
        let style = token_style(kind);
        let mut piece_start = start;
        for (index, piece) in text[start..end].split('\n').enumerate() {
            if index > 0 {
                lines.push(Line::default());
                piece_start += 1;
            }
            emit_piece(
                lines.last_mut().expect("line"),
                piece,
                piece_start,
                style,
                selection,
                show_cursor.then_some(cursor_byte),
                editing,
            );
            piece_start += piece.len();
        }
    }
    if show_cursor {
        paint_line_caret(&mut lines, &text, buffer.cursor, editing);
    }
    pad_selected_rows(&mut lines, buffer, &text, show_cursor, content_width);
    Text::from(lines)
}

fn paint_line_caret(lines: &mut [Line<'static>], text: &str, cursor: usize, editing: bool) {
    let mut offset = 0;
    for (index, row) in text.split('\n').enumerate() {
        let end = offset + row.chars().count();
        if cursor < end {
            return;
        }
        if cursor == end {
            if let Some(line) = lines.get_mut(index) {
                line.spans.push(TuiSpan::styled(" ", cursor_style(editing)));
            }
            return;
        }
        offset = end + 1;
    }
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

fn emit_piece(
    line: &mut Line<'static>,
    piece: &str,
    piece_start: usize,
    style: Style,
    selection: Option<(usize, usize)>,
    cursor_byte: Option<usize>,
    editing: bool,
) {
    if piece.is_empty() {
        return;
    }
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
        line.spans
            .push(TuiSpan::styled(piece[start..end].to_string(), part_style));
    }
}

fn selection_style(base: Style) -> Style {
    let mut style = base;
    style.bg = Some(SELECTION_BG);
    if let Some(color) = style.fg {
        style.fg = Some(foreground_on_selection(color));
    }
    style
}

/// base00–base04 sit on the background ramp, so they vanish on base02.
/// base05 is the theme's normal text and stays readable there.
fn foreground_on_selection(color: Color) -> Color {
    match color {
        Color::Black
        | Color::DarkGray
        | Color::LightGreen
        | Color::LightYellow
        | Color::LightBlue => Color::Gray,
        other => other,
    }
}

fn focus_color(editing: bool) -> Color {
    if editing {
        Color::Rgb(230, 120, 20)
    } else {
        Color::Magenta
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

fn release_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
}

fn restore_terminal() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    Ok(())
}

pub fn detect_path(path: &str) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let profiles = ide_syntax::load_profiles(&ide_syntax::bundled_language_dir());
    ide_syntax::detect(&profiles, Some(Path::new(path)), &text)
}

pub fn run(path: Option<&str>) -> io::Result<()> {
    // Crossterm drops every color when NO_COLOR is set. This is a full-screen editor.
    crossterm::style::force_color_output(true);
    enable_raw_mode()?;
    ignore_interrupt();
    let _guard = TerminalGuard;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.hide_cursor()?;
    let mut app = if let Some(path) = path {
        let path = PathBuf::from(path);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "\n".to_string());
        let mut app = EditorApp::new(&text);
        app.bind_initial(path);
        app
    } else {
        EditorApp::new("Welcome to Posit Edit.\n")
    };
    let mut last = Instant::now();
    let mut held: Option<Event> = None;
    loop {
        let size = terminal.size()?;
        app.note_pixels(host_pixels());
        let now = Instant::now();
        let dt = now.saturating_duration_since(last);
        last = now;
        app.note_frame(size.width, size.height);
        app.tick(dt);
        app.maintain_terminal();
        present(&mut terminal, &app)?;
        let wait = if app.animating() || app.terminals.wants_poll() {
            Duration::from_millis(16)
        } else if app.terminals.open {
            Duration::from_millis(40)
        } else if let Some(deadline) = app.chord_deadline {
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100))
        } else {
            Duration::from_millis(100)
        };
        let event = if let Some(event) = held.take() {
            Some(event)
        } else if event::poll(wait)? {
            Some(event::read()?)
        } else {
            None
        };
        if let Some(event) = event {
            match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if !app.terminal_key(key.code, key.modifiers) {
                        if let Some(command) = map_key(key.code, key.modifiers) {
                            app.handle(command);
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    let event = match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => Some(Pointer::Down {
                            column: mouse.column,
                            row: mouse.row,
                        }),
                        MouseEventKind::Drag(MouseButton::Left) => Some(Pointer::Drag {
                            column: mouse.column,
                            row: mouse.row,
                        }),
                        MouseEventKind::ScrollUp => Some(Pointer::ScrollUp {
                            column: mouse.column,
                            row: mouse.row,
                        }),
                        MouseEventKind::ScrollDown => Some(Pointer::ScrollDown {
                            column: mouse.column,
                            row: mouse.row,
                        }),
                        MouseEventKind::Down(MouseButton::Right) => Some(Pointer::Right {
                            column: mouse.column,
                            row: mouse.row,
                        }),
                        _ => None,
                    };
                    if let Some(event) = event {
                        app.pointer(event, size.width, size.height);
                    }
                }
                Event::Resize(_, _) => held = drain_resizes()?,
                _ => {}
            }
        } else if app.chord_due() {
            app.flush_chord();
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
        }
        if app.quit() {
            break;
        }
    }
    Ok(())
}

fn paint_notifications(frame: &mut Frame, app: &EditorApp, area: Rect) {
    if app.notifications.is_empty() {
        return;
    }
    if app.notifications_open {
        let rect = notification_rect(area, app.notifications.len());
        frame.render_widget(Clear, rect);
        frame.render_widget(notification_paragraph(app, rect.width), rect);
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
    frame.render_widget(
        Paragraph::new(badge).style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        rect,
    );
}

fn notification_rect(area: Rect, count: usize) -> Rect {
    let width = 46.min(area.width.saturating_sub(2)).max(16);
    let rows = 1 + count.min(4) * 2;
    let height = (rows as u16 + 2).min(area.height.saturating_sub(1)).max(3);
    Rect {
        x: area.width.saturating_sub(width),
        y: area.height.saturating_sub(1).saturating_sub(height),
        width,
        height,
    }
}

fn notification_paragraph(app: &EditorApp, width: u16) -> Paragraph<'static> {
    let inner = width.saturating_sub(2).max(1) as usize;
    let mut lines = Vec::new();
    for note in app.notifications.iter().take(4) {
        lines.push(Line::styled(
            clip_cells(&note.title, inner),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        let body = format!("{} x", note.body.replace('\n', " "));
        lines.push(Line::from(clip_cells(&body, inner)));
    }
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title("Notifications")
            .title(
                Line::from(TuiSpan::styled(" v ", Style::default().fg(Color::Yellow)))
                    .alignment(Alignment::Right),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red)),
    )
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
        "rust" | "c" | "zig" | "swift" => debug_lldb(&project_root(path)),
        _ => Err(format!("no debugger for {language}")),
    }
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
        for dir in ["/usr/bin", "/bin", "/opt/homebrew/bin"] {
            let candidate = Path::new(dir).join(name);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    None
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
    app: &EditorApp,
) -> io::Result<()> {
    // The resize path clears the screen before painting. Hold that pair until the
    // frame is complete so the terminal does not show the blank in between.
    execute!(stdout(), BeginSynchronizedUpdate)?;
    let drawn = terminal.draw(|frame| draw(frame, app));
    execute!(stdout(), EndSynchronizedUpdate)?;
    drawn.map(|_| ())
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
    loop {
        if dir.join("Cargo.toml").exists()
            || dir.join("package.json").exists()
            || dir.join("build.zig").exists()
            || dir.join("flake.nix").exists()
            || dir.join("Package.swift").exists()
            || dir.join("gradlew").exists()
        {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    origin
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
        "java" | "kotlin" => Some("gradle"),
        "javascript" | "typescript" => Some("javascript"),
        _ => None,
    };
    preferred
        .filter(|name| systems.iter().any(|system| system == *name))
        .map(str::to_string)
        .or_else(|| (systems.len() == 1).then(|| systems[0].clone()))
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

fn spawn_detached(program: &str, args: &[String], cwd: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = command.process_group(0);
    }
    command.spawn().map(|_| ()).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("{program} unavailable")
        } else {
            format!("{program} failed: {error}")
        }
    })
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

fn agent_program() -> String {
    let mut names = Vec::new();
    if let Ok(name) = std::env::var("IDE_AGENT") {
        if !name.is_empty() {
            names.push(name);
        }
    }
    if names.is_empty() {
        names.extend([
            "agent".to_string(),
            "codex".to_string(),
            "claude".to_string(),
        ]);
    }
    for name in names {
        let found = std::process::Command::new("which")
            .arg(&name)
            .output()
            .is_ok_and(|output| output.status.success());
        if found {
            return name;
        }
    }
    String::new()
}

fn agent_args(program: &str, prompt: &str) -> Vec<String> {
    match program {
        "codex" => vec!["exec".to_string(), prompt.to_string()],
        _ => vec!["-p".to_string(), prompt.to_string()],
    }
}

fn map_key(code: KeyCode, modifiers: KeyModifiers) -> Option<CommandKey> {
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let shift = modifiers.contains(KeyModifiers::SHIFT);
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
        KeyCode::Left if shift => Some(CommandKey::ShiftLeft),
        KeyCode::Right if shift => Some(CommandKey::ShiftRight),
        KeyCode::Up if shift => Some(CommandKey::ShiftUp),
        KeyCode::Down if shift => Some(CommandKey::ShiftDown),
        KeyCode::Char(ch) if !ctrl => Some(CommandKey::Char(ch)),
        KeyCode::Enter => Some(CommandKey::Enter),
        KeyCode::Backspace => Some(CommandKey::Backspace),
        KeyCode::Delete => Some(CommandKey::Delete),
        KeyCode::Left => Some(CommandKey::Left),
        KeyCode::Right => Some(CommandKey::Right),
        KeyCode::Up => Some(CommandKey::Up),
        KeyCode::Down => Some(CommandKey::Down),
        KeyCode::Esc => Some(CommandKey::Esc),
        KeyCode::F(1) => Some(CommandKey::Help),
        KeyCode::Tab => Some(CommandKey::Tab),
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
    fn split_palette_and_header_render() {
        let mut app = EditorApp::new("hello");
        assert_eq!(app.splits(), 2);
        let screen = rendered(&app);
        assert!(screen.contains("Posit"));
        assert!(screen.contains("auto"));
        assert!(screen.contains("Files"));
        app.handle(CommandKey::CtrlW);
        assert_eq!(app.splits(), 1);
        assert!(rendered(&app).contains("full"));
        app.handle(CommandKey::CtrlP);
        app.handle(CommandKey::Char('q'));
        app.handle(CommandKey::Char('u'));
        app.handle(CommandKey::Char('i'));
        app.handle(CommandKey::Char('t'));
        app.handle(CommandKey::Enter);
        assert!(app.quit());
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
        assert!(app.notice().is_empty());
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
        terminal.draw(|frame| draw(frame, &app)).unwrap();
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
        assert!(screen_has_color(&app, Color::Magenta));
    }

    #[test]
    fn help_button_is_visible_and_opens_the_guide() {
        let app = EditorApp::new("hello");
        let screen = rendered(&app);
        assert!(screen.contains('?'));
        assert!(screen.contains("help"));
        let mut app = app;
        app.pointer(Pointer::Down { column: 75, row: 0 }, 80, 24);
        assert!(app.help_open());
        let mut saw_title = false;
        let mut saw_columns = false;
        let mut saw_close = false;
        let mut saw_rename = false;
        for _ in 0..400 {
            let screen = rendered(&app);
            saw_title |= screen.contains("title bar");
            saw_columns |= screen.contains("80 columns");
            saw_close |= screen.contains("close the buffer");
            saw_rename |= screen.contains("rename");
            app.handle(CommandKey::Down);
        }
        assert!(saw_title);
        assert!(saw_columns);
        assert!(saw_close);
        assert!(saw_rename);
        assert!(help_text().contains("format the file"));
        assert!(help_text().contains("open floating Yazi"));
        app.pointer(Pointer::Down { column: 1, row: 23 }, 80, 24);
        assert!(!app.help_open());
        app.handle(CommandKey::Help);
        assert!(app.help_open());
        app.handle(CommandKey::Esc);
        assert!(!app.help_open());
    }

    #[test]
    fn mouse_click_drag_and_scroll_edit_the_buffer() {
        let mut app = EditorApp::new("hello\nworld");
        app.note_frame(160, 40);
        let (x, y) = app.text_origin().unwrap();
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
    fn elastic_scroll_moves_immediately_and_springs_back() {
        assert_eq!(track_scroll(0.0, 1.0, 10.0, 20.0), 1.0);
        let pulled = track_scroll(0.0, 1.0, 0.0, 20.0);
        assert!(pulled > 0.2 && pulled < 1.0, "{pulled}");
        let mut pos = 0.0;
        let mut vel = 40.0;
        for _ in 0..30 {
            (pos, vel) = step_scroll(pos, vel, 0.016, 80.0, false);
        }
        assert!(pos > 4.0, "{pos}");
        assert!(vel.abs() < 40.0);
        pos = pulled;
        vel = 0.0;
        for _ in 0..40 {
            (pos, vel) = step_scroll(pos, vel, 0.016, 0.0, false);
        }
        assert!(pos.abs() < 0.05, "{pos}");
        assert_eq!(vel, 0.0);
        (pos, vel) = step_scroll(3.4, 12.0, 0.016, 40.0, true);
        assert_eq!(pos, 3.4);
        assert_eq!(vel, 12.0);

        let mut app = EditorApp::new("only\n");
        app.note_frame(120, 40);
        app.pointer(Pointer::ScrollDown { column: 40, row: 8 }, 120, 40);
        assert_eq!(app.scroll(), 0);
        assert!(app.buffers[0].scroll_pos > 0.2);
        app.wheel_at = Some(Instant::now() - Duration::from_millis(80));
        for _ in 0..40 {
            app.tick(Duration::from_millis(16));
        }
        assert_eq!(app.scroll(), 0);
        assert!(app.buffers[0].scroll_pos.abs() < 0.05);
        assert!(!app.animating());

        let mut app = EditorApp::new(&"line\n".repeat(80));
        app.note_frame(120, 40);
        app.pointer(Pointer::ScrollDown { column: 40, row: 8 }, 120, 40);
        assert_eq!(app.scroll(), 1);
        app.pointer(Pointer::ScrollDown { column: 40, row: 8 }, 120, 40);
        assert_eq!(app.scroll(), 2);
    }

    #[test]
    fn copy_and_paste_use_the_system_clipboard() {
        let previous = clipboard::get().ok();
        let sample = "axiom-clip\nsecond";
        clipboard::set(sample).expect("system clipboard");
        assert_eq!(clipboard::get().expect("system clipboard"), sample);

        let mut app = EditorApp::new("keep me");
        app.handle(CommandKey::Char('a'));
        app.handle(CommandKey::Char('y'));
        assert_eq!(app.notice(), "copied");
        assert_eq!(clipboard::get().unwrap(), "keep me");
        clipboard::set("ZZ").unwrap();
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
        app.pointer(Pointer::Right { column: x, row: y }, 160, 40);
        let screen = rendered_at(&app, 160, 40);
        assert!(screen.contains("Clipboard"));
        assert!(screen.contains("Cut"));
        assert!(screen.contains("Copy"));
        assert!(screen.contains("Paste"));
        assert!(screen.contains("Select All"));
        app.pointer(
            Pointer::Down {
                column: x + 2,
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
        let rect = app.terminal_rect();
        app.pointer(
            Pointer::Down {
                column: rect.x + 2,
                row: rect.y + 2,
            },
            80,
            24,
        );
        assert!(app.terminal_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.quit());
    }

    #[test]
    fn terminal_panel_is_half_the_screen_and_multiplexes() {
        let mut app = EditorApp::new("file\n");
        app.note_frame(80, 24);
        assert_eq!(app.terminal_rect().height, 12);
        app.handle(CommandKey::Char('t'));
        assert_eq!(app.terminal_rect().height, 1);
        assert_eq!(app.notice(), "terminal off");
        app.handle(CommandKey::Char('t'));
        assert_eq!(app.terminal_rect().height, 12);
        let rect = app.terminal_rect();
        let (body, _) = app.terminals.grid_size(rect.width, rect.height);
        let side = rect.width.saturating_sub(body);
        assert!(side >= 8);
        app.pointer(
            Pointer::Down {
                column: rect.x + body,
                row: rect.y + rect.height - 2,
            },
            80,
            24,
        );
        app.pointer(
            Pointer::Down {
                column: rect.x + body,
                row: rect.y + rect.height - 2,
            },
            80,
            24,
        );
        assert_eq!(app.terminals.len(), 2);
        let screen = rendered_at(&app, 80, 24);
        assert!(screen.contains('+'));
        assert!(screen.contains('x'));
        assert!(screen.contains('1'));
        assert!(screen.contains('2'));
        assert!(rect.y > 0);
        assert_eq!(rect.width, 80);
        let before = app.terminals.len();
        app.pointer(
            Pointer::Down {
                column: rect.x + rect.width - 1,
                row: rect.y,
            },
            80,
            24,
        );
        assert!(app.terminals.len() < before);
        app.pointer(
            Pointer::Down {
                column: rect.x + body,
                row: rect.y + rect.height - 1,
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
        assert!(rendered(&app).contains("saves and closes"));
        app.handle(CommandKey::Esc);
        assert!(app.text().contains('!'));
        app.handle(CommandKey::Char('w'));
        app.handle(CommandKey::Char('s'));
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
        app.handle(CommandKey::Char('n'));
        assert!(!app.quit());
        assert!(app.text().contains('x'));
        app.handle(CommandKey::Char('q'));
        app.handle(CommandKey::Char('y'));
        assert!(app.quit());
        assert_eq!(fs::read_to_string(&other).unwrap(), "two\nx");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn click_past_the_line_puts_the_cursor_on_the_last_character() {
        let mut app = EditorApp::new("hi\nworld");
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
        app.note_frame(188, 40);
        assert_eq!(app.window().tile_width, 128);
        let (x, y) = app.text_origin().unwrap();
        let row = row_symbols(&app, 188, 40, y);
        assert_eq!(row[x as usize..(x as usize + 90)].concat(), "a".repeat(90));

        app.add_buffer("other", "b\n");
        app.note_frame(188, 40);
        assert_eq!(app.window().tile_width, 64);
        let (x, y) = app.text_origin().unwrap();
        let row = row_symbols(&app, 188, 40, y);
        assert_eq!(row[x as usize..(x as usize + 60)].concat(), "a".repeat(60));
        assert_ne!(row[x as usize + 62], "a");
        app.add_buffer("third", "c\n");
        app.note_frame(188, 40);
        assert!(app.tile_corner(2).is_none());
    }

    #[test]
    fn tile_edge_drag_widens_a_clipped_line_and_fullscreen_does_not() {
        let mut app = EditorApp::new(&"a".repeat(90));
        app.add_buffer("other", "b\n");
        app.note_frame(188, 40);
        let before = app.window().along(0);
        assert_eq!(before, app.window().along(1));
        let (x, y) = app.text_origin().unwrap();
        let row = row_symbols(&app, 188, 40, y);
        let visible_before = row[x as usize..]
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
        assert!(app.window().along(1) < before);
        assert_eq!(app.window().along(0) + app.window().along(1), before * 2);
        let (x, y) = app.text_origin().unwrap();
        let row = row_symbols(&app, 188, 40, y);
        let visible_after = row[x as usize..]
            .iter()
            .take_while(|cell| *cell == "a")
            .count();
        assert!(visible_after > visible_before);

        app.note_frame(50, 110);
        assert_eq!(app.axis(), Axis::Vertical);
        let top = app.window().along(0);
        assert_eq!(top, app.window().along(1));
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
        assert_eq!(app.window().along(0) + app.window().along(1), top * 2);

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
        assert!(!app.editing);
        assert_eq!(app.text(), "one\n");
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

        app.handle(CommandKey::Char('q'));
        assert!(!app.quit());
        assert!(rendered(&app).contains("saves and quits"));
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
        assert!(guide.contains("key"));
        assert!(guide.contains("action"));
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
        let (x, y) = app.text_origin().unwrap();
        assert_eq!(cell_bg(&app, 160, 40, x, y), SELECTION_BG);
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
        let (x, y) = app.text_origin().unwrap();
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
        assert_eq!(cell_bg(&app, 160, 40, x + 11, y), SELECTION_BG);
        assert_eq!(cell_fg(&app, 160, 40, x, y), Color::Reset);
        editing(&mut app);
        app.handle(CommandKey::Backspace);
        assert_eq!(app.text(), "second");

        let mut app = EditorApp::new("fn main() {\n    // note\n}\n");
        app.set_language("rust");
        app.note_frame(80, 24);
        app.handle(CommandKey::Char('a'));
        let (keyword, comment) = selection_colors(&app, 80, 24);
        assert_eq!(keyword, (Color::Magenta, SELECTION_BG));
        assert_eq!(comment.1, SELECTION_BG);
        assert_eq!(comment.0, Color::Yellow);
        assert_eq!(foreground_on_selection(Color::DarkGray), Color::Gray);
        assert_eq!(foreground_on_selection(Color::LightYellow), Color::Gray);
        assert_eq!(foreground_on_selection(Color::Magenta), Color::Magenta);
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
        assert!(app.text().contains("cargo"));
        assert!(app.text().contains("build"));
        assert!(!root.join("target").exists());
        app.handle(CommandKey::Char('y'));
        assert_eq!(app.notice(), "untrusted workspace stays preview-only");
        assert!(!root.join("target").exists());
        app.handle(CommandKey::Char('9'));
        assert!(!app.text().contains("not attached"));
        let screen = rendered_at(&app, 120, 30);
        assert!(screen.contains("Debug"));
        if app.notice().contains("debugging with") {
            assert!(app.terminals.len() >= 1);
            assert!(screen.contains("lldb"));
        } else {
            assert!(app.notifications_open);
            assert!(
                screen.contains("lldb")
                    || screen.contains("debug binary")
                    || screen.contains("debugger")
            );
        }
        editing(&mut app);
        app.handle(CommandKey::Char('1'));
        assert!(app.text().contains('1'));
        let _ = fs::remove_dir_all(&root);
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
        }
        let _ = fs::remove_dir_all(root);
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

    fn selection_colors(
        app: &EditorApp,
        width: u16,
        height: u16,
    ) -> ((Color, Color), (Color, Color)) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let cells = terminal.backend().buffer().content();
        let keyword = cells
            .windows(2)
            .find(|pair| pair[0].symbol() == "f" && pair[1].symbol() == "n")
            .map(|pair| (pair[0].fg, pair[0].bg))
            .unwrap_or((Color::Reset, Color::Reset));
        let comment = cells
            .windows(4)
            .find(|window| {
                window[0].symbol() == "n"
                    && window[1].symbol() == "o"
                    && window[2].symbol() == "t"
                    && window[3].symbol() == "e"
            })
            .map(|window| (window[0].fg, window[0].bg))
            .unwrap_or((Color::Reset, Color::Reset));
        (keyword, comment)
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
}
