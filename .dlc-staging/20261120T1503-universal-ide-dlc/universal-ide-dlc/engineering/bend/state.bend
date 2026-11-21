# Isolated additive state model. No modifications to core laws.
import Base

type Trust is Data:
  Untrusted{}
  Trusted{}

type Action is Data:
  Preview{}
  Execute{}

def authorize(state: Trust) -> Action:
  match state:
    case Untrusted{}: Preview{}
    case Trusted{}: Execute{}
