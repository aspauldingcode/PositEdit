# Skill: formal-bend

**Goal:** Model invariant relationships without pretending proofs cover hardware.

**Tools/standards:** DLC_LAWS.bend, DLC_PROOF.bend, Bend 2 CLI.

**Implementation discipline:** Validate syntax with actual installed Bend version; model atomic state and trust; do not claim Rust/Swift runtime conformance without tests..

**Evidence required:** Proof verdict plus implementation conformance tests and recorded tool version..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
