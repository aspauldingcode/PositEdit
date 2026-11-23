# Pure job cancellation state model for the graph-task supervisor.
# Proved by LAWS.bend and PROOF.bend; Rust conformance must be tested separately.
#
# Additive Bend modules (do not edit accepted root LAWS.bend to absorb them
# without owner review — see engineering/proposals/bend-core-invariants.md):
#   bend/doc/       document revision gate
#   bend/minimap/   VS Code proportional minimap layout
#   bend/scroll/    scrollbar offset clamp / track map
#   bend/palette/   command-palette effects (incl. Vim refusal)
#   bend/dlc/       project-plan trust authorize
import Base

type State is Data:
  Queued{}
  Running{}
  Cancelled{}
  Finished{}

def cancel(state: State) -> State:
  match state:
    case Queued{}: Cancelled{}
    case Running{}: Cancelled{}
    case Cancelled{}: Cancelled{}
    case Finished{}: Finished{}
