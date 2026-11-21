# Engineering verification gates for autonomous implementation

G0: Environment is detected and report is reproducible; no silent tool installer or credential escalation.
G1: File detection is passive, bounded and correct for mixed roots; no untrusted project evaluation.
G2: Every adapter resolves target/config/toolchain/destination through documented native models, preserving unknown native data.
G3: Every configuration mutation has a preview, owner-specific writer, reimport, semantic diff, rollback and fixtures.
G4: Every Run action verifies compatible executable output and target ABI; no fabricated command.
G5: UI controls Build, Run, Configure, Test respond by mouse and keyboard, and host UI stays responsive under long-running jobs.
G6: Browser: real terminal-browser embedded pane in host editor, input/viewport/DOM/console tested on macOS+Linux.
G7: iOS: terminal-phone simulated and physical sessions tested, requires owner's input permission and macOS SDK for CoreSimulator.
G8: Apple Linux: xtool/omarchy builds and real-device signing/deploy/debug verified under explicit SDK rights.
G9: Android: AVD list/create/start, HVF/KVM GPU acceleration verified, gRPC frames and input under IDE pane, ADB and JDWP tests.
G10: Diagnostics, logs, visual observations and test recordings are timestamped with process/device versions and revisions.
G11: Blend formal model/proofs with Rust/Swift/JVM/Python native integration tests; mathematically provable control properties distinct from real runtime behavior.
G12: Cross-platform release matrix, performance baselines and CI green; unresolved native parity cells explicitly marked unsupported.

Never satisfy a gate by changing its requirement or deleting failing tests. Never claim a vendor feature is tested unless the real toolchain/device executed and a usable receipt exists.
