# Pure Flip 3D Tab-hold state machine. Additive; does not touch accepted LAWS.bend.
# Invariant: Tab-armed Open always has tab_held True{}. Release → Closed.
import Base

type Flip is Data:
  Closed{}
  # Tab-armed only — chord fallback is out of this model.
  Open{restore: Nat, selected: Nat, tab_held: Bool}

def open_tab(+focus: Nat) -> Flip:
  Open{focus, focus, True{}}

def unpack_rem(pair: Nat & Nat) -> Nat:
  match pair:
    case (_, r):
      r

def rem_add1(+selected: Nat, +count: Nat) -> Nat:
  unpack_rem(Nat.divmod(Nat.add(selected, 1n), count))

def flip_next(state: Flip, +count: Nat) -> Flip:
  match state:
    case Closed{}:
      Closed{}
    case Open{restore, selected, tab_held}:
      Open{restore, rem_add1(selected, count), tab_held}

def release_result(state: Flip, +current: Nat) -> Nat & Flip:
  match state:
    case Closed{}:
      (current, Closed{})
    case Open{_, selected, _}:
      (selected, Closed{})

def cancel_result(state: Flip, +current: Nat) -> Nat & Flip:
  match state:
    case Closed{}:
      (current, Closed{})
    case Open{restore, _, _}:
      (restore, Closed{})

# True when state satisfies: Closed, or Open with Tab still held.
def hold_invariant(state: Flip) -> Bool:
  match state:
    case Closed{}:
      True{}
    case Open{_, _, tab_held}:
      tab_held
