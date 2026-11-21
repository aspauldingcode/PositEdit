# Skill: android-accelerated-runtime

**Goal:** Integrate IntelliJ-compatible Android AVD on Apple Silicon/Linux.

**Tools/standards:** Android Emulator, AVD manager, HVF/KVM, GPU backend, Emulator gRPC, ADB, scrcpy.

**Implementation discipline:** Virtualization not CPU emulation; measure actual acceleration; secure gRPC; no silent AVD image downloads..

**Evidence required:** Device boots, installs, tests, gRPC frames/touches under IDE; fps/latency receipts on each host..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
