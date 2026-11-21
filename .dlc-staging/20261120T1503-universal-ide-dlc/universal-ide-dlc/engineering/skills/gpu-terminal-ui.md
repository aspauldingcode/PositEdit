# Skill: gpu-terminal-ui

**Goal:** Build one GPU-aware surface manager across mobile and web.

**Tools/standards:** Kitty graphics, Sixel, iTerm2, terminal-browser/phone native transport.

**Implementation discipline:** Only owner writes final graphics; use backend negotiation, clipping/deletion, cell-to-pixel mapping and input ownership..

**Evidence required:** Stress resizing, terminal capability fallbacks, stale frame rejection, measured latency and memory..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
