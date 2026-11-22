# Pure scrollbar offset clamp. Additive; does not touch accepted LAWS.bend.
# Mirrors ScrollMetrics max_offset / offset clamp in ide-tui scrollbar.
import Base

def max_offset(+total: Nat, +visible: Nat) -> Nat:
  Nat.sub(total, visible)

def clamp_offset(+offset: Nat, +total: Nat, +visible: Nat) -> Nat:
  Nat.min(offset, max_offset(total, visible))

# Track click → offset (discrete linear map). track>=2, max_offset>0 assumed by caller.
def unpack_track_map(pair: Nat & Nat, +max_off: Nat) -> Nat:
  match pair:
    case (q, _):
      q

def offset_for_track(+y: Nat, +track: Nat, +max_off: Nat) -> Nat:
  # (y * max_off) / (track - 1)
  denom = Nat.sub(track, 1n)
  unpack_track_map(Nat.divmod(Nat.mul(y, max_off), Nat.max(denom, 1n)), max_off)
