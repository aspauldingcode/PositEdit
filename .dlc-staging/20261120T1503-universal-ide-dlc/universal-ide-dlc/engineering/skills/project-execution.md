# Skill: project-execution

**Goal:** Resolve Build Run Debug Test precisely.

**Tools/standards:** Native toolchain, artifact resolver, ABI and destination matrix.

**Implementation discipline:** No command guessing or shell concatenation; require trusted project, verified output path, compatible target/host, cancellable tasks..

**Evidence required:** Run real sample app on target and capture exit, logs, diagnostics..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
