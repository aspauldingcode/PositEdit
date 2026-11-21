//! Multiplexed shell panel. Each session is a real PTY, not a text buffer.
use std::ffi::CString;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    fg: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Reset,
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
    pending: Vec<u8>,
    csi: Vec<u8>,
    state: Ansi,
    scrollback: Vec<Vec<Cell>>,
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
            pending: Vec::new(),
            csi: Vec::new(),
            state: Ansi::Ground,
            scrollback: Vec::new(),
            view: 0,
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
            return;
        }
        for param in params {
            self.fg = match param {
                0 | 39 => Color::Reset,
                30 => Color::Black,
                31 => Color::Red,
                32 => Color::Green,
                33 => Color::Yellow,
                34 => Color::Blue,
                35 => Color::Magenta,
                36 => Color::Cyan,
                37 => Color::Gray,
                90 => Color::DarkGray,
                91 => Color::LightRed,
                92 => Color::LightGreen,
                93 => Color::LightYellow,
                94 => Color::LightBlue,
                95 => Color::LightMagenta,
                96 => Color::LightCyan,
                97 => Color::White,
                _ => self.fg,
            };
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
            *cell = Cell { ch, fg: self.fg };
        }
        self.x += 1;
    }

    fn line_feed(&mut self) {
        if self.y + 1 < self.rows {
            self.y += 1;
            return;
        }
        let row: Vec<Cell> = self.cells[..self.cols].to_vec();
        self.scrollback.push(row);
        if self.scrollback.len() > 500 {
            self.scrollback.remove(0);
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

struct Session {
    title: String,
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
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
}

const SIDEBAR_WIDTH: u16 = 18;

impl Panel {
    pub fn new() -> Self {
        Self {
            open: true,
            focused: false,
            focus: 0,
            sessions: Vec::new(),
            spawn_error: None,
            active: false,
            list_view: 0,
        }
    }

    pub fn grid_size(&self, width: u16, height: u16) -> (u16, u16) {
        let (body, _) = split_width(width);
        (body.max(2), height.max(1))
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn focused(&self) -> bool {
        self.focused && self.open
    }

    pub fn blur(&mut self) {
        self.focused = false;
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
        self.add(cwd, cols, rows)
    }

    pub fn run_program(
        &mut self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
    ) -> Result<(), String> {
        let session =
            Session::command(cwd, cols, rows, program, args).map_err(|error| error.to_string())?;
        self.sessions.push(session);
        self.focus = self.sessions.len() - 1;
        self.open = true;
        self.focused = true;
        self.reveal(24);
        Ok(())
    }

    pub fn add(&mut self, cwd: &Path, cols: u16, rows: u16) -> Result<(), String> {
        match Session::spawn(cwd, cols, rows) {
            Ok(session) => {
                self.sessions.push(session);
                self.focus = self.sessions.len() - 1;
                self.open = true;
                self.focused = true;
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
        let (body, side) = split_width(width);
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
        let (body, side) = split_width(width);
        if side > 0 && local_x >= body {
            return self.click_side(local_x - body, local_y, side, height);
        }
        self.focused = true;
        Click::Focus
    }

    fn click_side(&mut self, local_x: u16, local_y: u16, side: u16, height: u16) -> Click {
        if local_y + 1 == height {
            self.toggle();
            return Click::Toggle;
        }
        if height >= 2 && local_y + 2 == height {
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

    pub fn paragraph(&self, width: u16, height: u16) -> Paragraph<'static> {
        let width = width.max(1);
        let height = height.max(1);
        if height == 1 || !self.open {
            return Paragraph::new(collapsed_line(width));
        }
        let (body, side) = split_width(width);
        let rows = height as usize;
        let mut lines = Vec::with_capacity(rows);
        let message = self.spawn_error.clone().unwrap_or_else(|| {
            if self.sessions.is_empty() {
                "no terminal yet. press + to add one".to_string()
            } else {
                String::new()
            }
        });
        for row in 0..rows {
            let main = self.main_line(row, body as usize, &message);
            let tab = sidebar_line(self, row as u16, side, height);
            lines.push(join_line(main, tab));
        }
        Paragraph::new(lines)
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
        let mut line = grid_line(&cells, cursor.then_some(session.grid.x));
        let shown = line
            .spans
            .iter()
            .map(|span| span.content.chars().count())
            .sum::<usize>();
        if shown < body {
            line.spans
                .push(ratatui::text::Span::raw(" ".repeat(body - shown)));
        }
        line
    }
}

fn split_width(width: u16) -> (u16, u16) {
    if width < SIDEBAR_WIDTH + 8 {
        return (width.max(1), 0);
    }
    let side = SIDEBAR_WIDTH.min(width / 3).max(8);
    (width.saturating_sub(side).max(1), side)
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

fn sidebar_line(panel: &Panel, row: u16, side: u16, height: u16) -> Line<'static> {
    if side == 0 {
        return Line::from("");
    }
    let text = if row + 1 == height {
        fit_side("t", side)
    } else if height >= 2 && row + 2 == height {
        fit_side("+", side)
    } else {
        let index = panel.list_view + row as usize;
        if let Some(session) = panel.sessions.get(index) {
            session_label(index, &session.title, index == panel.focus, side)
        } else {
            " ".repeat(side as usize)
        }
    };
    let style = if row + 1 < height.saturating_sub(1)
        && panel.list_view + row as usize == panel.focus
        && panel.list_view + (row as usize) < panel.sessions.len()
    {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(ratatui::style::Modifier::BOLD)
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

fn join_line(main: Line<'static>, side: Line<'static>) -> Line<'static> {
    let mut spans = main.spans;
    spans.extend(side.spans);
    Line::from(spans)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Focus,
    Add,
    Close,
    Toggle,
}

fn grid_line(cells: &[Cell], cursor: Option<usize>) -> Line<'static> {
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
        if cursor == Some(index) {
            next = next.add_modifier(ratatui::style::Modifier::REVERSED);
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
}
