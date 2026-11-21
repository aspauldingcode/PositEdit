# Skill: configuration-roundtrip

**Goal:** Safely implement native build configuration editing.

**Tools/standards:** Schema-aware JSON/XML/Syntax-aware AST/Swift XCSchema writer.

**Implementation discipline:** Read original, create native diff, check revision, atomic apply, reimport, semantic verify, rollback on mismatch; preserve unknown plugin data..

**Evidence required:** Untouched model semantic equivalence and vendor IDE re-open tests..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
