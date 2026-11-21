# Compatibility and native-fidelity matrix

**Status meanings:** `tested` = exercised in this sandbox; `implemented-not-tested` = implementation code available without target environment; `prototype` = limited capability; `planned` = future engineering; `unsupported` = explicit boundary.

| Project system | Passive detection | Native authoritative model | Native compile plan | Run selection | Native setting roundtrip |
|---|---|---|---|---|---|
| IntelliJ .idea/.iml | tested | planned | delegate only | planned | planned |
| Gradle | tested | JVM bridge reference, not tested | tested planning | requires explicit task | planned |
| Maven POM | tested | planned effective model | tested planning | main class required | planned |
| CMake | tested | File API passive read implemented | **real configure-build-run tested** | path configured | planned preset mutation |
| Meson | tested | intro-targets passive read tested | tested planning | blocked until validated artifact | planned |
| Nix | tested | trusted eval planned | tested planning | explicit flake attribute | planned |
| Xcode `.xcproj` | tested | JSON read partial, Swift writer planned | tested planning | native runtime still needed | planned |
| `.pbxproj` | intentionally excluded | unsupported | unsupported | unsupported | unsupported |
| SwiftPM | tested | trusted manifest API planned | tested planning | named product required | planned |
| Cargo | tested | basic manifest read tested | tested planning | cargo bin/config | planned |
| Zig | tested | planned | tested planning | named build step | planned |
| JS/TS | tested | scripts metadata tested | tested planning | declared dev/run script | JSON writer planned |
| Bazel | tested | planned | tested planning | explicit label | planned |
| Make/Ninja | detected | limited | basic planning | requires native target | planned |

| Runtime | Current DLC | Future IDE-native implementation |
|---|---|---|
| terminal-browser | documented command plan; executable unavailable here | embedded TUI via native pane protocol, DEVTOOLS and visual testing |
| terminal-phone iOS simulator | documented CLI plan; Mac/Xcode unavailable here | integrated iOS session, focus/input, LLDB, GPU surface control |
| real iPhone | owner permission prerequisites specified | physical device signed deploy/launch/control, read-only by default |
| Android AVD | native emulator launch plan, acceleration requested | gRPC frame streaming, accelerated GPU surface, Java/Kotlin debugger |
| real Android device | explicit ADB install/launch plans | ADB + frame stream + UIAutomator + physical input permission |
| Apple Linux | xtool dev invocation plan | omarchy toolchain provisioning, Swift SDK, LLDB, signing, real-device tests |

The design goal is IDE-grade parity, but current DLC must not claim 1:1 compatibility. Vendor plugins, proprietary launch configs and SDK or signing limitations are real capability boundaries.
