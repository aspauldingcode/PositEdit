# ACCEPTED OWNER-CONTROLLED LAWS. AGENTS MUST NOT MODIFY.
# Pure graph job cancellation model. The implementation is separately tested in Rust/Python.
import Base
import ./bend/model.bend as Job

# LAW: cancelling twice yields the same result as cancelling once.
law cancel_idempotent:
  for state: Job.State
  {Job.cancel(Job.cancel(state)) == Job.cancel(state) : Job.State}

# LAW: queued jobs cancel.
law cancels_queued:
  {Job.cancel(Job.Queued{}) == Job.Cancelled{} : Job.State}

# LAW: running jobs cancel.
law cancels_running:
  {Job.cancel(Job.Running{}) == Job.Cancelled{} : Job.State}

# LAW: finished jobs remain finished.
law preserves_finished:
  {Job.cancel(Job.Finished{}) == Job.Finished{} : Job.State}
