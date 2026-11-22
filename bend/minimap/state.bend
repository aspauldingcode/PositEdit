# Pure VS Code–proportional minimap layout. Additive; not root LAWS.
# Mirrors crates/ide-tui/src/minimap_gfx.rs (scale=1 → 8 px/line, sliding window).
import Base

def px_per_line(+scale: Nat) -> Nat:
  Nat.mul(8n, Nat.max(scale, 1n))

def unpack_quot_max1(pair: Nat & Nat) -> Nat:
  match pair:
    case (q, _r):
      Nat.max(q, 1n)

def unpack_quot(pair: Nat & Nat) -> Nat:
  match pair:
    case (q, _):
      q

def shown_lines(+img_h: Nat, +scale: Nat) -> Nat:
  unpack_quot_max1(Nat.divmod(img_h, px_per_line(scale)))

def half_of(+n: Nat) -> Nat:
  unpack_quot(Nat.divmod(n, 2n))

def start_when_overflow(
  +line_count: Nat,
  +scroll: Nat,
  +visible: Nat,
  +shown: Nat
) -> Nat:
  max_start = Nat.sub(line_count, shown)
  slack = Nat.sub(shown, visible)
  ideal = Nat.sub(scroll, half_of(slack))
  Nat.min(ideal, max_start)

def start_by_fit(
  +fits: Bool,
  +line_count: Nat,
  +scroll: Nat,
  +visible: Nat,
  +shown: Nat
) -> Nat:
  match fits:
    case True{}:
      0n
    case False{}:
      start_when_overflow(line_count, scroll, visible, shown)

# First source line painted in the minimap window.
def start_line(+line_count: Nat, +scroll: Nat, +visible: Nat, +shown: Nat) -> Nat:
  start_by_fit(Nat.is_le(line_count, shown), line_count, scroll, visible, shown)

def unpack_add_start(pair: Nat & Nat, +start: Nat) -> Nat:
  match pair:
    case (idx, _):
      Nat.add(start, idx)

# Map a Y pixel inside the image to a source line (window-relative).
def line_at_px(+start: Nat, +px_per: Nat, +y: Nat) -> Nat:
  unpack_add_start(Nat.divmod(y, Nat.max(px_per, 1n)), start)

# Convenience: layout start for scale-1 proportional mode.
def layout_start(+line_count: Nat, +scroll: Nat, +visible: Nat, +img_h: Nat) -> Nat:
  start_line(line_count, scroll, visible, shown_lines(img_h, 1n))
