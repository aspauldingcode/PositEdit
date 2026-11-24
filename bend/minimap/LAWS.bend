# Additive proportional-minimap laws. Owner acceptance required before root merge.
# Nat sizes stay modest so bend --verdict (BendTT kernel) stays within fuel.
import Base
import ./state.bend as Mini

# LAW: scale 1 uses exactly 8 device pixels per source line.
law scale_one_is_eight_px:
  {Mini.px_per_line(1n) == 8n : Nat}

# LAW: a 160px strip at scale 1 shows 20 source lines.
law one_sixty_px_shows_twenty:
  {Mini.shown_lines(160n, 1n) == 20n : Nat}

# LAW: files that fit the strip start at line 0 (no sliding).
law small_file_starts_at_origin:
  {Mini.start_line(10n, 0n, 5n, 20n) == 0n : Nat}

# LAW: large files use a sliding window that keeps the viewport inside.
# shown=20, slack=10, half=5, start=min(40-5, 80-20)=35.
law large_file_window_contains_scroll:
  {Mini.layout_start(80n, 40n, 10n, 160n) == 35n : Nat}

# LAW: click at the top of the window maps to the window start line.
law click_top_is_window_start:
  {Mini.line_at_px(35n, 8n, 0n) == 35n : Nat}

# LAW: click mapping stays inside the sliding window (not full-file squeeze).
# 35 + 152/8 = 35 + 19 = 54 (< 80, and within window 35..54).
law click_bottom_stays_in_window:
  {Mini.line_at_px(35n, 8n, 152n) == 54n : Nat}
