//! Additive bottom-right Tab toggle. No model or other infrastructure is loaded by this crate.
//! Host must wire this into the real editor status area, persistent settings, and a worker.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabStatus {
    Ready,
    LoadingModel,
    LexicalFallback,
    Unavailable,
    Paused,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Toggle,
    OpenSettings,
}

#[derive(Debug, Clone)]
pub struct TabControl {
    pub enabled: bool,
    pub status: TabStatus,
    pub hovered: bool,
    rect: Rect,
}
impl Default for TabControl {
    fn default() -> Self {
        Self {
            enabled: true,
            status: TabStatus::LexicalFallback,
            hovered: false,
            rect: Rect::default(),
        }
    }
}
impl TabControl {
    pub fn render(&mut self, frame: &mut Frame, status_area: Rect) {
        if status_area.height == 0 || status_area.width == 0 {
            self.rect = Rect::default();
            return;
        }
        let text = match (self.enabled, self.status) {
            (false, _) => " Tab OFF ",
            (true, TabStatus::Ready) => " Tab ON ✓ ",
            (true, TabStatus::LoadingModel) => " Tab Loading ",
            (true, TabStatus::LexicalFallback) => " Tab ON · Basic ",
            (true, TabStatus::Unavailable) => " Tab ON · Setup ",
            (true, TabStatus::Paused) => " Tab Paused ",
        };
        let width = (text.chars().count() as u16 + 2).min(status_area.width);
        self.rect = Rect::new(
            status_area.right().saturating_sub(width),
            status_area.y,
            width,
            1,
        );
        let style = if self.hovered {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if self.enabled {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        };
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::new().borders(Borders::LEFT | Borders::RIGHT))
                .style(style),
            self.rect,
        );
    }
    pub fn mouse_action(&self, ev: MouseEvent) -> Option<TabAction> {
        if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
            return None;
        }
        if ev.column >= self.rect.x
            && ev.column < self.rect.right()
            && ev.row >= self.rect.y
            && ev.row < self.rect.bottom()
        {
            if ev.modifiers.contains(KeyModifiers::SHIFT) {
                Some(TabAction::OpenSettings)
            } else {
                Some(TabAction::Toggle)
            }
        } else {
            None
        }
    }
    pub fn keyboard_action(ev: KeyEvent) -> Option<TabAction> {
        if ev.code == KeyCode::Char('t')
            && ev.modifiers.contains(KeyModifiers::CONTROL)
            && ev.modifiers.contains(KeyModifiers::ALT)
        {
            Some(TabAction::Toggle)
        } else {
            None
        }
    }
    pub fn apply(&mut self, action: TabAction) {
        if action == TabAction::Toggle {
            self.enabled = !self.enabled;
        }
    }
    pub fn bounds(&self) -> Rect {
        self.rect
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enabled_by_default() {
        assert!(TabControl::default().enabled);
    }
    #[test]
    fn toggle() {
        let mut c = TabControl::default();
        c.apply(TabAction::Toggle);
        assert!(!c.enabled);
    }
    #[test]
    fn keyboard() {
        assert_eq!(
            TabControl::keyboard_action(KeyEvent::new(
                KeyCode::Char('t'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            )),
            Some(TabAction::Toggle)
        );
    }
}
