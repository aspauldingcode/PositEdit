//! Exclusive pointer-gesture state machine for the TUI.
//!
//! Once a gesture is armed on mouse-down, drags may only continue that gesture
//! until mouse-up. Scrollbar / minimap scrolling must never morph into pane
//! resize (tile edge, sidebar seam, canvas, terminal). Mirrors `bend/tui`.

/// Which exclusive pointer gesture is armed (at most one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PointerGesture {
    #[default]
    Idle,
    /// Document scroll via editor scrollbar.
    Scrollbar,
    /// Document scroll via code minimap strip.
    Minimap,
    /// Bottom buffer-focus strip.
    BufferStrip,
    /// Niri-style buffer tile edge resize.
    TileResize,
    /// Left/right sidebar width resize.
    SidebarResize,
    /// Terminal panel height.
    TerminalHeight,
    /// Terminal session-list side seam.
    TerminalSide,
    /// Canvas pane width.
    CanvasResize,
}

impl PointerGesture {
    pub fn is_idle(self) -> bool {
        matches!(self, Self::Idle)
    }

    pub fn is_doc_scroll(self) -> bool {
        matches!(self, Self::Scrollbar | Self::Minimap)
    }

    /// Pane / split geometry is moving — Kitty placements must be dropped.
    pub fn is_layout_mutating(self) -> bool {
        matches!(
            self,
            Self::TileResize
                | Self::SidebarResize
                | Self::TerminalHeight
                | Self::TerminalSide
                | Self::CanvasResize
        )
    }

    /// Arm `next` only from Idle — sticky until [`Self::release`].
    pub fn arm(self, next: Self) -> Self {
        if self.is_idle() {
            next
        } else {
            self
        }
    }

    pub fn release(self) -> Self {
        Self::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_from_idle_takes_next() {
        assert_eq!(
            PointerGesture::Idle.arm(PointerGesture::Scrollbar),
            PointerGesture::Scrollbar
        );
    }

    #[test]
    fn arm_while_scrollbar_rejects_tile_resize() {
        // Bend law: scroll_gesture_sticky.
        let g = PointerGesture::Scrollbar.arm(PointerGesture::TileResize);
        assert_eq!(g, PointerGesture::Scrollbar);
        assert!(g.is_doc_scroll());
    }

    #[test]
    fn arm_while_minimap_rejects_sidebar_resize() {
        let g = PointerGesture::Minimap.arm(PointerGesture::SidebarResize);
        assert_eq!(g, PointerGesture::Minimap);
    }

    #[test]
    fn release_returns_idle() {
        assert_eq!(PointerGesture::TileResize.release(), PointerGesture::Idle);
        assert_eq!(PointerGesture::Scrollbar.release(), PointerGesture::Idle);
    }
}
