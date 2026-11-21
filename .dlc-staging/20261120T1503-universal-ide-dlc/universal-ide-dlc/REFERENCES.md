# Official upstream references for the DLC (verified 2026-11-20)

These URLs are reference sources, not vendored files or endorsements. Integrators must pin commits/releases and re-validate command/API compatibility before shipping. Some tools are platform- and licensing-constrained.

## Project models and build configuration

- Gradle Tooling API: https://docs.gradle.org/current/userguide/tooling_api.html
- Gradle Tooling API Javadoc: https://docs.gradle.org/current/javadoc/org/gradle/tooling/model/GradleProject.html
- Gradle project builds: https://docs.gradle.org/current/userguide/multi_project_builds.html
- Gradle Android plugin API: https://developer.android.com/build/extend-agp
- Maven effective POM: https://maven.apache.org/plugins/maven-help-plugin/effective-pom-mojo.html
- Maven core model: https://maven.apache.org/ref/current/maven-model/
- IntelliJ project structures: https://www.jetbrains.com/help/idea/project-settings-and-structure.html
- IntelliJ run/debug: https://www.jetbrains.com/help/idea/run-debug-configuration.html
- CMake File API: https://cmake.org/cmake/help/latest/manual/cmake-file-api.7.html
- CMake Presets: https://cmake.org/cmake/help/latest/manual/cmake-presets.7.html
- CMake build: https://cmake.org/cmake/help/latest/manual/cmake.1.html
- CTest: https://cmake.org/cmake/help/latest/manual/ctest.1.html
- Meson IDE integration: https://mesonbuild.com/IDE-integration.html
- Meson commands: https://mesonbuild.com/Commands.html
- Nix CLI: https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-flake-show
- Nix flake manual: https://nix.dev/concepts/flakes
- Cargo metadata: https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
- Swift package manager: https://docs.swift.org/package-manager/PackageDescription/PackageDescription.html
- Bazel query: https://bazel.build/query/guide
- Zig build system: https://ziglang.org/learn/build-system/
- npm scripts: https://docs.npmjs.com/cli/v11/using-npm/scripts/

## Apple project/runtime/cross-development

- Apple JSON `.xcproj` schema/library: https://github.com/apple/xcode-project-format
- Xcode JSON project configuration docs: https://developer.apple.com/documentation/xcode/updating-your-xcode-project-configuration-file-format
- Apple `xcodebuild`: https://developer.apple.com/library/archive/technotes/tn2339/_index.html
- Xcode schemes: https://developer.apple.com/documentation/xcode/customizing-the-build-schemes-for-a-project
- xtool: https://github.com/xtool-org/xtool
- xtool documentation: https://xtool.sh/
- Omarchy Apple Dev: https://github.com/joshuaswarren/omarchy-apple-dev
- Omarchy Apple Dev distro matrix: https://github.com/joshuaswarren/omarchy-apple-dev/blob/main/docs/DISTROS.md
- Omarchy Apple Dev test matrix: https://github.com/joshuaswarren/omarchy-apple-dev/blob/main/docs/TEST-MATRIX.md
- terminal-phone: https://github.com/govijr/terminal-phone
- terminal-phone architecture: https://github.com/govijr/terminal-phone/blob/main/docs/TERMINAL-PHONE-ARCHITECTURE.md
- terminal-phone agent guide: https://github.com/govijr/terminal-phone/blob/main/docs/TERMINAL-PHONE-AGENT.md
- terminal-phone security: https://github.com/govijr/terminal-phone/blob/main/SECURITY.md
- Apple SDK/download: https://developer.apple.com/download/
- Apple licensing: https://developer.apple.com/support/terms/

## Android/web/terminal graphics

- Android Emulator hardware acceleration: https://developer.android.com/studio/run/emulator-acceleration
- Android Emulator command-line: https://developer.android.com/studio/run/emulator-commandline
- Android Emulator gRPC console: https://developer.android.com/studio/run/emulator-console
- Android Virtual Devices: https://developer.android.com/studio/run/managing-avds
- ADB: https://developer.android.com/tools/adb
- Android Studio Gradle build: https://developer.android.com/build
- Android emulator WebRTC reference: https://github.com/google/android-emulator-webrtc
- scrcpy: https://github.com/Genymobile/scrcpy
- terminal-browser: https://github.com/zenbu-labs/terminal-browser
- embedded terminal-browser example: https://github.com/zenbu-labs/terminal-browser/tree/main/examples/embedded
- Kitty Graphics Protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- iTerm2 inline image protocol: https://iterm2.com/documentation-images.html
- Sixel: https://vt100.net/docs/vt3xx-gp/chapter14.html
- MCP official spec: https://modelcontextprotocol.io/specification

## Rust integration & verification

- Ratatui: https://ratatui.rs/
- Crossterm: https://docs.rs/crossterm/
- Rust process management: https://doc.rust-lang.org/std/process/struct.Command.html
- Tokio: https://tokio.rs/
- Bend 2: https://github.com/bendlang/bend

### Warning

Native project importing can execute untrusted code even when the intent is only to read models (Gradle, Maven plugins, Nix, Swift Package.swift, etc.). A read-only file parser and native model evaluation are separate modes, and the latter requires trust. No broad claim of Xcode, IntelliJ, Linux Apple SDK or Android simulator parity is implied by documenting these APIs.
