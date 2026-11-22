# Additive scrollbar laws. Owner acceptance required before root merge.
import Base
import ./state.bend as Scroll

# LAW: when offset is already in range, clamp is identity.
law clamp_identity_in_range:
  {Scroll.clamp_offset(10n, 100n, 20n) == 10n : Nat}

# LAW: offset past the end clamps to total-visible.
law clamp_caps_at_max:
  {Scroll.clamp_offset(90n, 100n, 20n) == 80n : Nat}

# LAW: max_offset is saturating subtraction.
law max_offset_saturates:
  {Scroll.max_offset(10n, 20n) == 0n : Nat}

# LAW: top-of-track click maps to offset 0.
law track_top_is_zero:
  {Scroll.offset_for_track(0n, 40n, 80n) == 0n : Nat}

# LAW: bottom-of-track click maps to max_offset.
law track_bottom_is_max:
  {Scroll.offset_for_track(39n, 40n, 80n) == 80n : Nat}
