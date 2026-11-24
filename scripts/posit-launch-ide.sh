#!/usr/bin/env bash
# Launch Posit Edit with vendored terminal-browser / terminal-phone on PATH.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export IDE_TERMINAL_BROWSER="$ROOT/vendor/bin/terminal-browser"
export IDE_TERMINAL_PHONE="$ROOT/vendor/bin/terminal-phone"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export PATH="$ROOT/vendor/bin:$HOME/.local/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
# Prefer a fixture when present so digit keys exercise runtimes safely.
TARGET="${1:-/tmp/posit-debug-demo/src/main.rs}"
if [[ ! -e "$TARGET" ]]; then
  TARGET="$ROOT"
fi
cd "$ROOT"
exec "$ROOT/target/debug/ide" "$TARGET"
