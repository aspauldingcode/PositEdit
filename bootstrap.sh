#!/bin/sh
# Mac/Nix-first bootstrap; does not install OS-level tools or bypass permissions.
set -eu
cd "$(dirname "$0")"
mode=${1:---check}
case "$mode" in --check|--setup|--run) ;; *) echo "Usage: $0 --check|--setup|--run" >&2;exit 2;; esac
command -v python3 >/dev/null 2>&1 || { echo 'BLOCKED: python3 required'; exit 1; }
python3 scripts/preflight.py
if [ "$mode" = "--check" ]; then
  python3 scripts/harness.py init
  python3 scripts/harness.py status
  exit 0
fi
python3 scripts/harness.py init
# Initialize only this project's repository. No remote is added and no commit is made.
if [ ! -e .git ] && command -v git >/dev/null 2>&1; then git init -q; fi
if command -v nix >/dev/null 2>&1; then
  nix --extra-experimental-features 'nix-command flakes' develop -c python3 scripts/configure_mcp.py || python3 scripts/configure_mcp.py
else
  python3 scripts/configure_mcp.py
fi
# A new source archive may not yet contain Cargo.lock. Generate within an existing
# local toolchain, or inside a Nix devShell when Nix is already installed.
if [ ! -f Cargo.lock ]; then
  if command -v cargo >/dev/null 2>&1; then
    cargo generate-lockfile || echo 'BLOCKED: Cargo.lock generation failed' >&2
  elif command -v nix >/dev/null 2>&1; then
    nix --extra-experimental-features 'nix-command flakes' develop -c cargo generate-lockfile || echo 'BLOCKED: Nix devShell lockfile generation failed' >&2
  else
    echo 'BLOCKED: Cargo.lock requires cargo or Nix devShell' >&2
  fi
fi
if [ "$mode" = "--setup" ]; then
  python3 scripts/harness.py status
  exit 0
fi
# Use the existing agent authentication and sandbox; no automatic login or privilege escalations.
if command -v nix >/dev/null 2>&1; then
  exec nix --extra-experimental-features 'nix-command flakes' develop -c python3 scripts/harness.py run --max-tasks "${IDE_MAX_TASKS:-60}"
fi
python3 scripts/harness.py run --max-tasks "${IDE_MAX_TASKS:-60}"
