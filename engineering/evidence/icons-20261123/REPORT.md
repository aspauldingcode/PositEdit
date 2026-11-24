# Posit Edit icons + wrapper (2026-11-23)

## Done
- Stripped ancillary PNG metadata (`caBX` etc.) from Downloads `PositEdit-Icons` into `assets/icons/source/`.
- Authored Icon Composer 2 Liquid Glass package from PE **silhouette** (not brushed metal): `assets/icons/PositEdit.icon`.
- Validated with bundled schema + `ictool` (Icon Composer 2.0 / bundle 143) Default + Dark macOS exports.
- macOS neovim-style wrapper: `scripts/build-macos-wrapper.sh` → `dist/PositEdit.app` (CLI launcher + Liquid Glass `.icon` + `.icns`).
- Linux metal FreeDesktop icons + `packaging/linux/posit-edit.desktop`; flake `postInstall` installs them.
- README hero + Help menu (`help_markdown`) embed `assets/icons/posit-edit.png`.

## Evidence commands
- `uv run python validate_icon.py assets/icons/PositEdit.icon` → VALID
- `ictool … --platform macOS --rendition Default` → exit 0
- `sh ./scripts/build-macos-wrapper.sh` → built dist/PositEdit.app
- `cargo test -p ide-tui --lib help_button_is_visible` → ok
