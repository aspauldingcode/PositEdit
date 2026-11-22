//! Multiplexed shell panel. Each session is a real PTY, not a text buffer.
use std::collections::VecDeque;
use std::ffi::CString;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use ide_core::PressureLevel;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    fg: Color,
    bg: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Reset,
            bg: Color::Reset,
        }
    }
}

struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    x: usize,
    y: usize,
    fg: Color,
    bg: Color,
    pending: Vec<u8>,
    csi: Vec<u8>,
    state: Ansi,
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_limit: usize,
    view: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ansi {
    Ground,
    Esc,
    Csi,
    Osc,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            x: 0,
            y: 0,
            fg: Color::Reset,
            bg: Color::Reset,
            pending: Vec::new(),
            csi: Vec::new(),
            state: Ansi::Ground,
            scrollback: VecDeque::new(),
            scrollback_limit: PressureLevel::Normal.scrollback_limit(),
            view: 0,
        }
    }

    fn set_scrollback_limit(&mut self, limit: usize) {
        self.scrollback_limit = limit.max(1);
        while self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
            self.view = self.view.saturating_sub(1);
        }
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        let mut next = vec![Cell::default(); cols * rows];
        for row in 0..self.rows.min(rows) {
            for col in 0..self.cols.min(cols) {
                next[row * cols + col] = self.cells[row * self.cols + col];
            }
        }
        self.cells = next;
        self.cols = cols;
        self.rows = rows;
        self.x = self.x.min(cols.saturating_sub(1));
        self.y = self.y.min(rows.saturating_sub(1));
    }

    fn feed(&mut self, bytes: &[u8]) {
        let follow = self.view == 0;
        self.pending.extend_from_slice(bytes);
        let data = std::mem::take(&mut self.pending);
        let mut rest = data.as_slice();
        while !rest.is_empty() {
            match self.state {
                Ansi::Ground => rest = self.ground(rest),
                Ansi::Esc => rest = self.escape(rest),
                Ansi::Csi => rest = self.csi(rest),
                Ansi::Osc => rest = self.osc(rest),
            }
        }
        if follow {
            self.view = 0;
        }
    }

    fn ground<'a>(&mut self, data: &'a [u8]) -> &'a [u8] {
        match data[0] {
            0x1b => {
                self.state = Ansi::Esc;
                &data[1..]
            }
            b'\n' => {
                self.line_feed();
                &data[1..]
            }
            b'\r' => {
                self.x = 0;
                &data[1..]
            }
            b'\t' => {
                self.x = (self.x + 8) & !7;
                if self.x >= self.cols {
                    self.x = 0;
                    self.line_feed();
                }
                &data[1..]
            }
            0x08 => {
                self.x = self.x.saturating_sub(1);
                &data[1..]
            }
            0x07 => &data[1..],
            byte if byte < 0x20 => &data[1..],
            _ => {
                let (ch, len) = utf8_char(data);
                match ch {
                    Some(ch) => self.put(ch),
                    None if len == 0 => {
                        self.pending.extend_from_slice(data);
                        return &[];
                    }
                    None => self.put('?'),
                }
                &data[len.max(1)..]
            }
        }
    }

    fn escape<'a>(&mut self, data: &'a [u8]) -> &'a [u8] {
        match data[0] {
            b'[' => self.state = Ansi::Csi,
            b']' => self.state = Ansi::Osc,
            _ => self.state = Ansi::Ground,
        }
        self.csi.clear();
        &data[1..]
    }

    fn csi<'a>(&mut self, data: &'a [u8]) -> &'a [u8] {
        let byte = data[0];
        if (0x40..=0x7e).contains(&byte) {
            self.finish_csi(byte);
            self.state = Ansi::Ground;
            self.csi.clear();
        } else {
            self.csi.push(byte);
        }
        &data[1..]
    }

    fn osc<'a>(&mut self, data: &'a [u8]) -> &'a [u8] {
        if data[0] == 0x07 || data[0] == 0x1b {
            self.state = Ansi::Ground;
        }
        &data[1..]
    }

    fn finish_csi(&mut self, command: u8) {
        let params = csi_params(&self.csi);
        let n = |index: usize, default: usize| {
            params
                .get(index)
                .copied()
                .filter(|n| *n > 0)
                .unwrap_or(default)
        };
        match command {
            b'A' => self.y = self.y.saturating_sub(n(0, 1)),
            b'B' => self.y = (self.y + n(0, 1)).min(self.rows.saturating_sub(1)),
            b'C' => self.x = (self.x + n(0, 1)).min(self.cols.saturating_sub(1)),
            b'D' => self.x = self.x.saturating_sub(n(0, 1)),
            b'G' => self.x = n(0, 1).saturating_sub(1).min(self.cols.saturating_sub(1)),
            b'H' | b'f' => {
                self.y = n(0, 1).saturating_sub(1).min(self.rows.saturating_sub(1));
                self.x = n(1, 1).saturating_sub(1).min(self.cols.saturating_sub(1));
            }
            b'J' => self.erase_display(params.first().copied().unwrap_or(0)),
            b'K' => self.erase_line(params.first().copied().unwrap_or(0)),
            b'm' => self.sgr(&params),
            _ => {}
        }
    }

    fn sgr(&mut self, params: &[usize]) {
        if params.is_empty() {
            self.fg = Color::Reset;
            self.bg = Color::Reset;
            return;
        }
        let mut index = 0;
        while index < params.len() {
            let param = params[index];
            match param {
                0 => {
                    self.fg = Color::Reset;
                    self.bg = Color::Reset;
                }
                30..=37 => self.fg = ansi_basic(param - 30),
                90..=97 => self.fg = ansi_bright(param - 90),
                40..=47 => self.bg = ansi_basic(param - 40),
                100..=107 => self.bg = ansi_bright(param - 100),
                39 => self.fg = Color::Reset,
                49 => self.bg = Color::Reset,
                38 | 48 => {
                    let is_fg = param == 38;
                    if let Some(color) = take_extended_color(params, &mut index) {
                        if is_fg {
                            self.fg = color;
                        } else {
                            self.bg = color;
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }

    fn erase_display(&mut self, mode: usize) {
        match mode {
            2 | 3 => self.cells.fill(Cell::default()),
            1 => {
                let end = (self.y * self.cols + self.x.min(self.cols)).min(self.cells.len());
                for cell in &mut self.cells[..end] {
                    *cell = Cell::default();
                }
            }
            _ => {
                let start = (self.y * self.cols + self.x.min(self.cols)).min(self.cells.len());
                for cell in &mut self.cells[start..] {
                    *cell = Cell::default();
                }
            }
        }
    }

    fn erase_line(&mut self, mode: usize) {
        let row = self.y.min(self.rows.saturating_sub(1));
        let start = row * self.cols;
        let end = start + self.cols;
        let (from, to) = match mode {
            1 => (start, start + self.x.min(self.cols)),
            2 => (start, end),
            _ => (start + self.x.min(self.cols), end),
        };
        let len = self.cells.len();
        for cell in &mut self.cells[from.min(len)..to.min(len)] {
            *cell = Cell::default();
        }
    }

    fn put(&mut self, ch: char) {
        if self.x >= self.cols {
            self.x = 0;
            self.line_feed();
        }
        let index = self.y * self.cols + self.x;
        if let Some(cell) = self.cells.get_mut(index) {
            *cell = Cell {
                ch,
                fg: self.fg,
                bg: self.bg,
            };
        }
        self.x += 1;
    }

    fn line_feed(&mut self) {
        if self.y + 1 < self.rows {
            self.y += 1;
            return;
        }
        let row: Vec<Cell> = self.cells[..self.cols].to_vec();
        self.scrollback.push_back(row);
        while self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
            self.view = self.view.saturating_sub(1);
        }
        self.cells.copy_within(self.cols.., 0);
        let start = self.cells.len() - self.cols;
        for cell in &mut self.cells[start..] {
            *cell = Cell::default();
        }
    }

    fn wheel(&mut self, delta: isize) {
        if delta < 0 {
            self.view = (self.view + delta.unsigned_abs()).min(self.scrollback.len());
        } else {
            self.view = self.view.saturating_sub(delta as usize);
        }
    }

    fn line(&self, row: usize) -> Vec<Cell> {
        let rows = self.rows.max(1);
        let start = self.scrollback.len().saturating_sub(self.view);
        let index = start + row;
        if index < self.scrollback.len() {
            let mut line = self.scrollback[index].clone();
            line.resize(self.cols, Cell::default());
            line
        } else {
            let grid_row = index - self.scrollback.len();
            if grid_row >= rows {
                return vec![Cell::default(); self.cols];
            }
            self.cells[grid_row * self.cols..(grid_row + 1) * self.cols].to_vec()
        }
    }
}

fn utf8_char(data: &[u8]) -> (Option<char>, usize) {
    let width = match data[0] {
        byte if byte < 0x80 => 1,
        byte if byte & 0xe0 == 0xc0 => 2,
        byte if byte & 0xf0 == 0xe0 => 3,
        byte if byte & 0xf8 == 0xf0 => 4,
        _ => return (None, 1),
    };
    if data.len() < width {
        return (None, 0);
    }
    match std::str::from_utf8(&data[..width]) {
        Ok(text) => (text.chars().next(), width),
        Err(_) => (None, 1),
    }
}

fn csi_params(bytes: &[u8]) -> Vec<usize> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(bytes);
    text.split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn ansi_basic(index: usize) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::Gray,
    }
}

fn ansi_bright(index: usize) -> Color {
    match index {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        _ => Color::White,
    }
}

/// Parse `38;5;n`, `38;2;r;g;b` (and the `48;` background forms). `index` points at 38/48.
fn take_extended_color(params: &[usize], index: &mut usize) -> Option<Color> {
    let mode = *params.get(*index + 1)?;
    match mode {
        5 => {
            let color = *params.get(*index + 2)?;
            *index += 2;
            Some(Color::Indexed(color.min(255) as u8))
        }
        2 => {
            let r = *params.get(*index + 2)?;
            let g = *params.get(*index + 3)?;
            let b = *params.get(*index + 4)?;
            *index += 4;
            Some(Color::Rgb(
                r.min(255) as u8,
                g.min(255) as u8,
                b.min(255) as u8,
            ))
        }
        _ => None,
    }
}

struct Session {
    title: String,
    runtime_id: Option<String>,
    grid: Grid,
    child: Option<Child>,
    #[cfg(unix)]
    master: Option<i32>,
    cols: u16,
    rows: u16,
}

impl Session {
    fn spawn(cwd: &Path, cols: u16, rows: u16) -> io::Result<Self> {
        let shell = shell_program();
        let title = Path::new(&shell)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("shell")
            .to_string();
        Self::launch(&shell, &[], title, cwd, cols, rows)
    }

    fn command(
        cwd: &Path,
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
    ) -> io::Result<Self> {
        let title = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program)
            .to_string();
        Self::launch(program, args, title, cwd, cols, rows)
    }

    fn launch(
        program: &str,
        args: &[String],
        title: String,
        cwd: &Path,
        cols: u16,
        rows: u16,
    ) -> io::Result<Self> {
        let cols = cols.max(2);
        let rows = rows.max(1);
        #[cfg(not(unix))]
        {
            let _ = (program, args, title, cwd);
            return Err(io::Error::other("interactive terminal needs a Unix PTY"));
        }
        #[cfg(unix)]
        {
            let (master, child) = spawn_pty(program, args, cwd, cols, rows)?;
            Ok(Self {
                title,
                runtime_id: None,
                grid: Grid::new(cols as usize, rows as usize),
                child: Some(child),
                master: Some(master),
                cols,
                rows,
            })
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(2);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.grid.resize(cols as usize, rows as usize);
        #[cfg(unix)]
        if let Some(master) = self.master {
            set_winsize(master, cols, rows);
        }
    }

    fn drain(&mut self) -> (bool, bool) {
        let output = self.read_pty();
        let exited = self.reap();
        (!exited, output)
    }

    fn read_pty(&mut self) -> bool {
        #[cfg(unix)]
        {
            let Some(master) = self.master else {
                return false;
            };
            let mut buf = [0u8; 4096];
            let mut read_any = false;
            loop {
                let n = unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) };
                if n > 0 {
                    self.grid.feed(&buf[..n as usize]);
                    read_any = true;
                } else {
                    break;
                }
            }
            read_any
        }
        #[cfg(not(unix))]
        false
    }

    fn write(&mut self, bytes: &[u8]) {
        #[cfg(unix)]
        if let Some(master) = self.master {
            unsafe {
                libc::write(master, bytes.as_ptr().cast(), bytes.len());
            }
        }
        #[cfg(not(unix))]
        let _ = bytes;
    }

    fn reap(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            self.child = None;
            #[cfg(unix)]
            self.close_master();
            return true;
        }
        false
    }

    #[cfg(unix)]
    fn close_master(&mut self) {
        if let Some(master) = self.master.take() {
            unsafe { libc::close(master) };
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pid = child.id() as i32;
            #[cfg(unix)]
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(unix)]
        self.close_master();
    }
}

fn enriched_path_for_runtimes() -> Option<std::ffi::OsString> {
    let mut dirs = Vec::new();
    // Prefer the forked vendor binaries shipped with this repo.
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            let vendor_bin = dir.join("vendor/bin");
            if vendor_bin.is_dir() {
                dirs.push(vendor_bin);
                break;
            }
            current = dir.parent();
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut dir) = exe.parent().map(|p| p.to_path_buf()) {
            for _ in 0..6 {
                let vendor_bin = dir.join("vendor/bin");
                if vendor_bin.is_dir() {
                    dirs.push(vendor_bin);
                    break;
                }
                if !dir.pop() {
                    break;
                }
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(&home).join(".local/bin"));
    }
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = std::env::var(key) {
            let root = PathBuf::from(root);
            dirs.push(root.join("platform-tools"));
            dirs.push(root.join("emulator"));
        }
    }
    let home_sdk = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Android/sdk"));
    if let Some(sdk) = home_sdk {
        dirs.push(sdk.join("platform-tools"));
        dirs.push(sdk.join("emulator"));
    }
    dirs.retain(|dir| dir.is_dir());
    if dirs.is_empty() {
        return None;
    }
    let mut path = std::env::var_os("PATH").unwrap_or_default();
    for dir in dirs.into_iter().rev() {
        let mut next = dir.into_os_string();
        next.push(":");
        next.push(&path);
        path = next;
    }
    Some(path)
}

/// PATH override used when spawning browser/phone on the host tty (Ghostty split).
#[allow(dead_code)]
pub fn runtime_path_override() -> Option<std::ffi::OsString> {
    enriched_path_for_runtimes()
}

#[cfg(unix)]
fn spawn_pty(
    program: &str,
    args: &[String],
    cwd: &Path,
    cols: u16,
    rows: u16,
) -> io::Result<(i32, Child)> {
    let (master, name) = open_pty(cols, rows)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env(crate::instance::SESSION_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(path) = enriched_path_for_runtimes() {
        command.env("PATH", path);
    }
    unsafe {
        command.pre_exec(move || attach_slave(&name));
    }
    match command.spawn() {
        Ok(child) => Ok((master, child)),
        Err(error) => {
            unsafe { libc::close(master) };
            Err(error)
        }
    }
}

#[cfg(unix)]
fn open_pty(cols: u16, rows: u16) -> io::Result<(i32, CString)> {
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        if master < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::grantpt(master) != 0 || libc::unlockpt(master) != 0 {
            let error = io::Error::last_os_error();
            libc::close(master);
            return Err(error);
        }
        let name_ptr = libc::ptsname(master);
        if name_ptr.is_null() {
            let error = io::Error::last_os_error();
            libc::close(master);
            return Err(error);
        }
        let name = std::ffi::CStr::from_ptr(name_ptr).to_owned();
        set_winsize(master, cols, rows);
        let flags = libc::fcntl(master, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        Ok((master, name))
    }
}

#[cfg(unix)]
fn set_winsize(master: i32, cols: u16, rows: u16) {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe {
        libc::ioctl(master, libc::TIOCSWINSZ, &size);
    }
}

#[cfg(unix)]
fn attach_slave(name: &CString) -> io::Result<()> {
    unsafe {
        let slave = libc::open(name.as_ptr(), libc::O_RDWR);
        if slave < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::setsid() < 0 {
            let error = io::Error::last_os_error();
            libc::close(slave);
            return Err(error);
        }
        libc::ioctl(slave, libc::TIOCSCTTY as libc::c_ulong, 0);
        libc::dup2(slave, 0);
        libc::dup2(slave, 1);
        libc::dup2(slave, 2);
        if slave > 2 {
            libc::close(slave);
        }
        Ok(())
    }
}

fn shell_program() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| Path::new(shell).is_file())
        .unwrap_or_else(|| {
            if Path::new("/bin/zsh").is_file() {
                "/bin/zsh".to_string()
            } else {
                "/bin/sh".to_string()
            }
        })
}

pub struct Panel {
    pub open: bool,
    focused: bool,
    focus: usize,
    sessions: Vec<Session>,
    spawn_error: Option<String>,
    active: bool,
    list_view: usize,
    /// Width of the session list (excluding the divider column).
    side_width: u16,
    /// Console module shares the bottom rect with the PTY terminal.
    pub show_console: bool,
    pub console: Vec<String>,
    console_scroll: usize,
    /// Visible-grid selection for the focused session (col, row), drag-style.
    select_anchor: Option<(u16, u16)>,
    select_end: Option<(u16, u16)>,
    select_by: SelectBy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectBy {
    Char,
    Word,
    Line,
}

const SIDEBAR_WIDTH: u16 = 18;
const SIDEBAR_MIN: u16 = 8;
const SIDEBAR_MAX: u16 = 40;
const DIVIDER: &str = "│";

impl Panel {
    /// Shrink PTY scrollback to the budget for the current OS memory pressure.
    pub fn apply_pressure(&mut self, level: PressureLevel) {
        let limit = level.scrollback_limit();
        for session in &mut self.sessions {
            session.grid.set_scrollback_limit(limit);
        }
        if level.is_elevated() && self.console.len() > 500 {
            let drop = self.console.len() - 500;
            self.console.drain(0..drop);
            self.console_scroll = self.console_scroll.saturating_sub(drop);
        }
    }

    pub fn new() -> Self {
        Self {
            open: true,
            focused: false,
            focus: 0,
            sessions: Vec::new(),
            spawn_error: None,
            active: false,
            list_view: 0,
            side_width: SIDEBAR_WIDTH,
            show_console: false,
            console: Vec::new(),
            console_scroll: 0,
            select_anchor: None,
            select_end: None,
            select_by: SelectBy::Char,
        }
    }

    fn split_width(&self, width: u16) -> (u16, u16) {
        let want = self.side_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        if width < want + 8 {
            return (width.max(1), 0);
        }
        let side = want.min(width / 3).max(SIDEBAR_MIN);
        (width.saturating_sub(side).max(1), side)
    }

    /// Panel-local hit on the session-list divider (resize handle).
    pub fn on_side_seam(&self, local_x: u16, local_y: u16, width: u16, height: u16) -> bool {
        if height <= 2 || !self.open || self.show_console {
            return false;
        }
        if local_y == 0 || local_y + 1 >= height {
            return false;
        }
        let inner_w = width.saturating_sub(2).max(1);
        let (body, side) = self.split_width(inner_w);
        if side == 0 {
            return false;
        }
        let body = body.saturating_sub(1);
        // Exact divider column only — never steal the last PTY body cell.
        let divider = body.saturating_add(1); // panel-local column of │
        local_x == divider
    }

    /// Resize the session list from a panel-local drag column.
    pub fn resize_side(&mut self, local_x: u16, width: u16) {
        // Side occupies columns after the divider up to the right border.
        let right = width.saturating_sub(2); // last content column
        let side = right
            .saturating_sub(local_x)
            .clamp(SIDEBAR_MIN, SIDEBAR_MAX.min(right / 2).max(SIDEBAR_MIN));
        self.side_width = side;
    }

    pub fn grid_size(&self, width: u16, height: u16) -> (u16, u16) {
        // Frame takes two columns and two rows; divider takes one more column.
        let inner_w = width.saturating_sub(2);
        let inner_h = height.saturating_sub(2).max(1);
        let (body, _) = self.split_width(inner_w);
        let body = body.saturating_sub(1).max(2);
        (body, inner_h.max(1))
    }

    pub fn append_console(&mut self, title: &str, text: &str) {
        let stamp = title.trim();
        if !stamp.is_empty() {
            self.console.push(format!("── {stamp} ──"));
        }
        for line in text.lines() {
            self.console.push(line.to_string());
        }
        if self.console.len() > 4_000 {
            let drop = self.console.len() - 4_000;
            self.console.drain(0..drop);
            self.console_scroll = self.console_scroll.saturating_sub(drop);
        }
        self.show_console = true;
        self.open = true;
        self.console_scroll = self.console.len().saturating_sub(1);
    }

    pub fn toggle_console(&mut self) {
        self.show_console = !self.show_console;
        self.open = true;
    }

    pub fn focus_runtime(&mut self, runtime_id: &str) -> bool {
        if let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.runtime_id.as_deref() == Some(runtime_id))
        {
            self.focus = index;
            // Dock owns the view — do not force the bottom Terminal panel open.
            self.focused = true;
            self.show_console = false;
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn runtime_count(&self, runtime_id: &str) -> usize {
        self.sessions
            .iter()
            .filter(|session| session.runtime_id.as_deref() == Some(runtime_id))
            .count()
    }

    #[cfg(test)]
    pub fn focused_text(&self) -> String {
        let Some(session) = self.sessions.get(self.focus) else {
            return String::new();
        };
        let mut out = String::new();
        for row in 0..session.grid.rows {
            let line: String = session
                .grid
                .line(row)
                .iter()
                .map(|cell| cell.ch)
                .collect::<String>()
                .trim_end()
                .to_string();
            if !line.is_empty() {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out
    }

    #[cfg(test)]
    pub fn titles(&self) -> Vec<String> {
        self.sessions.iter().map(|s| s.title.clone()).collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    #[cfg(test)]
    pub fn side_width(&self) -> u16 {
        self.side_width
    }

    pub fn focused(&self) -> bool {
        if !self.focused {
            return false;
        }
        // Bottom terminal panel, or a runtime dock session that keeps keyboard
        // without opening the quarter-height Terminal strip.
        if self.open {
            return true;
        }
        self.sessions
            .get(self.focus)
            .is_some_and(|session| session.runtime_id.is_some())
    }

    /// Release keyboard focus without dropping a text selection.
    pub fn unfocus(&mut self) {
        self.focused = false;
    }

    pub fn blur(&mut self) {
        self.unfocus();
        self.clear_selection();
    }

    pub fn focus_keyboard(&mut self) {
        self.focused = true;
        self.open = true;
    }

    pub fn has_selection(&self) -> bool {
        match (self.select_anchor, self.select_end) {
            (Some(a), Some(b)) => a != b || self.select_by != SelectBy::Char,
            _ => false,
        }
    }

    pub fn clear_selection(&mut self) {
        self.select_anchor = None;
        self.select_end = None;
        self.select_by = SelectBy::Char;
    }

    /// Selected text from the focused session grid, same shape as buffer yank.
    pub fn selected_text(&self) -> Option<String> {
        if !self.has_selection() {
            return None;
        }
        let (start, end) = self.selection_range()?;
        let session = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))?;
        let cols = session.grid.cols.max(1);
        let mut out = String::new();
        for index in start..end {
            let row = index / cols;
            let col = index % cols;
            if col == 0 && !out.is_empty() {
                out.push('\n');
            }
            let ch = session
                .grid
                .line(row)
                .get(col)
                .map(|cell| cell.ch)
                .unwrap_or(' ');
            out.push(ch);
        }
        let trimmed = out
            .lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        (!trimmed.is_empty()).then_some(trimmed)
    }

    pub fn select_all_visible(&mut self) {
        let Some(session) = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))
        else {
            return;
        };
        let cols = session.grid.cols.max(1) as u16;
        let rows = session.grid.rows.max(1) as u16;
        self.select_anchor = Some((0, 0));
        self.select_end = Some((cols.saturating_sub(1), rows.saturating_sub(1)));
        self.select_by = SelectBy::Line;
        self.focused = true;
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let (ax, ay) = self.select_anchor?;
        let (bx, by) = self.select_end?;
        let session = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))?;
        let cols = session.grid.cols.max(1);
        let a = ay as usize * cols + ax as usize;
        let b = by as usize * cols + bx as usize;
        let (start, end) = if a <= b { (a, b + 1) } else { (b, a + 1) };
        (start < end).then_some((start, end))
    }

    fn cell_selected(&self, col: usize, row: usize) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        let Some(session) = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))
        else {
            return false;
        };
        let cols = session.grid.cols.max(1);
        let index = row * cols + col;
        index >= start && index < end
    }

    /// Pointer in the PTY body. Click/drag selects like a text buffer.
    pub fn body_pointer(
        &mut self,
        local_x: u16,
        local_y: u16,
        width: u16,
        height: u16,
        drag: bool,
        click_count: u8,
    ) -> bool {
        if height <= 1 || !self.open || self.show_console || self.sessions.is_empty() {
            return false;
        }
        if local_x == 0 || local_y == 0 || local_x + 1 >= width || local_y + 1 >= height {
            return false;
        }
        let inner_x = local_x.saturating_sub(1);
        let inner_y = local_y.saturating_sub(1);
        let inner_w = width.saturating_sub(2).max(1);
        let (body, side) = self.split_width(inner_w);
        let body = body.saturating_sub(if side > 0 { 1 } else { 0 });
        // Divider and session list are not text-selection targets.
        if side > 0 && inner_x >= body {
            return false;
        }
        let session = &self.sessions[self.focus.min(self.sessions.len() - 1)];
        let col = (inner_x as usize).min(session.grid.cols.saturating_sub(1)) as u16;
        let row = (inner_y as usize).min(session.grid.rows.saturating_sub(1)) as u16;
        if drag {
            // Selecting terminal text must not capture the keyboard — Esc fights are gone.
            self.focused = false;
            match self.select_by {
                SelectBy::Char => {
                    self.select_end = Some((col, row));
                }
                SelectBy::Word => self.extend_word(col, row),
                SelectBy::Line => self.extend_line(row),
            }
            return true;
        }
        self.select_by = match click_count {
            2 => SelectBy::Word,
            3 => SelectBy::Line,
            _ => SelectBy::Char,
        };
        match self.select_by {
            SelectBy::Char => {
                // Click to type into the shell.
                self.select_anchor = Some((col, row));
                self.select_end = Some((col, row));
                self.focused = true;
            }
            SelectBy::Word => {
                self.select_word(col, row);
                self.focused = false;
            }
            SelectBy::Line => {
                self.select_line(row);
                self.focused = false;
            }
        }
        true
    }

    fn select_word(&mut self, col: u16, row: u16) {
        let Some((start, end)) = self.word_bounds(col, row) else {
            self.select_anchor = Some((col, row));
            self.select_end = Some((col, row));
            return;
        };
        self.select_anchor = Some((start, row));
        self.select_end = Some((end, row));
    }

    fn extend_word(&mut self, col: u16, row: u16) {
        let Some((anchor_col, anchor_row)) = self.select_anchor else {
            self.select_word(col, row);
            return;
        };
        let Some((start, end)) = self.word_bounds(col, row) else {
            self.select_end = Some((col, row));
            return;
        };
        if (row, col) < (anchor_row, anchor_col) {
            self.select_end = Some((start, row));
        } else {
            self.select_end = Some((end, row));
        }
    }

    fn select_line(&mut self, row: u16) {
        let Some(session) = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))
        else {
            return;
        };
        let last = session.grid.cols.saturating_sub(1) as u16;
        self.select_anchor = Some((0, row));
        self.select_end = Some((last, row));
    }

    fn extend_line(&mut self, row: u16) {
        let Some((_, anchor_row)) = self.select_anchor else {
            self.select_line(row);
            return;
        };
        let Some(session) = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))
        else {
            return;
        };
        let last = session.grid.cols.saturating_sub(1) as u16;
        if row <= anchor_row {
            self.select_anchor = Some((0, row));
            self.select_end = Some((last, anchor_row));
        } else {
            self.select_anchor = Some((0, anchor_row));
            self.select_end = Some((last, row));
        }
    }

    fn word_bounds(&self, col: u16, row: u16) -> Option<(u16, u16)> {
        let session = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))?;
        let line = session.grid.line(row as usize);
        if line.is_empty() {
            return None;
        }
        let col = (col as usize).min(line.len().saturating_sub(1));
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.';
        if !is_word(line[col].ch) {
            return Some((col as u16, col as u16));
        }
        let mut start = col;
        while start > 0 && is_word(line[start - 1].ch) {
            start -= 1;
        }
        let mut end = col;
        while end + 1 < line.len() && is_word(line[end + 1].ch) {
            end += 1;
        }
        Some((start as u16, end as u16))
    }

    pub fn wants_poll(&self) -> bool {
        self.open && self.active
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.focused = false;
        }
    }

    pub fn ensure(&mut self, cwd: &Path, cols: u16, rows: u16) -> Result<(), String> {
        if !self.open
            || !self.sessions.is_empty()
            || self.spawn_error.is_some()
            || cols < 2
            || rows < 1
        {
            return Ok(());
        }
        // Auto-spawn stays in the background. The text buffer keeps keyboard focus.
        self.spawn_shell(cwd, cols, rows, false)
    }

    pub fn run_program(
        &mut self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
    ) -> Result<(), String> {
        self.run_runtime(None, cwd, cols, rows, program, args)
    }

    pub fn run_runtime(
        &mut self,
        runtime_id: Option<&str>,
        cwd: &Path,
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
    ) -> Result<(), String> {
        if let Some(id) = runtime_id {
            if self.focus_runtime(id) {
                return Ok(());
            }
        }
        let mut session =
            Session::command(cwd, cols, rows, program, args).map_err(|error| error.to_string())?;
        session.runtime_id = runtime_id.map(str::to_string);
        if let Some(id) = runtime_id {
            session.title = id.to_string();
        }
        self.sessions.push(session);
        self.focus = self.sessions.len() - 1;
        self.open = true;
        self.focused = true;
        self.show_console = false;
        self.reveal(24);
        Ok(())
    }

    pub fn add(&mut self, cwd: &Path, cols: u16, rows: u16) -> Result<(), String> {
        self.spawn_shell(cwd, cols, rows, true)
    }

    fn spawn_shell(
        &mut self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        focus: bool,
    ) -> Result<(), String> {
        match Session::spawn(cwd, cols, rows) {
            Ok(session) => {
                self.sessions.push(session);
                self.focus = self.sessions.len() - 1;
                self.open = true;
                if focus {
                    self.focused = true;
                }
                self.spawn_error = None;
                self.reveal(24);
                Ok(())
            }
            Err(error) => {
                self.spawn_error = Some(error.to_string());
                Err(error.to_string())
            }
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if !self.open || cols < 2 || rows < 1 {
            return;
        }
        for session in &mut self.sessions {
            session.resize(cols, rows);
        }
    }

    pub fn drain(&mut self) -> usize {
        let mut active = false;
        let before = self.sessions.len();
        self.sessions.retain_mut(|session| {
            let (alive, output) = session.drain();
            active |= output;
            alive
        });
        self.active = active;
        if self.sessions.is_empty() {
            self.focus = 0;
            self.focused = false;
            self.list_view = 0;
        } else if self.focus >= self.sessions.len() {
            self.focus = self.sessions.len() - 1;
        }
        self.reveal(24);
        before - self.sessions.len()
    }

    pub fn wheel(&mut self, delta: isize, local_x: u16, width: u16) {
        let (body, side) = self.split_width(width);
        if side > 0 && local_x >= body {
            if delta < 0 {
                self.list_view = self.list_view.saturating_sub(delta.unsigned_abs());
            } else {
                self.list_view = self.list_view.saturating_add(delta as usize);
            }
            let max = self.sessions.len().saturating_sub(1);
            self.list_view = self.list_view.min(max);
            return;
        }
        if let Some(session) = self.sessions.get_mut(self.focus) {
            session.grid.wheel(delta);
        }
    }

    /// `(offset from top of history, total lines)` for the right-edge scrollbar.
    pub fn scroll_metrics(&self) -> (usize, usize) {
        let Some(session) = self.sessions.get(self.focus.min(self.sessions.len().saturating_sub(1)))
        else {
            return (0, 1);
        };
        let back = session.grid.scrollback.len();
        let rows = session.grid.rows.max(1);
        let offset = back.saturating_sub(session.grid.view);
        (offset, back + rows)
    }

    pub fn write_key(&mut self, bytes: &[u8]) -> bool {
        let Some(session) = self.sessions.get_mut(self.focus) else {
            return false;
        };
        if session.child.is_none() {
            return false;
        }
        session.write(bytes);
        session.grid.view = 0;
        true
    }

    pub fn click(&mut self, local_x: u16, local_y: u16, width: u16, height: u16) -> Click {
        if height <= 1 || !self.open {
            self.toggle();
            return Click::Toggle;
        }
        if self.toggle_hit(local_x, local_y, width) {
            self.toggle();
            return Click::Toggle;
        }
        if self.switch_hit(local_x, local_y, width) {
            self.toggle_console();
            return Click::Switch;
        }
        // Border inset: content lives inside the frame.
        if local_x == 0 || local_y == 0 || local_x + 1 >= width || local_y + 1 >= height {
            self.focused = !self.show_console;
            return Click::Focus;
        }
        let inner_x = local_x.saturating_sub(1);
        let inner_y = local_y.saturating_sub(1);
        let inner_w = width.saturating_sub(2).max(1);
        let inner_h = height.saturating_sub(2).max(1);
        if self.show_console {
            self.focused = false;
            return Click::Focus;
        }
        let (body, side) = self.split_width(inner_w);
        let body = body.saturating_sub(if side > 0 { 1 } else { 0 });
        let side_start = body.saturating_add(if side > 0 { 1 } else { 0 });
        if side > 0 && inner_x == body {
            // Divider is a resize handle, not a focus click.
            self.focused = true;
            return Click::Focus;
        }
        if side > 0 && inner_x >= side_start {
            return self.click_side(inner_x - side_start, inner_y, side, inner_h);
        }
        self.focused = true;
        Click::Focus
    }

    fn click_side(&mut self, local_x: u16, local_y: u16, side: u16, height: u16) -> Click {
        // Bottom row of the session list is `+` (add). Toggle lives top-right now.
        if local_y + 1 == height {
            return Click::Add;
        }
        let index = self.list_view + local_y as usize;
        if index < self.sessions.len() {
            if local_x + 1 >= side {
                self.close_at(index);
                return Click::Close;
            }
            self.focus = index;
            self.focused = true;
            self.show_console = false;
            self.clear_selection();
            self.reveal(height);
            return Click::Focus;
        }
        Click::Focus
    }

    fn close_at(&mut self, index: usize) {
        if index >= self.sessions.len() {
            return;
        }
        self.sessions.remove(index);
        if self.sessions.is_empty() {
            self.focus = 0;
            self.focused = false;
            self.list_view = 0;
            return;
        }
        if self.focus > index {
            self.focus -= 1;
        }
        if self.focus >= self.sessions.len() {
            self.focus = self.sessions.len() - 1;
        }
        self.reveal(24);
    }

    fn reveal(&mut self, height: u16) {
        let room = session_room(height);
        if room == 0 || self.sessions.is_empty() {
            self.list_view = 0;
            return;
        }
        if self.focus < self.list_view {
            self.list_view = self.focus;
        } else if self.focus >= self.list_view + room {
            self.list_view = self.focus + 1 - room;
        }
        let max = self.sessions.len().saturating_sub(room);
        self.list_view = self.list_view.min(max);
    }

    /// Legacy nested-PTY dock view (superseded by the dedicated Canvas pane).
    #[allow(dead_code)]
    pub fn dock_paragraph(
        &self,
        runtime_id: &str,
        width: u16,
        height: u16,
        border: Color,
    ) -> Paragraph<'static> {
        let width = width.max(1);
        let height = height.max(1);
        let title: String = match runtime_id {
            "browser" => "Browser".into(),
            "phone" => "Phone".into(),
            "android" => "Android".into(),
            other => other.to_string(),
        };
        let block = Block::default()
            .title(title.clone())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border));
        if height <= 2 {
            return Paragraph::new(String::new()).block(block);
        }
        let inner_w = width.saturating_sub(2).max(1) as usize;
        let inner_h = height.saturating_sub(2).max(1) as usize;
        let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.runtime_id.as_deref() == Some(runtime_id))
        else {
            return Paragraph::new(format!("{title} panel — press 6/7/8 to launch"))
                .block(block);
        };
        let session = &self.sessions[index];
        let mut lines = Vec::with_capacity(inner_h);
        for row in 0..inner_h {
            let cells = session.grid.line(row);
            let cursor = session.grid.view == 0 && row == session.grid.y && self.focused;
            let mut line = grid_line(&cells, cursor.then_some(session.grid.x), None);
            let shown = line
                .spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum::<usize>();
            if shown < inner_w {
                line.spans
                    .push(Span::raw(" ".repeat(inner_w - shown)));
            }
            lines.push(line);
        }
        Paragraph::new(lines).block(block)
    }

    pub fn paragraph(
        &self,
        width: u16,
        height: u16,
        hover: Option<(u16, u16)>,
        border: Color,
    ) -> Paragraph<'static> {
        let width = width.max(1);
        let height = height.max(1);
        let title = if self.show_console {
            "Console"
        } else {
            "Terminal"
        };
        // Match Files `<`, Agent `>`, buffer `w`: yellow key hint top-right.
        let toggle = " t ";
        let switch = if self.show_console { " term " } else { " log " };
        let toggle_hot = hover.is_some_and(|(column, row)| Self::title_toggle_hit(column, row, width));
        let switch_hot = hover.is_some_and(|(column, row)| Self::title_switch_hit(column, row, width));
        let hint = |label: &'static str, hot: bool| {
            Span::styled(
                label,
                if hot {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                },
            )
        };
        let block = Block::default()
            .title(title)
            .title(
                Line::from(vec![hint(switch, switch_hot), hint(toggle, toggle_hot)])
                    .alignment(ratatui::layout::Alignment::Right),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border));
        if height <= 2 || !self.open {
            return Paragraph::new(collapsed_line(width.saturating_sub(2).max(1))).block(block);
        }
        let inner_w = width.saturating_sub(2).max(1);
        let inner_h = height.saturating_sub(2).max(1);
        if self.show_console {
            return self.console_paragraph(inner_w, inner_h).block(block);
        }
        let (body, side) = self.split_width(inner_w);
        let body = body.saturating_sub(if side > 0 { 1 } else { 0 }).max(1);
        let rows = inner_h as usize;
        let mut lines = Vec::with_capacity(rows);
        let message = self.spawn_error.clone().unwrap_or_else(|| {
            if self.sessions.is_empty() {
                "no terminal yet — press + (or click +) to open a shell".to_string()
            } else {
                String::new()
            }
        });
        let seam_hot = hover.is_some_and(|(column, row_y)| {
            row_y > 0
                && row_y + 1 < height
                && self.on_side_seam(column, row_y, width, height)
        });
        for row in 0..rows {
            let main = self.main_line(row, body as usize, &message);
            let tab = sidebar_line(self, row as u16, side, inner_h, hover, body);
            lines.push(join_line(main, tab, side > 0, seam_hot));
        }
        Paragraph::new(lines).block(block)
    }

    fn console_paragraph(&self, width: u16, height: u16) -> Paragraph<'static> {
        let visible = height.max(1) as usize;
        let start = self
            .console_scroll
            .saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        if self.console.is_empty() {
            lines.push(Line::from("workbench output appears here"));
        } else {
            for line in self.console.iter().skip(start).take(visible) {
                let clipped: String = line.chars().take(width as usize).collect();
                lines.push(Line::from(clipped));
            }
        }
        while lines.len() < visible {
            lines.push(Line::from(""));
        }
        Paragraph::new(lines)
    }

    pub fn wheel_console(&mut self, delta: isize) {
        if self.console.is_empty() {
            return;
        }
        if delta < 0 {
            self.console_scroll = self.console_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.console_scroll =
                (self.console_scroll + delta as usize).min(self.console.len().saturating_sub(1));
        }
    }

    /// Top-right `" t "` title hint (same slot as buffer `w` / Files `<`).
    pub fn toggle_hit(&self, local_x: u16, local_y: u16, width: u16) -> bool {
        Self::title_toggle_hit(local_x, local_y, width)
    }

    /// Console/terminal switch immediately left of the `t` toggle.
    pub fn switch_hit(&self, local_x: u16, local_y: u16, width: u16) -> bool {
        Self::title_switch_hit(local_x, local_y, width)
    }

    fn title_toggle_hit(local_x: u16, local_y: u16, width: u16) -> bool {
        local_y == 0 && width > 0 && local_x + 3 >= width
    }

    fn title_switch_hit(local_x: u16, local_y: u16, width: u16) -> bool {
        local_y == 0
            && width > 3
            && !Self::title_toggle_hit(local_x, local_y, width)
            && local_x + 9 >= width
    }

    fn main_line(&self, row: usize, body: usize, message: &str) -> Line<'static> {
        if self.sessions.is_empty() {
            let text = if row == 0 {
                message.chars().take(body).collect::<String>()
            } else {
                String::new()
            };
            return Line::from(text);
        }
        let session = &self.sessions[self.focus.min(self.sessions.len() - 1)];
        let cells = session.grid.line(row);
        let cursor = session.grid.view == 0 && row == session.grid.y && self.focused;
        let selected = self.has_selection().then(|| {
            (0..cells.len())
                .map(|col| self.cell_selected(col, row))
                .collect::<Vec<_>>()
        });
        let mut line = grid_line(
            &cells,
            cursor.then_some(session.grid.x),
            selected.as_deref(),
        );
        let shown = line
            .spans
            .iter()
            .map(|span| span.content.chars().count())
            .sum::<usize>();
        if shown < body {
            let pad = body - shown;
            let pad_selected = self.has_selection()
                && (shown..shown + pad).any(|col| self.cell_selected(col, row));
            let style = if pad_selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            line.spans
                .push(ratatui::text::Span::styled(" ".repeat(pad), style));
        }
        line
    }
}

fn session_room(height: u16) -> usize {
    height.saturating_sub(2) as usize
}

fn collapsed_line(width: u16) -> Line<'static> {
    let mut text = format!("{:width$}", "t  terminal", width = width as usize);
    if text.chars().count() > width as usize {
        text = text.chars().take(width as usize).collect();
    }
    Line::styled(
        text,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )
}

fn sidebar_line(
    panel: &Panel,
    row: u16,
    side: u16,
    height: u16,
    hover: Option<(u16, u16)>,
    body: u16,
) -> Line<'static> {
    if side == 0 {
        return Line::from("");
    }
    let index = panel.list_view + row as usize;
    let is_session = row + 1 < height.saturating_sub(1) && index < panel.sessions.len();
    let text = if row + 1 == height {
        fit_side("+", side)
    } else if let Some(session) = panel.sessions.get(index) {
        session_label(index, &session.title, index == panel.focus, side)
    } else {
        " ".repeat(side as usize)
    };
    // Panel-local: content starts at column 1; divider at body+1; list at body+2.
    let side_start = body.saturating_add(2);
    let side_end = side_start.saturating_add(side);
    let hovered = hover.is_some_and(|(column, row_y)| {
        row_y.saturating_sub(1) == row && column >= side_start && column < side_end
    });
    let focused = is_session && index == panel.focus;
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if hovered {
        // Cyan = click to select / activate, matching other panes.
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if is_session || row + 1 == height || (height >= 2 && row + 2 == height) {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::styled(text, style)
}

fn session_label(index: usize, title: &str, _active: bool, side: u16) -> String {
    let room = side.saturating_sub(2) as usize;
    let mut label = format!(" {} {title}", index + 1);
    if label.chars().count() > room {
        label = label.chars().take(room).collect();
    }
    let pad = room.saturating_sub(label.chars().count());
    format!("{label}{} x", " ".repeat(pad))
}

fn fit_side(label: &str, side: u16) -> String {
    let mut text = format!(" {label}");
    if text.chars().count() < side as usize {
        text.push_str(&" ".repeat(side as usize - text.chars().count()));
    } else {
        text = text.chars().take(side as usize).collect();
    }
    text
}

fn join_line(
    main: Line<'static>,
    side: Line<'static>,
    divider: bool,
    seam_hot: bool,
) -> Line<'static> {
    let mut spans = main.spans;
    if divider {
        // Always visible resize affordance; yellow when the pointer is on it.
        let style = if seam_hot {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(DIVIDER, style));
    }
    spans.extend(side.spans);
    Line::from(spans)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Focus,
    Add,
    Close,
    Toggle,
    Switch,
}

fn grid_line(cells: &[Cell], cursor: Option<usize>, selected: Option<&[bool]>) -> Line<'static> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut style = Style::default();
    let flush =
        |spans: &mut Vec<ratatui::text::Span<'static>>, current: &mut String, style: Style| {
            if !current.is_empty() {
                spans.push(ratatui::text::Span::styled(std::mem::take(current), style));
            }
        };
    for (index, cell) in cells.iter().enumerate() {
        let mut next = Style::default().fg(cell.fg);
        if cell.bg != Color::Reset {
            next = next.bg(cell.bg);
        }
        if selected.is_some_and(|mask| mask.get(index) == Some(&true)) {
            // Host terminal paints its default selection via reverse video.
            next = next.add_modifier(Modifier::REVERSED);
        } else if cursor == Some(index) {
            next = next.add_modifier(Modifier::REVERSED);
        }
        if next != style && !current.is_empty() {
            flush(&mut spans, &mut current, style);
        }
        style = next;
        current.push(cell.ch);
    }
    flush(&mut spans, &mut current, style);
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn scrollback_stays_bounded_under_line_storm() {
        let mut grid = Grid::new(8, 2);
        grid.set_scrollback_limit(12);
        for _ in 0..200 {
            grid.feed(b"abcdefgh\n");
        }
        assert!(
            grid.scrollback.len() <= 12,
            "scrollback unbounded: {}",
            grid.scrollback.len()
        );
        grid.set_scrollback_limit(4);
        assert!(grid.scrollback.len() <= 4);
    }

    #[test]
    fn grid_prints_and_scrolls() {
        let mut grid = Grid::new(8, 2);
        grid.feed(b"hello\r\nnext");
        let first: String = grid.line(0).iter().map(|cell| cell.ch).collect();
        let second: String = grid.line(1).iter().map(|cell| cell.ch).collect();
        assert_eq!(first.trim_end(), "hello");
        assert_eq!(second.trim_end(), "next");
    }

    #[test]
    fn sgr_keeps_background_and_truecolor_for_fastfetch() {
        let mut grid = Grid::new(8, 2);
        // fastfetch logos use background cells (40–47 / 100–107), not only fg.
        grid.feed(b"\x1b[41mA\x1b[102mB\x1b[0m\x1b[38;2;10;20;30mC\x1b[48;5;12mD");
        assert_eq!(grid.cells[0].ch, 'A');
        assert_eq!(grid.cells[0].bg, Color::Red);
        assert_eq!(grid.cells[1].ch, 'B');
        assert_eq!(grid.cells[1].bg, Color::LightGreen);
        assert_eq!(grid.cells[2].ch, 'C');
        assert_eq!(grid.cells[2].fg, Color::Rgb(10, 20, 30));
        assert_eq!(grid.cells[2].bg, Color::Reset);
        assert_eq!(grid.cells[3].ch, 'D');
        assert_eq!(grid.cells[3].bg, Color::Indexed(12));
        let line = grid_line(&grid.line(0)[..4], None, None);
        assert!(
            line.spans.iter().any(|span| span.style.bg == Some(Color::Red)),
            "{line:?}"
        );
    }

    #[test]
    fn body_pointer_selects_and_yanks_like_a_buffer() {
        let mut panel = Panel::new();
        panel
            .run_program(
                Path::new("."),
                24,
                6,
                "/bin/sh",
                &["-c".into(), "printf 'hello world\\nsecond'; sleep 0.2".into()],
            )
            .expect("shell");
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) {
            panel.drain();
            let line: String = panel.sessions[0]
                .grid
                .line(0)
                .iter()
                .map(|cell| cell.ch)
                .collect();
            if line.contains("hello") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // Frame inset: first cell is local (1,1). Width 40 keeps a sidebar.
        assert!(panel.body_pointer(1, 1, 40, 8, false, 1));
        assert!(!panel.has_selection());
        assert!(panel.body_pointer(11, 1, 40, 8, true, 1));
        assert!(panel.has_selection());
        let text = panel.selected_text().expect("selection");
        assert!(text.contains("hello"), "{text}");
        assert!(panel.body_pointer(1, 1, 40, 8, false, 2));
        let word = panel.selected_text().expect("word");
        assert!(word.contains("hello"), "{word}");
    }

    #[test]
    fn exit_closes_the_session() {
        let mut panel = Panel::new();
        panel
            .run_program(
                Path::new("."),
                20,
                8,
                "/bin/sh",
                &["-c".into(), "exit".into()],
            )
            .expect("shell");
        assert_eq!(panel.len(), 1);
        let start = std::time::Instant::now();
        while panel.len() > 0 && start.elapsed() < std::time::Duration::from_secs(2) {
            panel.drain();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(panel.len(), 0);
    }

    #[test]
    fn side_seam_resizes_and_stays_visible() {
        let mut panel = Panel::new();
        assert_eq!(panel.side_width, SIDEBAR_WIDTH);
        let width = 60_u16;
        let height = 10_u16;
        let (body, side) = panel.split_width(width.saturating_sub(2));
        assert!(side > 0);
        let body = body.saturating_sub(1);
        let divider = body.saturating_add(1);
        assert!(panel.on_side_seam(divider, 2, width, height));
        assert!(
            !panel.on_side_seam(divider.saturating_sub(1), 2, width, height),
            "body cell left of divider must stay selectable"
        );
        assert!(!panel.on_side_seam(2, 2, width, height));
        panel.resize_side(divider.saturating_sub(6), width);
        assert!(panel.side_width > SIDEBAR_WIDTH, "{}", panel.side_width);
        panel.resize_side(divider.saturating_add(8), width);
        assert!(panel.side_width < SIDEBAR_WIDTH, "{}", panel.side_width);
        assert!(panel.side_width >= SIDEBAR_MIN);
    }

    #[test]
    fn browser_phone_android_live_inside_terminal_buffer() {
        let browser = std::env::var("IDE_TERMINAL_BROWSER").ok();
        let phone = std::env::var("IDE_TERMINAL_PHONE").ok();
        let Some(browser) = browser.filter(|p| Path::new(p).is_file()) else {
            eprintln!("skip: IDE_TERMINAL_BROWSER not set to an executable");
            return;
        };
        let Some(phone) = phone.filter(|p| Path::new(p).is_file()) else {
            eprintln!("skip: IDE_TERMINAL_PHONE not set to an executable");
            return;
        };
        let android_py = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../extensions/project-dlc/project_dlc/android_pane.py");
        assert!(android_py.is_file(), "{}", android_py.display());

        let mut panel = Panel::new();
        let cwd = Path::new(".");
        panel
            .run_runtime(
                Some("browser"),
                cwd,
                72,
                18,
                &browser,
                &["open".into(), "http://example.com".into()],
            )
            .expect("browser pane");
        panel
            .run_runtime(
                Some("phone"),
                cwd,
                72,
                18,
                &phone,
                &["view".into(), "default".into()],
            )
            .expect("phone pane");
        panel
            .run_runtime(
                Some("android"),
                cwd,
                72,
                18,
                "/usr/bin/python3",
                &[
                    android_py.display().to_string(),
                    "Medium_Phone_API_36.1".into(),
                ],
            )
            .expect("android pane");
        assert_eq!(panel.runtime_count("browser"), 1);
        assert_eq!(panel.runtime_count("phone"), 1);
        assert_eq!(panel.runtime_count("android"), 1);
        assert_eq!(panel.len(), 3);
        assert_eq!(panel.titles(), vec!["browser", "phone", "android"]);

        // Re-focus must not spawn a second browser pane.
        panel
            .run_runtime(
                Some("browser"),
                cwd,
                72,
                18,
                &browser,
                &["open".into(), "http://example.com".into()],
            )
            .unwrap();
        assert_eq!(panel.runtime_count("browser"), 1);
        assert_eq!(panel.len(), 3);

        let mut saw_browser = false;
        let mut saw_phone = false;
        let mut saw_android = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            panel.drain();
            for id in ["browser", "phone", "android"] {
                assert!(panel.focus_runtime(id));
                let text = panel.focused_text().to_ascii_lowercase();
                if id == "browser"
                    && (text.contains("browser")
                        || text.contains("example")
                        || text.contains("documentation"))
                {
                    saw_browser = true;
                }
                if id == "phone" && (text.contains("phone") || text.contains("booted") || text.contains("sim"))
                {
                    saw_phone = true;
                }
                if id == "android"
                    && (text.contains("android")
                        || text.contains("frame")
                        || text.contains("emulator")
                        || text.contains("png"))
                {
                    saw_android = true;
                }
            }
            if saw_browser && saw_phone && saw_android {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        assert!(saw_browser, "browser pane had no recognizable output");
        assert!(saw_phone, "phone pane had no recognizable output");
        assert!(saw_android, "android pane had no recognizable output");
    }
}
