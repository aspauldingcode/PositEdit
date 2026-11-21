# Skill: web-terminal-runtime

**Goal:** Provide real Chromium web developer experience in IDE.

**Tools/standards:** terminal-browser embedded mode, browser CLI, DevTools and DOM.

**Implementation discipline:** Launch dev server, verified URL, embedded graphics ownership, focus/input routing, HMR, responsive presets..

**Evidence required:** Full webpage visual regression, DOM/console/network inspection, mouse+keyboard and resize tests..

**General constraints:** Never claim full native IDE parity from static parsing. Reuse official APIs and preserve foreign extensions; run builds only in trusted workspaces. Cache tool version, target and provenance, apply graph task gates, and report missing host capabilities. All changes to user devices, signing, or non-workspace files require separate authorization.
