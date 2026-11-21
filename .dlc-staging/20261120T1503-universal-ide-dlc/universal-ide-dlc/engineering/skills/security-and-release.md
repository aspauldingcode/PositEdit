# Skill: security-and-release

**Goal:** Fail closed on untrusted projects and physical devices.

**Tools/standards:** Permissions, process groups, native build tool script execution, SDK licenses, immutable logs.

**Implementation discipline:** No default execute on project scan, no credential capture, no unauthorized install/signing, bounded retries; provide receipts per required platform..

**Evidence required:** Threat-model test, native integration suite, Nix/Rust/Swift/JVM builds, actual device verification..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
