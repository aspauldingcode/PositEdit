# Universal IDE DLC: Project Intelligence & Runtime Workbench

An **additive extension only**. This archive contains no files from the earlier editor archive, no replacement foundation, and no redistributable upstream binaries. It provides new project metadata inspection, Build/Run/Configure/Test controls, native command planning, a verified Python-based reference control plane, a Rust TUI toolbar integration crate, runtime launcher contracts, graph-engineering roadmap, and a one-shot agent development directive.

## Run this DLC independently

Requires Python 3.11+; no Python third-party dependencies.

```sh
python3 dlc.py --root /path/to/workspace scan
python3 dlc.py --root /path/to/workspace inspect --system cmake
python3 dlc.py --root /path/to/workspace plan build --system cmake
python3 dlc.py --root /path/to/workspace ui
```

The TUI has **clickable `[1 Build] [2 Run] [3 Configure] [4 Test] [5 Refresh] [6 Browser] [7 Phone] [8 Android]`** controls, with corresponding keys. Use Tab to cycle among detected build systems. Commands are previewed and require explicit `y` approval. `Configure` edits only the IDE's `.ide-dlc.json` launch configuration; it does NOT claim universal native project-file round-trip support.

For non-interactive builds:

```sh
python3 dlc.py --root /path/to/workspace plan build --system cmake
python3 dlc.py --root /path/to/workspace execute build --system cmake --approve
```

Only use `--approve` for trusted source trees. Gradle, Nix, Maven, CMake, Meson, and other build definitions may execute arbitrary scripts, fetch dependencies, or mutate outputs.

Example setup for a CMake project:

```sh
python3 dlc.py --root /path/to/workspace set build_dir build
python3 dlc.py --root /path/to/workspace execute configure --system cmake --approve
python3 dlc.py --root /path/to/workspace execute build --system cmake --approve
python3 dlc.py --root /path/to/workspace set run_artifact build/myapp
python3 dlc.py --root /path/to/workspace execute run --system cmake --approve
```

## Runtime commands (after upstream tools installed)

```sh
python3 dlc.py --root /path/to/workspace doctor
python3 dlc.py --root /path/to/workspace set browser_url http://localhost:5173
python3 dlc.py --root /path/to/workspace runtime browser
python3 dlc.py --root /path/to/workspace runtime browser --execute --approve
python3 dlc.py --root /path/to/workspace set phone_session work
python3 dlc.py --root /path/to/workspace runtime phone-boot --execute --approve
python3 dlc.py --root /path/to/workspace runtime phone-view --execute --approve
python3 dlc.py --root /path/to/workspace set avd Pixel_9_Arm64
python3 dlc.py --root /path/to/workspace runtime android-boot --execute --approve
python3 dlc.py --root /path/to/workspace runtime xtool-dev
```

Direct upstream viewers that draw to the terminal must use an external pane or an approved embedded graphics protocol. The standalone curses view delegates to the upstream tools rather than pretending an embedded compositor is already built. The Android launch presently starts an existing AVD with its supported host GPU configuration; embedding its gRPC video and input is an engineering milestone, not a tested feature.

## Merge into an existing Rust IDE

Read `integration/README.md`, `DLC_ARCHITECTURE.md`, and `DLC_PLATFORM_TARGETS.md`. Copy only the additive `ide-dlc-ui` Rust crate and register its toolbar/command events; the control plane can run as an optional process. No original IDE source is included or overwritten.

## Verify

```sh
python3 -m unittest discover -s tests -v
python3 -m compileall -q project_dlc
python3 engineering/graph/validate.py
```

Additional verification required on the actual target machines: Cargo compile and rustfmt, Bend proof verdict, Nix package build, Gradle/Maven/JVM bridge builds, Android emulator/ADB, Apple's Xcode and real devices, native JSON Xcode round-trip, screenshots and GPU performance. See `DLC_VERIFICATION.md` for explicit status.

## Security & portability

No auto-install of Apple SDKs, host credentials, IDE plugins, rooted device tools, or proprietary vendor binaries. No unattended physical device input or signing. No implicit Bash shell, no network during project discovery, no untrusted Gradle/Nix evaluation during metadata scan, and no unsupported build command guessing.

### First native configuration writer: CMake user presets

The DLC includes one narrowly scoped native project writer, separate from IDE launch settings. It appends a validated user-level configure preset while preserving existing JSON properties semantically and refusing to overwrite existing preset names. Preview first:

```sh
python3 dlc.py --root /path/to/cmake-project cmake-user-preset dlc-debug --build-type Debug
python3 dlc.py --root /path/to/cmake-project cmake-user-preset dlc-debug --build-type Debug --apply
cmake --list-presets
cmake --preset dlc-debug
```

In the standalone TUI press **3 Configure**, type `cmake-native`, then enter a preset name and build type. It asks for confirmation before writing `CMakeUserPresets.json`. This does not imply Gradle, IntelliJ, Maven or Xcode native editing has been implemented.

### Optional merge/overlay staging

```sh
python3 scripts/install_overlay.py --host /path/to/existing-rust-ide
python3 scripts/install_overlay.py --host /path/to/existing-rust-ide --apply
```

This does **not** silently claim to finish host UI integration: it copies the additive Rust crate/Python service and registers the crate dependency, then your engineering agent wires the UI render and input events in the host IDE and tests the result. No previous-project source files are included in this DLC archive.
