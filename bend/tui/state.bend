# Pure TUI pointer-gesture state machine. Additive; does not touch accepted LAWS.bend.
# Invariant: a non-Idle gesture stays sticky until release — scroll never becomes resize.
import Base

type Gesture is Data:
  Idle{}
  Scrollbar{}
  Minimap{}
  BufferStrip{}
  TileResize{}
  SidebarResize{}
  TerminalHeight{}
  TerminalSide{}
  CanvasResize{}

# Arm only from Idle; otherwise keep the armed gesture (sticky).
def arm(g: Gesture, next: Gesture) -> Gesture:
  match g:
    case Idle{}:
      next
    case _:
      g

def release(_g: Gesture) -> Gesture:
  Idle{}

def is_doc_scroll(g: Gesture) -> Bool:
  match g:
    case Scrollbar{}:
      True{}
    case Minimap{}:
      True{}
    case _:
      False{}
