//! Terminal editor with scrolling 80-column tiles, a file tree, and Yazi.
//! Drawing and commands are testable without a tty.
//! The interactive loop restores the terminal on return and on drop.
mod files;
mod finder;
mod layout;

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
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use files::ChooseError;
use ide_core::{Document, Edit};
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
    anchor: usize,
    selection: Option<(usize, usize)>,
    language: String,
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
    agent_focused: bool,
    agent_draft: String,
    agent_log: Vec<String>,
    agent_program: String,
    overlay: Overlay,
    chord: String,
    chord_deadline: Option<Instant>,
    click: Option<(Instant, u16, u16, u8)>,
    select_by: SelectBy,
}

const HELP_INTRO: &str = "\
How to use Axiom

The yellow ? in the title bar opens and closes this guide.
Click it, or the ? help label on the status line. F1 does the same.
Esc closes the guide.

Buffers
  A wide terminal lays files out left to right. Columns cap at 80 columns
  once enough files are open to fill the width.
  80 columns or less: one buffer, as wide as the window.
  81 to 160: two columns, when two files are open.
  161 to 240: three columns. Wider windows add another every 80.
  Fewer open files grow to fill the window instead of leaving empty space.
  Files beyond the visible columns stay on the strip. Focus scrolls to them.

  A tall terminal stacks files top to bottom and uses the full width.
  There is no 80-column cap in that direction.

Editing
  The editor starts in view mode. Letters run commands.
  Enter on the focused buffer starts editing. Esc returns to view mode.
  Click places the cursor. Drag selects. Double-click selects a word.
  Triple-click selects the line. Shift+arrows select while editing.
  Ctrl-A selects all. In view mode, a does too.
  A second letter extends a bind. Wait, and the short bind runs.
";

fn keybind_rows() -> Vec<(&'static str, &'static str)> {
    let mut rows = vec![
        (",", "open settings"),
        (":", "open the command line"),
        ("?", "open this guide"),
        ("[", "focus the first buffer"),
        ("]", "focus the last buffer"),
        ("{", "move this buffer to the start"),
        ("}", "move this buffer to the end"),
        ("b", "build"),
        ("a", "select all"),
        ("bb", "toggle the file sidebar"),
        ("cc", "toggle the agent chat"),
        ("Ctrl-A", "select all"),
        ("Ctrl-Down", "focus the next buffer when the window is tall"),
        (
            "Ctrl-Left",
            "focus the previous buffer when the window is wide",
        ),
        ("Ctrl-P", "open the command line"),
        ("Ctrl-Q", "quit"),
        (
            "Ctrl-Right",
            "focus the next buffer when the window is wide",
        ),
        ("Ctrl-S", "save"),
        (
            "Ctrl-Up",
            "focus the previous buffer when the window is tall",
        ),
        ("Ctrl-W", "toggle fullscreen"),
        ("Ctrl-Z", "undo the last edit"),
        (
            "Down",
            "scroll the file, or focus the next buffer when tall",
        ),
        ("Enter", "enter editing on the focused buffer"),
        ("Esc", "leave editing and return to view mode"),
        ("f", "toggle fullscreen"),
        ("ff", "format the file"),
        ("F1", "open this guide"),
        ("l", "search open buffers"),
        (
            "Left",
            "scroll the file, or focus the previous buffer when wide",
        ),
        ("ll", "search files"),
        ("lll", "live grep"),
        ("o", "open floating Yazi"),
        ("q", "quit after confirmation"),
        ("r", "run"),
        (
            "Right",
            "scroll the file, or focus the next buffer when wide",
        ),
        ("rr", "rename the file"),
        ("s", "save"),
        (
            "Shift-Down",
            "view: swap with the next buffer. edit: select down",
        ),
        (
            "Shift-Left",
            "view: swap with the previous buffer. edit: select left",
        ),
        (
            "Shift-Right",
            "view: swap with the next buffer. edit: select right",
        ),
        (
            "Shift-Up",
            "view: swap with the previous buffer. edit: select up",
        ),
        (
            "Up",
            "scroll the file, or focus the previous buffer when tall",
        ),
        ("w", "close the buffer"),
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
pub enum Pointer {
    Down { column: u16, row: u16 },
    Drag { column: u16, row: u16 },
    ScrollUp,
    ScrollDown,
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

const SELECTION_BG: Color = Color::Rgb(255, 214, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinderKind {
    Files,
    Grep,
    Buffers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Overlay {
    None,
    Quit,
    Rename(String),
    Settings,
    Vim,
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
            anchor: cursor,
            selection: None,
            language: language.into(),
        }
    }

    fn text(&self) -> String {
        self.document.to_text()
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
        (self.target - self.view).abs() >= 0.05
    }

    pub fn note_frame(&mut self, width: u16, height: u16) {
        self.width = width.max(1);
        self.height = height.max(1);
        let orientation = layout::axis(self.width, self.height);
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
        self.target = self.focus as f32 * self.stride as f32;
        if changed {
            self.view = self.target;
        }
    }

    pub fn tick(&mut self, dt: Duration) {
        let delta = self.target - self.view;
        if delta.abs() < 0.05 {
            self.view = self.target;
            return;
        }
        let step = 1.0 - (-dt.as_secs_f32() / 0.12).exp();
        self.view += delta * step;
    }

    pub fn handle(&mut self, key: CommandKey) {
        self.dispatch(key);
        self.note_frame(self.width, self.height);
    }

    pub fn pointer(&mut self, event: Pointer, width: u16, height: u16) {
        self.note_frame(width, height);
        let area = Rect::new(0, 0, self.width, self.height);
        let chrome = layout::chrome(area, self.blocks_status(), self.sidebar, self.chat);
        let window = self.window();
        match event {
            Pointer::ScrollUp => {
                if self.yazi.is_some() {
                    self.move_yazi(-1);
                } else if self.help {
                    self.scroll_help(-1);
                } else {
                    let scroll = self.focused().scroll.saturating_sub(1);
                    self.focused_mut().scroll = scroll;
                }
            }
            Pointer::ScrollDown => {
                if self.yazi.is_some() {
                    self.move_yazi(1);
                } else if self.help {
                    self.scroll_help(1);
                } else {
                    let scroll = self.focused().scroll.saturating_add(1);
                    self.focused_mut().scroll = scroll;
                    self.clamp_scroll();
                }
            }
            Pointer::Down { column, row } | Pointer::Drag { column, row } => {
                let drag = matches!(event, Pointer::Drag { .. });
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
                    Overlay::Quit | Overlay::Rename(_) | Overlay::Vim
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
                let Some(index) =
                    layout::hit_tile(window, column, row, self.view, self.buffers.len())
                else {
                    return;
                };
                if drag && index != self.focus {
                    return;
                }
                if !drag && self.close_button(window, index, column, row) {
                    self.close_buffer(index);
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
        self.palette || matches!(self.overlay, Overlay::Quit | Overlay::Rename(_))
    }

    fn dispatch(&mut self, key: CommandKey) {
        if self.overlay != Overlay::None && !self.help && !self.palette {
            self.handle_overlay(key);
            return;
        }
        if self.help {
            match key {
                CommandKey::Help | CommandKey::Esc => self.help = false,
                CommandKey::CtrlQ => self.quit = true,
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
            CommandKey::CtrlQ => self.quit = true,
            CommandKey::CtrlP => {
                self.palette = true;
                self.query.clear();
            }
            CommandKey::CtrlW => self.show_many = !self.show_many,
            CommandKey::CtrlS => self.save(),
            CommandKey::Help => self.open_help(),
            CommandKey::CtrlZ => self.focused_mut().undo(),
            CommandKey::CtrlA => self.select_all(),
            CommandKey::CtrlLeft | CommandKey::CtrlRight if self.axis == Axis::Vertical => {}
            CommandKey::CtrlLeft => self.shift_focus(-1),
            CommandKey::CtrlRight => self.shift_focus(1),
            CommandKey::CtrlUp | CommandKey::CtrlDown if self.axis == Axis::Horizontal => {}
            CommandKey::CtrlUp => self.shift_focus(-1),
            CommandKey::CtrlDown => self.shift_focus(1),
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
            CommandKey::CtrlQ => self.quit = true,
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
            "bb" => {
                self.clear_chord();
                self.sidebar = !self.sidebar;
                self.notice = if self.sidebar {
                    "sidebar on".to_string()
                } else {
                    "sidebar off".to_string()
                };
            }
            "cc" => {
                self.clear_chord();
                self.toggle_chat();
            }
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
            "b" | "c" | "r" | "f" | "l" | "ll" => {
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
            'q' => self.overlay = Overlay::Quit,
            's' => self.save(),
            'w' => self.close_buffer(self.focus),
            'o' => self.open_yazi(),
            'a' => self.select_all(),
            ',' => self.overlay = Overlay::Settings,
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
            'b' | 'c' | 'r' | 'f' | 'l' => {
                self.chord = ch.to_string();
                self.chord_deadline = Some(Instant::now() + Duration::from_millis(450));
            }
            _ => {}
        }
    }

    fn view_motion(&mut self, key: CommandKey, swap: bool) {
        let horizontal = self.axis == Axis::Horizontal;
        let along = match key {
            CommandKey::Left | CommandKey::ShiftLeft => horizontal.then_some(-1),
            CommandKey::Right | CommandKey::ShiftRight => horizontal.then_some(1),
            CommandKey::Up | CommandKey::ShiftUp => (!horizontal).then_some(-1),
            CommandKey::Down | CommandKey::ShiftDown => (!horizontal).then_some(1),
            _ => None,
        };
        if let Some(delta) = along {
            if swap {
                self.swap_buffer(delta);
            } else {
                self.shift_focus(delta);
            }
            return;
        }
        if swap {
            return;
        }
        let delta = match key {
            CommandKey::Up | CommandKey::Left => -1,
            _ => 1,
        };
        let scroll = if delta < 0 {
            self.focused().scroll.saturating_sub(1)
        } else {
            self.focused().scroll.saturating_add(1)
        };
        self.focused_mut().scroll = scroll;
        self.clamp_scroll();
    }

    fn handle_overlay(&mut self, key: CommandKey) {
        match &self.overlay.clone() {
            Overlay::Quit => match key {
                CommandKey::Char('y') => self.quit = true,
                CommandKey::Char('n') | CommandKey::Esc => self.overlay = Overlay::None,
                _ => {}
            },
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
                CommandKey::Esc | CommandKey::Char(',') => self.overlay = Overlay::None,
                CommandKey::Char('s') => self.sidebar = !self.sidebar,
                CommandKey::Char('c') => self.toggle_chat(),
                CommandKey::Char('f') => self.show_many = !self.show_many,
                _ => {}
            },
            Overlay::Vim => {
                if matches!(key, CommandKey::Esc | CommandKey::Enter) {
                    self.overlay = Overlay::None;
                }
            }
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
                match command.as_str() {
                    "quit" => self.quit = true,
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

    fn close_buffer(&mut self, index: usize) {
        if index >= self.buffers.len() {
            return;
        }
        self.buffers.remove(index);
        if self.buffers.is_empty() {
            self.buffers
                .push(EditBuffer::from_text("untitled", "\n", "plain"));
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
        let buffer = self.buffers.remove(from);
        self.buffers.insert(to, buffer);
        self.focus_at(to);
    }

    fn swap_buffer(&mut self, delta: isize) {
        let next = self.focus as isize + delta;
        if next < 0 || next >= self.buffers.len() as isize {
            return;
        }
        let next = next as usize;
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

    fn show_output(&mut self, title: &str, text: &str) {
        if let Some(index) = self.buffers.iter().position(|buffer| buffer.name == title) {
            self.buffers[index].replace_all(text);
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
        self.focus = index.min(self.buffers.len() - 1);
        self.tree_focused = false;
        self.target = self.focus as f32 * self.stride as f32;
    }

    fn focused(&self) -> &EditBuffer {
        &self.buffers[self.focus]
    }

    fn focused_mut(&mut self) -> &mut EditBuffer {
        &mut self.buffers[self.focus]
    }

    fn metrics_for(&self, width: u16, height: u16, orientation: Axis) -> (Chrome, Window) {
        let chrome = layout::chrome(
            Rect::new(0, 0, width, height),
            self.blocks_status(),
            self.sidebar,
            self.chat,
        );
        let window = layout::window(
            chrome.content,
            orientation,
            self.show_many,
            self.buffers.len(),
        );
        (chrome, window)
    }

    fn window(&self) -> Window {
        self.metrics_for(self.width, self.height, self.axis).1
    }

    fn visible_rows(&self) -> usize {
        self.window().tile_height.saturating_sub(2) as usize
    }

    fn clamp_scroll(&mut self) {
        let visible = self.visible_rows().max(1);
        let lines = self.focused().text().split('\n').count();
        let max_scroll = lines.saturating_sub(visible);
        let buffer = self.focused_mut();
        buffer.scroll = buffer.scroll.min(max_scroll);
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
        let Some(path) = self.focused().path.clone() else {
            self.notice = "save needs a path".to_string();
            return;
        };
        match std::fs::write(&path, self.focused().text()) {
            Ok(()) => self.notice = "saved".to_string(),
            Err(error) => self.notice = format!("save failed: {error}"),
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
        )
        .body;
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
        layout::chrome(
            Rect::new(0, 0, self.width, self.height),
            self.blocks_status(),
            self.sidebar,
            self.chat,
        )
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
            CommandKey::CtrlQ => self.quit = true,
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
            CommandKey::CtrlQ => self.quit = true,
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
            CommandKey::CtrlQ => self.quit = true,
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
    let chrome = layout::chrome(area, app.blocks_status(), app.sidebar, app.chat);
    let gaze = if app.editing { "EDIT" } else { "VIEW" };
    let header = Paragraph::new(format!(
        "Axiom {}  {}  {gaze}",
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
    if app.help {
        let body = Paragraph::new(help_text())
            .scroll((app.help_scroll.min(u16::MAX as usize) as u16, 0))
            .block(Block::default().title("Help").borders(Borders::ALL));
        frame.render_widget(body, chrome.body);
    } else if app.finder.is_some() {
        frame.render_widget(finder_paragraph(app), chrome.body);
    } else if matches!(app.overlay, Overlay::Settings) {
        frame.render_widget(settings_paragraph(app), chrome.body);
    } else if matches!(app.overlay, Overlay::Vim) {
        frame.render_widget(vim_paragraph(), chrome.body);
    } else {
        if chrome.tree.width > 0 {
            if app.sidebar && chrome.tree.width > 4 {
                frame.render_widget(tree_paragraph(app), chrome.tree);
            } else {
                frame.render_widget(sidebar_rail(), chrome.tree);
            }
        }
        if chrome.agent.width > 4 {
            frame.render_widget(agent_paragraph(app), chrome.agent);
        } else if chrome.agent.width > 0 {
            frame.render_widget(agent_rail(), chrome.agent);
        }
        let orientation = layout::axis(area.width, area.height);
        let window = layout::window(
            chrome.content,
            orientation,
            app.show_many,
            app.buffers.len(),
        );
        for index in 0..app.buffers.len() {
            if let Some(slice) = layout::tile_slice(window, index, app.view) {
                paint_tile(frame, app, index, window, slice);
            }
        }
    }
    let notice = if app.notice.is_empty() {
        String::new()
    } else {
        format!("  {}", app.notice)
    };
    let axis = match layout::axis(area.width, area.height) {
        Axis::Horizontal => "left/right",
        Axis::Vertical => "top/bottom",
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
        let indent = "  ".repeat(entry.depth);
        let mut style = if entry.dir {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        if app.tree_focused && index == app.tree_cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        lines.push(Line::styled(
            format!("{indent}{marker}{}", entry.name),
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
            Line::from(TuiSpan::styled(" « ", Style::default().fg(Color::Yellow)))
                .alignment(Alignment::Right),
        )
        .borders(Borders::ALL)
}

fn agent_paragraph(app: &EditorApp) -> Paragraph<'static> {
    let mut lines = vec![Line::styled(
        if app.agent_program.is_empty() {
            "no CLI".to_string()
        } else {
            app.agent_program.clone()
        },
        Style::default().fg(Color::Cyan),
    )];
    let start = app.agent_log.len().saturating_sub(12);
    for line in &app.agent_log[start..] {
        lines.push(Line::from(line.clone()));
    }
    let draft = if app.agent_focused {
        format!("› {}", app.agent_draft)
    } else {
        "click to ask".to_string()
    };
    lines.push(Line::styled(
        draft,
        Style::default().fg(if app.agent_focused {
            focus_color(true)
        } else {
            Color::Gray
        }),
    ));
    let border = if app.agent_focused {
        focus_color(false)
    } else {
        Color::Gray
    };
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title("Agent")
            .title(
                Line::from(TuiSpan::styled(" » ", Style::default().fg(Color::Yellow)))
                    .alignment(Alignment::Right),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border)),
    )
}

fn agent_rail() -> Paragraph<'static> {
    Paragraph::new("«")
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
    Paragraph::new("»")
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
        Line::from(format!("sidebar   {sidebar}    s or click")),
        Line::from(format!("chat      {chat}       c or click")),
        Line::from(format!("columns   {columns}    f or click")),
        Line::from(""),
        Line::from("Esc or , closes"),
    ]))
    .block(Block::default().title("Settings").borders(Borders::ALL))
}

fn prompt_line(app: &EditorApp) -> Option<String> {
    match &app.overlay {
        Overlay::Quit => Some("quit?  y confirms, n or Esc stays".to_string()),
        Overlay::Rename(name) => Some(format!("rename: {name}")),
        Overlay::None | Overlay::Settings | Overlay::Vim => None,
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
        Overlay::Rename(_) => "Rename",
        Overlay::None | Overlay::Settings | Overlay::Vim => "",
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

fn paint_tile(
    frame: &mut Frame,
    app: &EditorApp,
    index: usize,
    window: Window,
    slice: layout::Slice,
) {
    let area = Rect::new(0, 0, window.tile_width, window.tile_height.max(1));
    let mut screen = Screen::empty(area);
    let buffer = &app.buffers[index];
    let border = if index == app.focus {
        focus_color(app.editing)
    } else {
        Color::Gray
    };
    let paragraph = Paragraph::new(highlighted(
        buffer,
        index == app.focus,
        app.editing,
        window.tile_width.saturating_sub(2) as usize,
    ))
    .scroll((buffer.scroll.min(u16::MAX as usize) as u16, 0))
    .block(
        Block::default()
            .title(buffer.name.clone())
            .title(
                Line::from(TuiSpan::styled(" × ", Style::default().fg(Color::Red)))
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

fn header_block() -> Block<'static> {
    Block::default()
        .title("Axiom")
        .title(
            Line::from(TuiSpan::styled(
                " ? ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Right),
        )
        .borders(Borders::ALL)
}

fn help_hit(column: u16, row: u16, area: Rect, status: Option<Rect>) -> bool {
    let title = row == area.y && column + 5 >= area.x + area.width && column < area.x + area.width;
    let label = status.is_some_and(|status| {
        row == status.y && column >= status.x && column < status.x.saturating_add(8)
    });
    title || label
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
    if show_cursor && cursor_byte == text.len() {
        lines
            .last_mut()
            .expect("line")
            .spans
            .push(TuiSpan::styled(" ", cursor_style(editing)));
    }
    pad_selected_rows(&mut lines, buffer, &text, show_cursor, content_width);
    Text::from(lines)
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
                let caret = usize::from(show_cursor && buffer.cursor == chars && end == chars);
                let used = row_chars + caret;
                let room = content_width.saturating_sub(used);
                if room > 0 {
                    line.spans
                        .push(TuiSpan::styled(" ".repeat(room), selection_style()));
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
        let mut part_style = if selected { selection_style() } else { style };
        if cursor_byte == Some(absolute) {
            part_style = cursor_style(editing);
        }
        line.spans
            .push(TuiSpan::styled(piece[start..end].to_string(), part_style));
    }
}

fn selection_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(SELECTION_BG)
        .add_modifier(Modifier::BOLD)
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
    let _guard = TerminalGuard;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut app = if let Some(path) = path {
        let path = PathBuf::from(path);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "\n".to_string());
        let mut app = EditorApp::new(&text);
        app.bind_initial(path);
        app
    } else {
        EditorApp::new("Welcome to Axiom.\n")
    };
    let mut last = Instant::now();
    loop {
        let size = terminal.size()?;
        let now = Instant::now();
        let dt = now.saturating_duration_since(last);
        last = now;
        app.note_frame(size.width, size.height);
        app.tick(dt);
        terminal.draw(|frame| draw(frame, &app))?;
        let wait = if app.animating() {
            Duration::from_millis(16)
        } else if let Some(deadline) = app.chord_deadline {
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100))
        } else {
            Duration::from_millis(100)
        };
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(command) = map_key(key.code, key.modifiers) {
                        app.handle(command);
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
                        MouseEventKind::ScrollUp => Some(Pointer::ScrollUp),
                        MouseEventKind::ScrollDown => Some(Pointer::ScrollDown),
                        _ => None,
                    };
                    if let Some(event) = event {
                        app.pointer(event, size.width, size.height);
                    }
                }
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
        assert!(screen.contains("Axiom"));
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
        app.pointer(Pointer::Down { column: 77, row: 0 }, 80, 24);
        assert!(app.help_open());
        let screen = rendered(&app);
        assert!(screen.contains("title bar"));
        assert!(screen.contains("80 columns"));
        for _ in 0..200 {
            app.handle(CommandKey::Down);
        }
        let screen = rendered(&app);
        assert!(screen.contains("format"));
        assert!(screen.contains("Yazi"));
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
        app.pointer(Pointer::ScrollDown, 80, 8);
        assert_eq!(app.scroll(), 1);
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
        assert!(rendered_at(&app, 50, 110).contains("top/bottom"));
        let focus = app.focus_index();
        app.handle(CommandKey::CtrlLeft);
        assert_eq!(app.focus_index(), focus);
        app.handle(CommandKey::CtrlDown);
        assert_eq!(app.focus_index(), 1);
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
        app.handle(CommandKey::Char('n'));
        assert!(!app.quit());
        app.handle(CommandKey::Char('q'));
        app.handle(CommandKey::Char('y'));
        assert!(app.quit());

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
        app.handle(CommandKey::Char('b'));
        app.handle(CommandKey::Char('b'));
        assert!(!app.sidebar);
        assert!(rendered(&app).contains('»'));
        app.handle(CommandKey::Char('b'));
        app.handle(CommandKey::Char('b'));
        assert!(app.sidebar);

        app.handle(CommandKey::Char('l'));
        app.flush_chord();
        assert!(rendered(&app).contains("Buffers"));
        app.handle(CommandKey::Esc);

        app.handle(CommandKey::Char(','));
        assert!(rendered(&app).contains("sidebar"));
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
        app.pointer(Pointer::Down { column: 26, row: 3 }, 160, 40);
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
        assert!(screen.contains("Axiom"));
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
        assert_eq!(action("bb"), "toggle the file sidebar");
        assert_eq!(action("cc"), "toggle the agent chat");
        assert!(keys.iter().position(|key| *key == "b") < keys.iter().position(|key| *key == "bb"));
        assert!(
            keys.iter().position(|key| *key == "bb") < keys.iter().position(|key| *key == "cc")
        );
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
        app.handle(CommandKey::Char('c'));
        app.flush_chord();
        assert!(rendered_at(&app, 220, 40).contains("Agent"));
        app.handle(CommandKey::Char('c'));
        app.handle(CommandKey::Char('c'));
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
        app.handle(CommandKey::Char(','));
        assert!(rendered_at(&app, 220, 40).contains("chat"));
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
        editing(&mut app);
        app.handle(CommandKey::Backspace);
        assert_eq!(app.text(), "second");
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
