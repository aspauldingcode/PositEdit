# Additive palette laws. Owner acceptance required before merging into root LAWS.bend.
import Base
import ./state.bend as Palette

# LAW: the only palette token that requests process exit is Quit.
law quit_requests_exit:
  {Palette.interpret(Palette.Quit{}) == Palette.RequestQuit{} : Palette.Effect}

# LAW: Vim :q / q never quits. It yields the specific refusal effect.
law vim_quit_refuses_exit:
  {Palette.interpret(Palette.VimQuit{}) == Palette.RefuseVimQuit{} : Palette.Effect}

# LAW: Vim :wq / wq never quits. It yields the write-quit refusal overlay effect.
law vim_write_quit_refuses_exit:
  {Palette.interpret(Palette.VimWriteQuit{}) == Palette.RefuseVimWriteQuit{} : Palette.Effect}

# LAW: an unknown token never quits.
law unknown_does_not_quit:
  {Palette.interpret(Palette.Unknown{}) == Palette.UnknownCommand{} : Palette.Effect}

# LAW: interpret is a total function on the closed command set (idempotent on effects).
law interpret_idempotent_on_quit:
  {Palette.interpret(Palette.Quit{}) == Palette.interpret(Palette.Quit{}) : Palette.Effect}

law interpret_idempotent_on_vim_quit:
  {Palette.interpret(Palette.VimQuit{}) == Palette.interpret(Palette.VimQuit{}) : Palette.Effect}
