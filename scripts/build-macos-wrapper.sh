#!/usr/bin/env bash
# Build a neovim-style PositEdit.app that launches the posit-edit CLI in a terminal.
# macOS only. Icon is the Icon Composer Liquid Glass package (+ .icns fallback).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-"$ROOT/dist/PositEdit.app"}"
ICON_ICNS="$ROOT/assets/icons/PositEdit.icns"
ICON_PACKAGE="$ROOT/assets/icons/PositEdit.icon"
SCRIPT="$ROOT/packaging/macos/PositEdit.applescript"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "build-macos-wrapper.sh is macOS-only" >&2
  exit 2
fi
if [[ ! -f "$SCRIPT" ]]; then
  echo "missing $SCRIPT" >&2
  exit 1
fi
if [[ ! -f "$ICON_ICNS" ]]; then
  echo "missing $ICON_ICNS (run icon packaging first)" >&2
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$(dirname "$OUT")"
osacompile -o "$OUT" "$SCRIPT"

# Bundle identity + Liquid Glass / icns icon.
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier io.github.aspauldingcode.PositEdit" "$OUT/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string io.github.aspauldingcode.PositEdit" "$OUT/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleName Posit Edit" "$OUT/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Add :CFBundleName string Posit Edit" "$OUT/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName Posit Edit" "$OUT/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Add :CFBundleDisplayName string Posit Edit" "$OUT/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIconFile PositEdit" "$OUT/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string PositEdit" "$OUT/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion 0.1.0" "$OUT/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string 0.1.0" "$OUT/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString 0.1.0" "$OUT/Contents/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string 0.1.0" "$OUT/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes array" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0 dict" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeName string Text Document" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeRole string Editor" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:LSItemContentTypes array" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:LSItemContentTypes:0 string public.text" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:LSItemContentTypes:1 string public.plain-text" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:LSItemContentTypes:2 string public.source-code" "$OUT/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :NSHighResolutionCapable bool true" "$OUT/Contents/Info.plist" 2>/dev/null || true

mkdir -p "$OUT/Contents/Resources"
cp "$ICON_ICNS" "$OUT/Contents/Resources/PositEdit.icns"
# Ship the Icon Composer Liquid Glass document alongside the .icns for OS compositing tooling.
if [[ -d "$ICON_PACKAGE" ]]; then
  rm -rf "$OUT/Contents/Resources/PositEdit.icon"
  cp -R "$ICON_PACKAGE" "$OUT/Contents/Resources/PositEdit.icon"
fi

# Drop default Automator/applet icons so CFBundleIconFile=PositEdit wins in Finder/Dock.
rm -f "$OUT/Contents/Resources/applet.icns" \
  "$OUT/Contents/Resources/AutomatorApplet.icns" \
  "$OUT/Contents/Resources/droplet.icns" 2>/dev/null || true
# Assets.car from osacompile carries the stock droplet artwork; remove so .icns / .icon win.
rm -f "$OUT/Contents/Resources/Assets.car" 2>/dev/null || true

# Touch the bundle so LaunchServices picks up the new icon.
touch "$OUT"

echo "built $OUT"
echo "  icon: Liquid Glass (.icon) + PositEdit.icns"
echo "  install: cp -R \"$OUT\" /Applications/ && hash -r"
echo "  requires: posit-edit CLI on PATH (cargo install / nix profile / ./target/release/posit-edit)"
