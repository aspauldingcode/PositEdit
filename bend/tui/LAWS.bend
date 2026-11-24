# Additive TUI pointer-gesture laws. Owner acceptance required before root merge.
import Base
import ./state.bend as Tui

# LAW: from Idle, arming Scrollbar yields Scrollbar.
law idle_arms_scrollbar:
  {Tui.arm(Tui.Idle{}, Tui.Scrollbar{}) == Tui.Scrollbar{} : Tui.Gesture}

# LAW: while Scrollbar is armed, arm(TileResize) stays Scrollbar (no steal).
law scroll_gesture_sticky:
  {Tui.arm(Tui.Scrollbar{}, Tui.TileResize{}) == Tui.Scrollbar{} : Tui.Gesture}

# LAW: while Minimap is armed, arm(SidebarResize) stays Minimap.
law minimap_rejects_sidebar:
  {Tui.arm(Tui.Minimap{}, Tui.SidebarResize{}) == Tui.Minimap{} : Tui.Gesture}

# LAW: release always returns Idle.
law release_is_idle:
  {Tui.release(Tui.TileResize{}) == Tui.Idle{} : Tui.Gesture}

# LAW: Scrollbar counts as document scroll.
law scrollbar_is_doc_scroll:
  {Tui.is_doc_scroll(Tui.Scrollbar{}) == True{} : Bool}
