# Pure command-palette compiler. Additive; does not touch accepted LAWS.bend.
# Strings are normalized in Rust; this model reasons over classified tokens only.
import Base

# Closed set of palette tokens after normalization.
type Command is Data:
  Quit{}
  VimQuit{}
  VimWriteQuit{}
  Split{}
  Undo{}
  Format{}
  Help{}
  Files{}
  Find{}
  Grep{}
  New{}
  Save{}
  Unknown{}

# Effects the host may apply. Vim tokens never become RequestQuit.
type Effect is Data:
  RequestQuit{}
  RefuseVimQuit{}
  RefuseVimWriteQuit{}
  ToggleSplit{}
  UndoEdit{}
  FormatBuffer{}
  ShowHelp{}
  OpenFiles{}
  FindFiles{}
  GrepWorkspace{}
  NewBuffer{}
  SaveBuffer{}
  UnknownCommand{}

# The palette compiler: every token has exactly one effect.
def interpret(command: Command) -> Effect:
  match command:
    case Quit{}: RequestQuit{}
    case VimQuit{}: RefuseVimQuit{}
    case VimWriteQuit{}: RefuseVimWriteQuit{}
    case Split{}: ToggleSplit{}
    case Undo{}: UndoEdit{}
    case Format{}: FormatBuffer{}
    case Help{}: ShowHelp{}
    case Files{}: OpenFiles{}
    case Find{}: FindFiles{}
    case Grep{}: GrepWorkspace{}
    case New{}: NewBuffer{}
    case Save{}: SaveBuffer{}
    case Unknown{}: UnknownCommand{}
