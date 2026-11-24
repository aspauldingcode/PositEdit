#!/usr/bin/env bash
# Remove Terminal/Ghostty/etc from Cursor Computer Use BLOCKED_APPS.
# Nix store Cursor is immutable; this builds ~/Applications/Cursor-GhosttyCU.app
set -euo pipefail
SRC="${1:-}"
if [[ -z "$SRC" ]]; then
  SRC=$(ps -ax -o args= | rg -o '/nix/store/[^ ]+/Cursor\.app' | head -1 || true)
fi
if [[ -z "$SRC" || ! -d "$SRC" ]]; then
  echo "usage: $0 /path/to/Cursor.app" >&2
  exit 2
fi
DEST="${HOME}/Applications/Cursor-GhosttyCU.app"
mkdir -p "${HOME}/Applications"
rm -rf "$DEST"
ditto "$SRC" "$DEST"
chmod -R u+w "$DEST"
python3 - "$DEST" <<'PY'
import re, sys
from pathlib import Path
root = Path(sys.argv[1])
files = [
 root/"Contents/Resources/app/extensions/cursor-agent-host/dist/main.js",
 root/"Contents/Resources/app/extensions/cursor-agent-exec/dist/main.js",
 root/"Contents/Resources/app/extensions/cursor-agent-host/dist/agent-host-daemon/dist/bin/daemon.cjs",
]
bundles = [
  "com.apple.terminal","com.googlecode.iterm2","dev.warp.warp-stable",
  "dev.warp.warp-preview","com.mitchellh.ghostty","org.alacritty",
  "net.kovidgoyal.kitty","com.github.wez.wezterm","co.zeit.hyper","org.tabby",
]
obj_re = re.compile(
  r'\{bundleId:"(?:' + '|'.join(map(re.escape, bundles)) + r')",name:"[^"]*"(?:,aliases:\[[^\]]*\])?\},?'
)
for f in files:
  data = f.read_text("utf-8")
  new = obj_re.sub("", data)
  new = re.sub(r",\s*,", ",", new)
  new = re.sub(r",\s*]", "]", new)
  if "com.mitchellh.ghostty" in new:
    raise SystemExit(f"ghostty still present in {f}")
  f.write_text(new)
  print("patched", f)
print("Launch:", root)
print("open -na", root)
PY
echo "Done. Quit running Cursor, then: open -na $DEST"
