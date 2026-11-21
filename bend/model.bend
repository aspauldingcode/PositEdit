# Pure job cancellation state model for the graph-task supervisor.
# Proved by LAWS.bend and PROOF.bend; Rust conformance must be tested separately.
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
