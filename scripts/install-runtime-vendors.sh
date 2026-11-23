#!/usr/bin/env bash
# Install real terminal-browser + terminal-phone into vendor/dist and link vendor/bin.
# Source forks live under vendor/terminal-{browser,phone} (upstream MIT).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/vendor/bin"
DIST="$ROOT/vendor/dist"
mkdir -p "$BIN" "$DIST"

install_phone() {
  local ver=0.4.0
  local base="https://github.com/govijr/terminal-phone/releases/download/v${ver}"
  local tmp runtime
  tmp="$(mktemp -d)"
  echo "Installing terminal-phone ${ver}…"
  curl -fsSL "$base/terminal-phone-${ver}-macos-arm64.tar.gz" -o "$tmp/phone.tgz"
  curl -fsSL "$base/terminal-phone-${ver}-macos-arm64.tar.gz.sha256" -o "$tmp/phone.sha256"
  (cd "$tmp" && shasum -a 256 -c phone.sha256)
  tar -xzf "$tmp/phone.tgz" -C "$tmp"
  mkdir -p "$DIST/terminal-phone"
  "$tmp/terminal-phone/terminal-phone" self install --prefix "$DIST/terminal-phone" --no-path
  # Prefer a locally built fork binary when present (Xcode 26/27 scale patch).
  if [[ -x "$ROOT/vendor/terminal-phone/dist/terminal-phone" ]]; then
    runtime="$(find "$DIST/terminal-phone/lib/terminal-phone" -type f -name terminal-phone | head -1)"
    if [[ -n "$runtime" ]]; then
      cp -f "$ROOT/vendor/terminal-phone/dist/terminal-phone" "$runtime"
      codesign --force --sign - "$runtime" >/dev/null 2>&1 || true
      echo "Installed patched fork binary over $runtime"
    fi
  fi
  ln -sfn ../dist/terminal-phone/bin/terminal-phone "$BIN/terminal-phone"
  rm -rf "$tmp"
  "$BIN/terminal-phone" --version
}

install_browser() {
  local ver=v0.15.9
  local url="https://terminal-browser.sh/install/dl/stable/${ver}/terminal-browser-darwin-arm64.tar.gz"
  local sha="fa0b29716b04c7ad47d0825d4885443f273f6dcd2bbb6bebd88f2a9525133771"
  local tmp
  tmp="$(mktemp -d)"
  echo "Installing terminal-browser ${ver}…"
  curl -fL --retry 3 --progress-bar "$url" -o "$tmp/tb.tar.gz"
  echo "$sha  $tmp/tb.tar.gz" | shasum -a 256 -c -
  rm -rf "$DIST/terminal-browser"
  mkdir -p "$DIST/terminal-browser"
  tar -xzf "$tmp/tb.tar.gz" -C "$DIST/terminal-browser" --strip-components 1
  ln -sfn ../dist/terminal-browser/bin/terminal-browser "$BIN/terminal-browser"
  rm -rf "$tmp"
  "$BIN/terminal-browser" --version
}

clone_forks() {
  mkdir -p "$ROOT/vendor"
  if [[ ! -d "$ROOT/vendor/terminal-browser/.git" && ! -f "$ROOT/vendor/terminal-browser/package.json" ]]; then
    git clone --depth 1 https://github.com/zenbu-labs/terminal-browser.git "$ROOT/vendor/terminal-browser"
  fi
  if [[ ! -d "$ROOT/vendor/terminal-phone/.git" && ! -f "$ROOT/vendor/terminal-phone/Package.swift" ]]; then
    git clone --depth 1 https://github.com/govijr/terminal-phone.git "$ROOT/vendor/terminal-phone"
  fi
}

case "${1:-all}" in
  forks) clone_forks ;;
  browser) install_browser ;;
  phone) install_phone ;;
  all)
    clone_forks
    if [[ "$(uname -s)-$(uname -m)" == "Darwin-arm64" ]]; then
      install_browser
      install_phone
    else
      echo "Binary install currently scripted for Darwin arm64; clone forks only." >&2
      exit 1
    fi
    ;;
  *)
    echo "usage: $0 [all|forks|browser|phone]" >&2
    exit 2
    ;;
esac

echo "vendor bins:"
ls -la "$BIN"
