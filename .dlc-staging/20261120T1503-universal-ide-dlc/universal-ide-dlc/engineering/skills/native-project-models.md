# Skill: native-project-models

**Goal:** Discover native build models without lossy conversion.

**Tools/standards:** Gradle Tooling API, Maven effective POM, CMake File API, Meson introspection, XcodeProjectFormat, Nix, Cargo, SwiftPM, Zig, Bazel.

**Implementation discipline:** Use source-of-truth model; preserve native IDs; no untrusted eval; emit target graph, provenance and partial failure records..

**Evidence required:** Fixture per ecosystem; compare native models with actual vendor CLI/IDE; record exclusions..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
