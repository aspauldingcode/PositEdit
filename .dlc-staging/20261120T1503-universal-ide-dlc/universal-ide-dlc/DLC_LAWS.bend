# Additive governing laws for project/runtime DLC. Owner-controlled.
import Base
import ./engineering/bend/state.bend as State

# Project build code cannot run while untrusted.
law untrusted_is_preview_only:
  {State.authorize(State.Untrusted{}) == State.Preview{} : State.Action}

# Explicitly trusted projects may be submitted for execution.
law trusted_may_execute:
  {State.authorize(State.Trusted{}) == State.Execute{} : State.Action}
