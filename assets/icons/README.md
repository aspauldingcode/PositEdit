# Posit Edit icons

Source PNGs were stripped of ancillary metadata (including Apple `caBX` chunks) before packaging.

| Asset | Use |
| --- | --- |
| `PositEdit.icon/` | macOS Icon Composer 2 Liquid Glass document (PE **shape**, not brushed metal) |
| `PositEdit.icns` | macOS `.app` bundle icon fallback (`CFBundleIconFile`) |
| `posit-edit.png` | README + Help menu preview (Liquid Glass macOS render) |
| `posit-edit-linux.png` / `posit-edit-metal-*.png` | Linux / FreeDesktop metal mark |
| `posit-edit-shape.png` | White PE silhouette fed into Icon Composer |
| `source/` | Cleaned originals (gradient, metal, pre-composited) |
| `previews/` | `ictool` exports (Default / Dark) |

Rebuild the macOS wrapper:

```sh
sh ./scripts/build-macos-wrapper.sh
```
