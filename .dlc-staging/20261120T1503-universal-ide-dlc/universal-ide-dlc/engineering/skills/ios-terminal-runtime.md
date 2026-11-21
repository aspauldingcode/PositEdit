# Skill: ios-terminal-runtime

**Goal:** Embed iOS terminal-phone as an IDE runtime.

**Tools/standards:** terminal-phone CLI/MCP, Xcode/CoreSimulator, physical iPhone.

**Implementation discipline:** Owner-permission model enforced; split-pane first, then owned graphics and input surface, no competing stdout writers..

**Evidence required:** On-device/simulator capture, interaction, logs, launch and crash recovery on actual hardware..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
