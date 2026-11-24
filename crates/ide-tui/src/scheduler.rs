//! UI-thread scheduling: drain, coalesce, frame pace.
//!
//! Posit Edit's interactive loop is intentionally **synchronous on one UI thread**.
//! Crossterm + ratatui own the tty; they are not safe to drive from a worker pool.
//! Responsiveness comes from:
//! 1. never doing O(document) work on the UI thread,
//! 2. draining the whole input queue before one present,
//! 3. coalescing scroll storms so quit/keys are not stuck behind wheel events,
//! 4. offloading IO (Canvas capture process, device-action thread, future highlight
//!    workers) — **not** by making `run()` itself `async`.
//!
//! Tokio stays available for IO-bound helpers. Putting the editor loop on
//! `tokio::main` would not fix quit-during-scroll; only input batching does.

use crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::time::{Duration, Instant};

/// One frame's worth of user input after draining the tty queue.
#[derive(Clone, Debug, Default)]
pub struct InputBatch {
    /// Ordered keys with press / repeat / release kind (Kitty progressive enhancement).
    pub keys: Vec<(KeyCode, KeyModifiers, KeyEventKind)>,
    pub toggle_tab: bool,
    /// Net vertical wheel ticks (negative = up / content moves down).
    pub scroll_v: i32,
    /// Net horizontal wheel ticks.
    pub scroll_h: i32,
    pub scroll_column: u16,
    pub scroll_row: u16,
    pub moves: Option<(u16, u16)>,
    pub pointers: Vec<PointerOp>,
    pub resized: bool,
    /// Terminal regained focus — re-probe OSC palette (theme may have changed).
    pub focus_gained: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointerOp {
    Down { column: u16, row: u16 },
    Drag { column: u16, row: u16 },
    Up { column: u16, row: u16 },
    Right { column: u16, row: u16 },
}

impl InputBatch {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
            && !self.toggle_tab
            && self.scroll_v == 0
            && self.scroll_h == 0
            && self.moves.is_none()
            && self.pointers.is_empty()
            && !self.resized
            && !self.focus_gained
    }

    pub fn wants_present(&self) -> bool {
        !self.is_empty()
    }
}

/// Fold a raw event stream into one batch. Keys keep order; wheel events sum;
/// only the last mouse-move is kept; button gestures stay ordered.
pub fn coalesce(events: impl IntoIterator<Item = Event>) -> InputBatch {
    let mut batch = InputBatch::default();
    for event in events {
        match event {
            Event::Key(key)
                if matches!(
                    key.kind,
                    KeyEventKind::Press | KeyEventKind::Repeat | KeyEventKind::Release
                ) =>
            {
                if key.kind != KeyEventKind::Release
                    && key.code == KeyCode::Char('t')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.modifiers.contains(KeyModifiers::ALT)
                {
                    batch.toggle_tab = true;
                } else {
                    batch.keys.push((key.code, key.modifiers, key.kind));
                }
            }
            Event::Mouse(mouse) => merge_mouse(&mut batch, mouse),
            Event::Resize(_, _) => batch.resized = true,
            Event::FocusGained => batch.focus_gained = true,
            _ => {}
        }
    }
    batch
}

fn merge_mouse(batch: &mut InputBatch, mouse: MouseEvent) {
    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
    match mouse.kind {
        MouseEventKind::ScrollUp if shift => {
            batch.scroll_h -= 1;
            batch.scroll_column = mouse.column;
            batch.scroll_row = mouse.row;
        }
        MouseEventKind::ScrollDown if shift => {
            batch.scroll_h += 1;
            batch.scroll_column = mouse.column;
            batch.scroll_row = mouse.row;
        }
        MouseEventKind::ScrollLeft => {
            batch.scroll_h -= 1;
            batch.scroll_column = mouse.column;
            batch.scroll_row = mouse.row;
        }
        MouseEventKind::ScrollRight => {
            batch.scroll_h += 1;
            batch.scroll_column = mouse.column;
            batch.scroll_row = mouse.row;
        }
        MouseEventKind::ScrollUp => {
            batch.scroll_v -= 1;
            batch.scroll_column = mouse.column;
            batch.scroll_row = mouse.row;
        }
        MouseEventKind::ScrollDown => {
            batch.scroll_v += 1;
            batch.scroll_column = mouse.column;
            batch.scroll_row = mouse.row;
        }
        MouseEventKind::Moved => {
            batch.moves = Some((mouse.column, mouse.row));
        }
        MouseEventKind::Down(MouseButton::Left) => {
            batch.pointers.push(PointerOp::Down {
                column: mouse.column,
                row: mouse.row,
            });
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            // Keep only the latest drag in a run so resize/selection stay light.
            match batch.pointers.last() {
                Some(PointerOp::Drag { .. }) => {
                    *batch.pointers.last_mut().unwrap() = PointerOp::Drag {
                        column: mouse.column,
                        row: mouse.row,
                    };
                }
                _ => batch.pointers.push(PointerOp::Drag {
                    column: mouse.column,
                    row: mouse.row,
                }),
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            batch.pointers.push(PointerOp::Up {
                column: mouse.column,
                row: mouse.row,
            });
        }
        MouseEventKind::Down(MouseButton::Right) => {
            batch.pointers.push(PointerOp::Right {
                column: mouse.column,
                row: mouse.row,
            });
        }
        _ => {}
    }
}

/// True when enough time has passed since the last present (≈60 Hz).
#[derive(Clone, Debug)]
pub struct FrameClock {
    last_present: Instant,
    min_frame: Duration,
}

impl FrameClock {
    pub fn new(max_fps: u32) -> Self {
        // AFK: allow 24 Hz ceiling (was hard-clamped to 30).
        let fps = max_fps.clamp(24, 120);
        Self {
            last_present: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            min_frame: Duration::from_nanos(1_000_000_000 / u64::from(fps)),
        }
    }

    pub fn due(&self) -> bool {
        self.last_present.elapsed() >= self.min_frame
    }

    pub fn mark_presented(&mut self) {
        self.last_present = Instant::now();
    }

    pub fn wait_hint(&self) -> Duration {
        self.min_frame
            .saturating_sub(self.last_present.elapsed())
            .min(Duration::from_millis(16))
    }
}

/// Where work belongs (guidance for future workers — enforced by design, not a runtime).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WorkClass {
    /// Must run on the UI thread: input apply, layout, ratatui draw, Kitty escapes.
    UiSync,
    /// Background OS thread / process: screenshots, device taps, PTY read already.
    WorkerThread,
    /// Tokio tasks: multi-IO fan-out (grep many files, MCP). Not the editor frame loop.
    AsyncIo,
    /// Terminal GPU / shm present — owned by Ghostty/Kitty after we emit escapes.
    TerminalGpu,
}

#[allow(dead_code)]
pub fn classify(label: &str) -> WorkClass {
    match label {
        "present" | "draw" | "input" | "layout" | "kitty" => WorkClass::UiSync,
        "canvas-capture" | "device-action" | "pty-read" | "highlight-worker" => {
            WorkClass::WorkerThread
        }
        "grep" | "mcp" | "format-cli" => WorkClass::AsyncIo,
        "shm-upload" | "iosurface" => WorkClass::TerminalGpu,
        _ => WorkClass::UiSync,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn key(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn scroll_down(column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn scroll_storm_coalesces_and_keeps_quit_key() {
        let mut events = Vec::new();
        for _ in 0..40 {
            events.push(scroll_down(10, 10));
        }
        events.push(key('q'));
        let batch = coalesce(events);
        assert_eq!(batch.scroll_v, 40);
        assert_eq!(batch.keys.len(), 1);
        assert_eq!(batch.keys[0].0, KeyCode::Char('q'));
        assert_eq!(batch.keys[0].2, KeyEventKind::Press);
    }

    #[test]
    fn keys_are_not_reordered() {
        let batch = coalesce([key('a'), key('b'), key('c')]);
        assert_eq!(
            batch
                .keys
                .iter()
                .map(|(c, _, _)| *c)
                .collect::<Vec<_>>(),
            vec![
                KeyCode::Char('a'),
                KeyCode::Char('b'),
                KeyCode::Char('c')
            ]
        );
    }

    #[test]
    fn frame_clock_afk_ceiling_is_24hz() {
        let clock = FrameClock::new(24);
        assert_eq!(clock.min_frame, Duration::from_nanos(1_000_000_000 / 24));
        // Clamp floor is 24 — requesting lower still yields 24 Hz.
        let floor = FrameClock::new(15);
        assert_eq!(floor.min_frame, Duration::from_nanos(1_000_000_000 / 24));
    }

    #[test]
    fn key_release_is_kept_for_hold_gestures() {
        let batch = coalesce([
            Event::Key(KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: crossterm::event::KeyEventState::empty(),
            }),
            Event::Key(KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Release,
                state: crossterm::event::KeyEventState::empty(),
            }),
        ]);
        assert_eq!(batch.keys.len(), 2);
        assert_eq!(batch.keys[0].2, KeyEventKind::Press);
        assert_eq!(batch.keys[1].2, KeyEventKind::Release);
    }

    #[test]
    fn drag_run_keeps_latest_only() {
        let batch = coalesce([
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 1,
                row: 1,
                modifiers: KeyModifiers::NONE,
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 2,
                row: 2,
                modifiers: KeyModifiers::NONE,
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 9,
                row: 9,
                modifiers: KeyModifiers::NONE,
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 9,
                row: 9,
                modifiers: KeyModifiers::NONE,
            }),
        ]);
        assert_eq!(
            batch.pointers,
            vec![
                PointerOp::Down { column: 1, row: 1 },
                PointerOp::Drag { column: 9, row: 9 },
                PointerOp::Up { column: 9, row: 9 },
            ]
        );
    }

    #[test]
    fn work_classes_separate_ui_from_async() {
        assert_eq!(classify("present"), WorkClass::UiSync);
        assert_eq!(classify("grep"), WorkClass::AsyncIo);
        assert_eq!(classify("canvas-capture"), WorkClass::WorkerThread);
        assert_ne!(classify("present"), WorkClass::AsyncIo);
    }
}
