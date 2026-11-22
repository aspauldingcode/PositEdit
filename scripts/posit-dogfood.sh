#!/bin/zsh
# Launch a release Posit Edit in Ghostty with an isolated instance lock.
# Usage: scripts/posit-dogfood.sh [file]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/posit-edit"
FILE="${1:-$ROOT/README.md}"
LOCK="/tmp/posit-dogfood-$$.lock"
if [[ ! -x "$BIN" ]]; then
  echo "building release posit-edit…" >&2
  (cd "$ROOT" && cargo build --release --bin posit-edit)
fi
unset POSIT_EDIT_SESSION
export POSIT_EDIT_INSTANCE_LOCK="$LOCK"
export TERM_PROGRAM="${TERM_PROGRAM:-ghostty}"
exec "$BIN" "$FILE"
