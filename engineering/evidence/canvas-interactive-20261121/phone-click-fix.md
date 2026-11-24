# Phone Canvas click fix

## Root cause
`terminal-phone action screenshot` returns a PNG already in **display-points** (e.g. 402×874)
with JSON `{ width, height, scale:3, coordinateSpace implied }`.

Canvas was:
1. Preferring pane `geom_pixels` as the action space (wrong for phone), and/or
2. Dividing point-sized PNGs by retina `scale=3`, sending taps at ~1/3 the real location.

## Fix
- Capture meta: `coordinate_space=display-points`, `scale=1.0`, `coord_width/height` from phone JSON.
- Click map: contain-fit screenshot inside pane (letterbox aware), then scale into action coords.
- Phone tap commits on MouseDown (MouseUp often missing); swipe still on drag+up.
- Browser keeps CSS-pixel / viewport mapping via agent-browser mouse ops.

## Verify
- `cargo test -p ide-tui --lib canvas::`
- Live `terminal-phone action posit tap` against mapped Maps coords
