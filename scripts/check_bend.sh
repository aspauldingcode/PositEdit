#!/bin/sh
# Run every Bend proof gate in this repository (root + additive modules).
# Prefer the flake bend package; fall back to PATH.
set -eu
ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if command -v bend >/dev/null 2>&1; then
  BEND="$(command -v bend)"
elif [ -n "${BEND_BIN:-}" ]; then
  BEND="$BEND_BIN"
else
  BEND="$(nix build "$ROOT#bend" --no-link --print-out-paths)/bin/bend"
fi

fail=0
# Only gates that are real Bend proofs. Stub TODO proofs under extensions/ are
# tracked separately and must not fail the core gate.
for proof in \
  PROOF.bend \
  bend/palette/PROOF.bend \
  bend/dlc/PROOF.bend \
  bend/doc/PROOF.bend \
  bend/minimap/PROOF.bend \
  bend/scroll/PROOF.bend \
  bend/flip/PROOF.bend \
  bend/tui/PROOF.bend
do
  if [ ! -f "$proof" ]; then
    echo "missing proof file: $proof" >&2
    fail=1
    continue
  fi
  echo "== bend $proof --verdict =="
  if ! out="$("$BEND" "$proof" --verdict 2>&1)"; then
    printf '%s\n' "$out"
    fail=1
    continue
  fi
  printf '%s\n' "$out"
  printf '%s\n' "$out" | grep -q "ALL PROOFS CHECK" || fail=1
done

if [ "$fail" -ne 0 ]; then
  echo "Bend proof gate FAILED" >&2
  exit 1
fi
echo "Bend proof gate OK"
