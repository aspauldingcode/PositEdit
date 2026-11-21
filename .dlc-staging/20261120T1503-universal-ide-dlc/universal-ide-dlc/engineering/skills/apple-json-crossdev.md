# Skill: apple-json-crossdev

**Goal:** Provide Apple JSON project and Linux toolchain interoperability.

**Tools/standards:** Apple xcode-project-format, Xcode schemes, xtool, omarchy-apple-dev, Swift SDK, LLDB.

**Implementation discipline:** Read only .xcproj schema for Xcode project; support schemes separately; do not bundle licensed SDK, verify mode and target..

**Evidence required:** Native Xcode JSON roundtrip; macOS build or documented Linux build plus real iPhone deployment; capture receipts..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
