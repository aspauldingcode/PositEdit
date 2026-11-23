#!/bin/zsh
# Launch a release Posit Edit in Ghostty with an isolated instance lock.
# Usage: scripts/posit-dogfood.sh [file]
#
# Defaults to a long source file so the focused minimap and scrollbar chrome
# are visible without hunting for overflow. Toggle with :minimap if needed.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/posit-edit"
FILE="${1:-$ROOT/crates/ide-tui/src/lib.rs}"
LOCK="/tmp/posit-dogfood-$$.lock"
if [[ ! -x "$BIN" ]]; then
  echo "building release posit-edit…" >&2
  (cd "$ROOT" && cargo build --release --bin posit-edit)
fi
unset POSIT_EDIT_SESSION
export POSIT_EDIT_INSTANCE_LOCK="$LOCK"
export TERM_PROGRAM="${TERM_PROGRAM:-ghostty}"
# Keep the Kitty strip up during dogfood even if the host reports pressure.
export POSIT_EDIT_MINIMAP_FORCE="${POSIT_EDIT_MINIMAP_FORCE:-1}"
exec "$BIN" "$FILE"
