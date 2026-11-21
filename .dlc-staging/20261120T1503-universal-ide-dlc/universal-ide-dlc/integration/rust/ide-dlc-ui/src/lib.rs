//! Additive UI and typed command bridge for the Universal Project Workbench.
//! Does not replace the host IDE's renderer or core.
use std::{io, path::{Path, PathBuf}, process::Command};
use crossterm::event::{KeyCode, MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, style::{Color, Modifier, Style}, widgets::{Block, Borders, Paragraph}, Frame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkbenchAction { Build, Run, Configure, Test, Refresh, Browser, Phone, Android }

const BUTTONS: [(&str, WorkbenchAction); 8] = [
    (" Build ",WorkbenchAction::Build), (" Run ",WorkbenchAction::Run),
    (" Configure ",WorkbenchAction::Configure), (" Test ",WorkbenchAction::Test),
    (" Refresh ",WorkbenchAction::Refresh), (" Browser ",WorkbenchAction::Browser),
    (" Phone ",WorkbenchAction::Phone), (" Android ",WorkbenchAction::Android),
];

#[derive(Default)]
pub struct WorkbenchToolbar { hits: Vec<(Rect, WorkbenchAction)> }

impl WorkbenchToolbar {
    /// Paints selectable toolbar controls. Returns the actual clickable rectangles.
    /// Caller should map mouse events to `mouse_action` and dispatch the resulting action.
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        self.hits.clear();
        if area.height == 0 {return;}
        let mut x = area.x;
        let mut y = area.y;
        for (label, action) in BUTTONS {
            let width = label.len() as u16 + 2;
            if x.saturating_add(width) > area.right() {
                x = area.x;
                y = y.saturating_add(1);
            }
            if y >= area.bottom() || x.saturating_add(width) > area.right() { break; }
            let cell = Rect::new(x, y, width, 1);
            frame.render_widget(
                Paragraph::new(label).block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
                    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                cell,
            );
            self.hits.push((cell, action));
            x = x.saturating_add(width + 1);
        }
    }
    pub fn mouse_action(&self, event: MouseEvent) -> Option<WorkbenchAction> {
        if !matches!(event.kind, MouseEventKind::Down(_)) { return None; }
        self.hits.iter().find(|(r,_)|event.column >= r.x && event.column < r.right()
          && event.row >= r.y && event.row < r.bottom()).map(|(_,action)|*action)
    }
    pub fn key_action(key: KeyCode) -> Option<WorkbenchAction> {
        match key {
            KeyCode::Char('1') => Some(WorkbenchAction::Build),
            KeyCode::Char('2') => Some(WorkbenchAction::Run),
            KeyCode::Char('3') => Some(WorkbenchAction::Configure),
            KeyCode::Char('4') => Some(WorkbenchAction::Test),
            KeyCode::Char('5') => Some(WorkbenchAction::Refresh),
            KeyCode::Char('6') => Some(WorkbenchAction::Browser),
            KeyCode::Char('7') => Some(WorkbenchAction::Phone),
            KeyCode::Char('8') => Some(WorkbenchAction::Android),
            _ => None,
        }
    }
}

/// Short-lived JSON bridge; never runs project code by default. The host should
/// run the IO off the terminal render thread. Do not construct ad-hoc shell strings.
pub struct DlcBridge { pub python: PathBuf, pub script: PathBuf, pub workspace: PathBuf }
impl DlcBridge {
    pub fn new(python: impl AsRef<Path>, script: impl AsRef<Path>, workspace: impl AsRef<Path>) -> Self {
        Self {python: python.as_ref().into(), script: script.as_ref().into(),workspace:workspace.as_ref().into()}
    }
    pub fn request(&self, command: &str, args: &[&str]) -> io::Result<serde_json::Value> {
        if !["scan","inspect","plan","config","doctor","runtime"].contains(&command) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied,"Only read-only bridge operations allowed"));
        }
        let mut child = Command::new(&self.python);
        child.arg(&self.script).arg("--root").arg(&self.workspace).arg(command).args(args);
        let output = child.output()?;
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
           .map_err(|e|io::Error::new(io::ErrorKind::InvalidData,e))?;
        if !output.status.success() {
           return Err(io::Error::new(io::ErrorKind::Other,value.to_string()));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn all_toolbar_shortcuts() {
      for digit in '1'..='8' { assert!(WorkbenchToolbar::key_action(KeyCode::Char(digit)).is_some()); }
    }
}
