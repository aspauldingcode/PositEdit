# Skill: intellij-gradle-maven

**Goal:** Implement JetBrains-class project semantics.

**Tools/standards:** IntelliJ .idea/.iml/runs, Gradle Tooling API, Android plugin models, Maven effective POM.

**Implementation discipline:** Distinguish IDE metadata from authoritative native build; module, facets, source roots, SDK versions, composite builds..

**Evidence required:** Compare imports, variants and tasks with IntelliJ or Android Studio on multiple version fixtures..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
