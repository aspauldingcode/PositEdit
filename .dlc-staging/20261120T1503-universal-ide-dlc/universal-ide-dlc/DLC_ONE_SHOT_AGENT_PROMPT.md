# ONE-SHOT AGENT DIRECTIVE: UNIVERSAL IDE PROJECT + RUNTIME DLC

You are the autonomous integration engineering orchestrator for **only the additive DLC described in this repository**. The host application is an existing Rust terminal IDE; your deliverable is an extension. You must discover host interfaces and integrate the extension without replacing, rewriting or duplicating pre-existing IDE features. Do not copy this repository's base editor content into the DLC. This package contains only additional scope: universal project intelligence/configuration, build/run/test controls, and graphical Apple/Android/Web runtime workbenches.

## Mission and acceptance

Ship an independently verifiable and production-quality extension with:

1. Mouse- and keyboard-activated **Build**, **Run**, **Configure**, **Test**, **Refresh**, **Browser**, **Phone**, **Android** controls inside the host Rust editor. Job status, tool diagnostics, progress, exit codes, logs and cancellation are visible without stalling the TUI. In particular, Configure must present actual project settings and targets; it is not just a text box with extra compiler flags.
2. Deep read/modify/build/run/debug/test compatibility with IntelliJ `.idea`/`.iml` and external systems, Gradle, Maven/POM, CMake, Meson, Nix, Xcode `.xcodeproj/project.xcproj` **JSON only**, SwiftPM, Cargo, Zig, JS/TS package managers, Bazel, Make/Ninja, extensible to other systems.
3. Correct native project semantics: multiple roots, modules/source sets, build variants, profiles, toolchains, system SDKs, native configuration inheritance, target triples, dependencies, generated sources, resource bundles, packages, build steps, test suites, source mapping, artifacts, launch/debug configurations and native IDE round-trips where public APIs permit.
4. Apple mobile: local iOS Simulator via terminal-phone on compatible macOS, real iPhone sessions with explicit owner input authorization, Linux-native Apple build/deploy via xtool and omarchy-apple-dev where supported, no false promise of Apple's CoreSimulator running on Linux.
5. Android mobile: Google Android Emulator AVDs compatible with Android Studio and Gradle variants. Prefer ARM64 Android guest on Apple Silicon with Hypervisor.framework + GPU acceleration; KVM Linux on compatible machines. Use emulator gRPC/ADB and physical-device transport to embed visible and interactive graphical phone panes. Acceleration MUST be measured and reported, not assumed from command-line flags.
6. Web: actual terminal-browser embedded TUI viewport, Chromium GPU rendering, input, DOM/console/devtools, multiple URLs, HMR and visual regression. No custom browser engine.
7. Unified runtime API: one session/capability/launch/inspect/input/view/debug contract across browser, phone simulator/physical, and Android AVD/physical devices. Keep all heavy services lazy/offline core unaffected.
8. Evidence: real tests per native model, native build, native configuration mutation, artifact and target, runtime, graphics; comparable behavior in vendor IDE where possible; security reviews and measured performance.

**Do not declare full universal 1:1 parity or production stability without evidence.** A capability that depends on proprietary plugins, missing SDK rights, unsupported host architecture, absent device permission or unavailable native tool MUST be represented accurately as partial/unavailable, never as a fake success.

## Start by inspecting the installed tools

Determine host OS/arch, CPU, RAM, terminal graphics capabilities, local software, Rust/Cargo/Nix, JVM/JDK/Gradle/Maven, CMake/Meson, Apple Swift/xtool/omarchy, Apple SDK licensing, Xcode/Simulator/paired devices, Android SDK/emulator/AVDs/ADB, terminal-phone, terminal-browser. Identify available authenticated automation/AI tools and network. Capture versions and checksums. Read only documented supported APIs from REFERENCES.md. Pin upstream commit versions in a dependency ledger before writing integration code.

If the current host cannot run Mac-only or physical-device tests, build and test the generic layer, run mock/fixture tests, record an explicit unresolved platform gate, and continue other independent work. Never fabricate a test or request that a human provide credentials through an agent log. Never bypass platform security or Apple SDK license restrictions.

## Read in this order

1. `README.md`: available working reference prototype and how to test.
2. `DLC_ARCHITECTURE.md`: boundaries, native model/semantic graph, runtime adapters and workflows.
3. `DLC_COMPATIBILITY_MATRIX.md`: advertised vs missing features.
4. `DLC_PLATFORM_TARGETS.md`, then `DLC_VERIFICATION.md`: exact currently verified evidence and missing platform work.
5. `DLC_LAWS.bend`, `DLC_PROOF.bend`, `engineering/bend/state.bend`: additive formal invariants.
6. `engineering/graph/nodes.json`, `engineering/graph/edges.json`, `engineering/VALIDATION_GATES.md`: persistent task graph.
7. `integration/README.md`, `integration/contracts.json`, `integration/rust/ide-dlc-ui`: host Rust UI/JSON bridge contract.
8. `engineering/skills/*.md`: specialized skill playbooks.
9. `REFERENCES.md`: authoritative APIs and upstream GitHub repositories.

## The initial code is not finished product

The included standard-library Python control plane implements passive detection, narrow native metadata inspection, some native command planning, IDE-owned configuration persistence, a standalone interactive terminal panel, runtime CLI action previews and a test suite including actual CMake configure-build-run. It is a reference and bridge, NOT proof of complete native IDE equivalence or embedded GPU view. The included Rust `ide-dlc-ui` crate has not been compiled on this environment. Your job is to finish native integrations and platform verification, replacing Python components with Rust equivalents if performance/integration justify while preserving stable JSON contracts.

## Organize work as a graph (mandatory)

- Use nodes and dependencies in `engineering/graph`. Every task has requirement, native reference, design contract, owner, branch/worktree, tests, risk, evidence, completion criterion, and regression dependents.
- Strict dependency ordering: native models precede executable target plans; projects precede native configuration editing; destination resolution precedes Build→Run; graphics ownership precedes embedded browser/phone/Android panes.
- A build test success proves only that build test, not device or native project round-trip.
- Parallelize independent adapters in isolated worktrees with additive changes; merge only after completed checks. Never run two writers concurrently against the same project config without revision/ownership locks.
- Before every task: retrieve minimal relevant code, current graph nodes, native API docs, applicable law and failing receipts. Do not flood an agent with all repository context.
- Each task must emit an evidence receipt (command, cwd, test fixture, actual environment, tool/version, commit, stdout/stderr path, exit, timestamps, artifact hashes). Immutable accepted release evidence; new attempts may supersede but not rewrite.
- On failures, classify reproducible defect, narrow hypothesis, write regression test, repair minimal component, rerun target checks, update graph. Bound retries. Exhausted or unauthorized action becomes `BLOCKED`, not `DONE`.
- After integration, reopen dependent nodes if a contract changes. Validate graph acyclicity and all required evidence before advancing to release.

## Architecture and implementation requirements

### Universal native model

Use two layers: lossless native representation with opaque vendor extension fields and a versioned semantic graph of `Workspace, Project, Module, SourceSet, Target, Variant, Configuration, Toolchain, SDK, Platform, Destination, Dependency, Resource, Artifact, BuildAction, RunAction, DebugAction, TestAction, Environment`.

Mandatory relationships `CONTAINS, DEPENDS_ON, BUILDS, TESTS, RUNS, DEBUGS, TARGETS_PLATFORM, USES_TOOLCHAIN, GENERATES, CONFIGURED_BY, DEFINED_IN`. Every node records authoritative origin URI/ID, tool version, evaluated profile, system triple, source revision, trust status, completeness and confidence. No “detect == model resolved” shortcuts.

### Native importers

- Gradle: JVM Tooling API bridge. Capture GradleBuild/GradleProject/tasks, composite builds, sources, variants. Android plugin variants via supported APIs. Treat Gradle scripts as executable. Start from `integration/bridges/GradleModelBridge.java` as a deliberately limited reference. Expand with real Gradle tests and plugin compatibility.
- IntelliJ: `.idea`/`.iml`, JPS models where supported, run configs, facets, SDKs, project libraries; reconcile with external Gradle/Maven authoritative models, preserve proprietary/unknown plugin settings.
- Maven: effective POM, inherited parents, profiles, plugin executions, reactor modules, classpath, test runners. Preserve existing XML on writes and test reimport.
- CMake: supported File API codemodel/cache/toolchains, configure/build/test presets, multi-config and cross-compiler platforms. Build with native CMake; do not reconstruct compilation using IDE-only flags.
- Meson: official `intro-*.json`, machine files, options, targets, build/configure/test. Do not execute arbitrary Meson script during passive scan.
- Nix: evaluation with explicit user trust; flakes/system outputs, `nix develop` environment and deterministic `nix build/run`; preserve impurity setting and SDK licensing provenance.
- Xcode: ONLY new `project.xcproj` JSON, use Apple's `XcodeProjectFormat` Swift package, separate scheme XML if required, Xcode native build on compatible Mac and xtool where capabilities are demonstrated; round-trip native model and target selection. Ignore legacy `.pbxproj` rather than silently migrate.
- SwiftPM/Cargo/Zig/Bazel/JS: use their native metadata/query/build tools, not just filename parsing; target variants, compiled outputs, launch/test/debug configuration.

### Configuration UI / round trips

Build actual TUI settings tree: build profiles, modules, variants, SDK/toolchain, environment, compiler options, linker, packaging, signing, architectures, run/debug/test, deployment destination. Every property displays its authoritative source and mutability. Implement preview → native schema validation → atomic update → reimport → semantic equivalence → rollback. Refuse unknown writes; do not format/rewrite entire vendor file needlessly.

### Execution planner

Produce typed immutable plan with correct native tool, exact argv, environment, cwd, prerequisites, expected artifacts, source mapping, build config, selected target/destination. Validate host/toolchain/SDK/architecture. Execute with host process supervisor (timeouts, cancellation, process groups, limits); preserve progress and diagnostics in existing IDE. Run never guesses the entrypoint or launches a mismatched artifact.

### GPU and graphics

Plan a terminal graphics surface compositor with Kitty graphics as preferred where supported and iTerm2/Sixel/Unicode fallback. A device/browser may render via host GPU; final terminal transport can still require frame copies. Measure capture, encoding, transport, presentation independently. The host renderer owns terminal output; embedded view must not independently corrupt text panes. Handle clipping, resize, image deletion, focus arbitration, logical coordinate transformation, mouse/touch scrolling and stale frames. Keep GPU viewers as external child processes.

### Apple simulator/device and Linux

Use terminal-phone CLI/MCP on macOS, inspect terminal split/embedded APIs before integrating. Simulators require Xcode/CoreSimulator on macOS; real devices require pairing, signing, developer mode and explicit owner input authorization. For Linux compile/deploy use xtool/omarchy-apple-dev without promising Xcode-equivalence; verify Swift SDK version, license provenance, resource compiler, signing and real-device LLDB support. A real device is distinct from an emulator or simulated iOS host.

### Android

Use official Android Emulator and AVD manager as runtime, not a bespoke emulator. On Apple Silicon macOS test native ARM64 AVD + HVF + supported GPU backend; on Linux test KVM and matching guest CPU architecture. Verify `emulator -accel-check`, actual host capabilities, AVD system image/API/platform. Implement authenticated emulator gRPC frame+input, correct framebuffer size/orientation and ADB install/launch/logcat/JDWP/UIAutomator. Real-device transport separately via ADB plus suitable video transport. Preserve Android Studio Gradle variants and device profiles. Do not call hardware virtualization “not an emulator” to make a misleading claim.

### Browser

Use official terminal-browser embedded TUI mode and Chromium offscreen GPU renderer, not an invented renderer. Integrate browser session/pane, HTML/CSS/JS/TS dev server URL, browser console, network, DOM inspection, responsive layouts, hot reload, controlled input and visual regression. Verify upstream protocol and version before writing the bridge; no invented terminal-browser MCP server.

## Agent skills and capability tools

Load specialist files from `engineering/skills` as needed for native models, roundtrips, Gradle/IntelliJ, Apple, Android, iOS terminal runtime, web browser, graphics, graph engineering, formal Bend, and security/release. If current AI host supports MCP, integrate official `terminal-phone mcp` when installed (example in `integration/mcp.json.example`) and only verified servers. Native CMake/Meson/Gradle/Nix CLIs are tools, not necessarily MCP servers. If MCP server is missing, implement a local verified adapter on stdio and document its authorization schema. Tools must be minimally privileged and opt-in where they can alter OS/device/third-party accounts.

## Formal laws

`DLC_LAWS.bend` governs a small trust/dispatch model, with proofs in `DLC_PROOF.bend`. Validate against actual Bend 2 syntax before trusting verdicts; do not rewrite owner-governed laws to make a proof pass. Extend the model for graph dependency validity, stable target identity, explicit resource ownership, safe mutation and input consent. A mathematical theorem about a model is NOT evidence that subprocesses, Swift code, iOS USB services or Android GPU behave correctly; run conformance tests.

## Start-to-finish gates

**A. Foundation:** compile/test existing DLC control plane, validate graph DAG, inspect host crate boundaries, generate integration plan and dependency pins; do not damage existing workspace.
**B. Host toolbar:** render Build/Run/Configure/Test and runtime buttons with keyboard/mouse, visible disabled states and shortcuts; integrate async host command routing and user action permission.
**C. Native models:** implement and verify full supported native importers, incremental caching and provenance. Fixture projects on actual vendor tools.
**D. Native config:** real native settings editors, inheritance, variants, toolchain and target management; round-trip compare with vendor IDEs.
**E. Build-run-test-debug:** artifact and ABI aware launch, JVM/C/C++/Swift/Kotlin/JS language-specific DAP paths, tests and diagnostics.
**F. Web:** embedded browser, devtools, HMR, browser interaction and visual regression.
**G. Apple:** terminal-phone sim/device (Mac), xtool/omarchy build/deploy (Linux), owner permissions and actual device validation.
**H. Android:** Gradle Android model, AVD manager, HVF/KVM acceleration, gRPC renderer, ADB and Android device tests.
**I. Unified runtimes:** one session manager, graphics/input focus, accessibility and AI agent runtime observations, cross-target inspector and snapshot/record/replay.
**J. Security/UX/performance:** fuzz native parsers, cancellation, malformed inputs, video/input stress, 30+ min runtime monitoring, resource leak tests, small core cold/warm startup, background lazy loading.
**K. Release:** full test matrix for each claimed host/guest, code signing/SDK license checks, Nix builds, Cargo/Rust/Swift/JVM tests, Bend verdict, independent final evidence audit and supported/unsupported status publication.

## Commands available now

```
python3 -m unittest discover -s tests -v
python3 -m compileall -q project_dlc
python3 engineering/graph/validate.py
python3 dlc.py --root /some/project scan
python3 dlc.py --root /some/project inspect --system cmake
python3 dlc.py --root /some/project plan build --system cmake
python3 dlc.py --root /some/project ui
cargo test --manifest-path integration/rust/ide-dlc-ui/Cargo.toml  # requires Rust and dependencies
nix build .                                                  # requires Nix
bend DLC_PROOF.bend --verdict                                # requires installed Bend 2
```

Run native project code only after explicit project trust and an execution plan preview. Do not run any downloaded installer piping directly into `bash` without pinning and verifying source and operator authorization. No background unsupervised work beyond the lifetime/session authorized by the tool's available ability. Stop when required access, credential, or device consent is unavailable, preserving everything done.

## Evidence and final response

Maintain a table `required capability / observed status / host / tool version / test command / receipt / caveat`. Never hide failures or infer platform support from another machine. At completion, provide an additive merge patch or new DLC release, executed test logs and source hashes. If not all gates pass, present exactly which are blocked and what can be used now. No claim of 'fully tested stable universal' unless every required gate has truly passed on the target platform.

Begin now with the provided graph, host repository discovery, toolchain check and tests. Continue task by task without asking the user to restate known goals. Human approval is required only for out-of-scope/policy-constrained operations, credentials, Apple SDK rights, physical device control and other real-world actions that cannot safely be authorized by this directive alone.
