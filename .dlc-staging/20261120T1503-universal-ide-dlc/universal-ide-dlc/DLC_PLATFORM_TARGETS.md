# Platform, target, toolchain and destination model

The IDE must never confuse **host platform** with **build target** or **run destination**. A project can support many targets from one workspace, and one target may run in several environments.

## Universal target identity

```text
ProjectTargetKey {
  native_project_id, native_module_id, target_name, target_kind,
  build_variant, configuration, compiler_toolchain, sdk,
  target_os, target_abi, target_cpu, min_deployment_version,
  packaging_format, signing_profile, runtime_destination
}
```

Examples: `DemoApp | Debug | iphoneos | arm64 | iPhone18,4`, `DemoApp | Debug | iphonesimulator | arm64 | iOS Simulator`, `:app | freeDebug | android | arm64-v8a | emulator-5554`, `hello | RelWithDebInfo | linux | x86_64 | local`. These are not interchangeable outputs.

## Apple platform families

| Target | Build backend on macOS | Linux backend target | Run/debug destination | Release requirement |
|---|---|---|---|---|
| iOS physical | Xcode/SwiftPM | xtool/omarchy, where demonstrated | Paired iPhone | Verify SDK/signature, install, debug on real phone |
| iOS Simulator | Xcode/CoreSimulator | No native Apple CoreSimulator | iOS Simulator on macOS | Verify simulator OS/runtime, signing and app architecture |
| iPadOS | Xcode iOS SDK and device profile | Limited same supported iOS cross-target | iPad, compatible simulator on Mac | Verify true device size, capabilities and deployment |
| macOS | Xcode/SwiftPM | Experimental cross-build | Local/remote Mac | Build + launch + debugger receipt |
| macCatalyst | Xcode | No general Linux equivalence established | Mac | Native Xcode destination receipt |
| watchOS | Xcode | No general Linux equivalence established | Watch simulator/paired watch via Mac | Supported only after toolchain and runtime evidence |
| tvOS | Xcode | No general Linux equivalence established | Apple TV/Simulator via Mac | Supported only after real runtime receipt |
| visionOS | Xcode | No general Linux equivalence established | visionOS Simulator/headset | Supported only after real runtime receipt |

The project graph still represents unsupported build destinations; they are not discarded just because the current host cannot run them. Unknown SDK licensing status is an explicit blocker to licensed toolchain installation.

## Android platform families

| Target | Build | Run/view | Debug/test |
|---|---|---|---|
| Android phone ARM64 emulator on Apple Silicon macOS | Gradle AGP variant | Google Android Emulator ARM64 AVD on HVF, graphical capture/backend negotiated | JDWP, ADB, emulator gRPC, instrumentation |
| Android phone emulator Linux | Gradle AGP variant | Google AVD + KVM if supported for guest/host ABI, compatible graphics | ADB, gRPC, instrumentation |
| Physical Android phone/tablet | Gradle APK/AAB profile | ADB installation; capture via device video backend | JDWP, logcat, UIAutomator |
| Wear OS emulator/device | Gradle WearOS target | Compatible AVD/physical watch, capability gated | ADB, instrumentation where available |
| Android XR | Gradle XR target | XR emulator/device, separate spatial UI backend required | Device-specific debugging/inspection |

Do not claim platform-agnostic GPU zero-copy: screen acquisition/transport may still involve copies or encoding. Android Studio is an IntelliJ-based IDE; the Google Android Emulator is a separate executable/runtime, not a JetBrains emulator engine.

## JVM, native and web

Gradle supports JVM/Android/Kotlin Multiplatform targets including JS, Wasm and native variants through plugins, each needing its own model/target ABI/runtime adapter. Maven supports JVM and plugin-specific packaging. CMake/Meson/Bazel may cross-compile with suitable toolchain files and SDKs; native project model must preserve these distinctions. Nix development environments may be native or cross-system derivations. Web projects may use Node, Bun, Deno or other runtimes and must have explicit script and URL detection rather than assume `npm run dev` always exists.

## Capability resolution schema

```text
RequestedSelection
  -> ModelVersion + NativeTarget
  -> BuildProfile/Variant
  -> HostCapability + Toolchain/SDK
  -> ArtifactKind + target ABI
  -> DestinationCapability
  -> Native build/test/install/launch/debug plans
  -> Verified session with logs/graphics/inspection
```

Every failure shows a precise reason: `toolchain_missing`, `sdk_unavailable`, `target_variant_unknown`, `artifact_incompatible`, `signing_required`, `device_unpaired`, `device_locked`, `unsupported_platform`, `acceleration_unavailable`, `graphic_protocol_missing`, `native_adapter_incomplete`.

Remote macOS hosts are optional for Apple simulator work from NixOS; Linux-native Apple builds should not silently route through macOS when the user requests no macOS involvement. Physical Apple-device control retains explicit authorization.
