//! Toolbar and trust decision for the project workbench.
//! The host IDE owns the terminal. This crate does not run project code.
use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crossterm::event::{KeyCode, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkbenchAction {
    Build,
    Run,
    Configure,
    Test,
    Refresh,
    Browser,
    Phone,
    Android,
    Debug,
}

const BUTTONS: [(&str, WorkbenchAction); 9] = [
    (" Build ", WorkbenchAction::Build),
    (" Run ", WorkbenchAction::Run),
    (" Configure ", WorkbenchAction::Configure),
    (" Test ", WorkbenchAction::Test),
    (" Refresh ", WorkbenchAction::Refresh),
    (" Browser ", WorkbenchAction::Browser),
    (" Phone ", WorkbenchAction::Phone),
    (" Android ", WorkbenchAction::Android),
    (" Debug ", WorkbenchAction::Debug),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    Untrusted,
    Trusted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustAction {
    Preview,
    Execute,
}

/// Same decision as `bend/dlc/state.bend`. Untrusted work stays a preview.
pub fn authorize(trust: Trust) -> TrustAction {
    match trust {
        Trust::Untrusted => TrustAction::Preview,
        Trust::Trusted => TrustAction::Execute,
    }
}

pub fn toolbar_layout(area: Rect) -> Vec<(Rect, WorkbenchAction)> {
    let mut hits = Vec::new();
    if area.height == 0 || area.width == 0 {
        return hits;
    }
    let mut x = area.x;
    let mut y = area.y;
    for (label, action) in BUTTONS {
        let width = label.len() as u16 + 2;
        if x > area.x && x.saturating_add(width) > area.right() {
            x = area.x;
            y = y.saturating_add(1);
        }
        if y >= area.bottom() || width > area.width {
            break;
        }
        let cell = Rect::new(x, y, width.min(area.right().saturating_sub(x)), 1);
        hits.push((cell, action));
        x = x.saturating_add(width.saturating_add(1));
    }
    hits
}

pub fn toolbar_hit(area: Rect, column: u16, row: u16) -> Option<WorkbenchAction> {
    toolbar_layout(area).into_iter().find_map(|(rect, action)| {
        (column >= rect.x && column < rect.right() && row >= rect.y && row < rect.bottom())
            .then_some(action)
    })
}

#[derive(Default)]
pub struct WorkbenchToolbar {
    hits: Vec<(Rect, WorkbenchAction)>,
}

impl WorkbenchToolbar {
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        self.hits = toolbar_layout(area);
        for (cell, action) in &self.hits {
            let label = BUTTONS
                .iter()
                .find(|(_, button)| button == action)
                .map(|(label, _)| *label)
                .unwrap_or(" ");
            frame.render_widget(
                Paragraph::new(label)
                    .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
                    .style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                *cell,
            );
        }
    }

    pub fn mouse_action(&self, event: MouseEvent) -> Option<WorkbenchAction> {
        if !matches!(event.kind, MouseEventKind::Down(_)) {
            return None;
        }
        self.hits
            .iter()
            .find(|(rect, _)| {
                event.column >= rect.x
                    && event.column < rect.right()
                    && event.row >= rect.y
                    && event.row < rect.bottom()
            })
            .map(|(_, action)| *action)
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
            KeyCode::Char('9') => Some(WorkbenchAction::Debug),
            _ => None,
        }
    }
}

/// Read-only bridge. Execution stays in the host, behind [`authorize`].
pub struct DlcBridge {
    pub python: PathBuf,
    pub script: PathBuf,
    pub workspace: PathBuf,
}

impl DlcBridge {
    pub fn new(
        python: impl AsRef<Path>,
        script: impl AsRef<Path>,
        workspace: impl AsRef<Path>,
    ) -> Self {
        Self {
            python: python.as_ref().into(),
            script: script.as_ref().into(),
            workspace: workspace.as_ref().into(),
        }
    }

    pub fn request(&self, command: &str, args: &[&str]) -> io::Result<serde_json::Value> {
        if !["scan", "inspect", "plan", "config", "doctor", "runtime"].contains(&command) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Only read-only bridge operations allowed",
            ));
        }
        let mut child = Command::new(&self.python);
        child
            .arg(&self.script)
            .arg("--root")
            .arg(&self.workspace)
            .arg(command)
            .args(args);
        let output = child.output()?;
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if !output.status.success() {
            return Err(io::Error::other(value.to_string()));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_cover_every_toolbar_button() {
        for digit in '1'..='9' {
            assert!(WorkbenchToolbar::key_action(KeyCode::Char(digit)).is_some());
        }
    }

    #[test]
    fn untrusted_work_stays_a_preview() {
        assert_eq!(authorize(Trust::Untrusted), TrustAction::Preview);
        assert_eq!(authorize(Trust::Trusted), TrustAction::Execute);
    }
}
