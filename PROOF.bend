# AI-writable mathematical proofs for accepted LAWS.bend.
import Base
import ./bend/model.bend as Job
import ./LAWS.bend as Laws

def Laws.cancel_idempotent(state):
  match state:
    case Job.Queued{}: {==}
    case Job.Running{}: {==}
    case Job.Cancelled{}: {==}
    case Job.Finished{}: {==}

def Laws.cancels_queued():
  {==}

def Laws.cancels_running():
  {==}

def Laws.preserves_finished():
  {==}
