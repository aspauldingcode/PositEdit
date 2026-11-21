# Additive project-plan laws. These do not replace the accepted root laws.
import Base
import ./state.bend as State

law untrusted_is_preview_only:
  {State.authorize(State.Untrusted{}) == State.Preview{} : State.Action}

law trusted_may_execute:
  {State.authorize(State.Trusted{}) == State.Execute{} : State.Action}
