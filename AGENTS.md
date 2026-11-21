# Posit Edit agent instructions

Read `ONE_SHOT_PROMPT.md` first. `LAWS.bend` and `SPEC.bend` are accepted requirements; do not edit them. Proof code lives in `PROOF.bend` and modeling code in `bend/model.bend`. Run `bend guide` for syntax, `bend PROOF.bend --verdict` for the real proof gate, and Rust conformance tests for real behavior. Run `python3 scripts/harness.py status` before new tasks. Work from graph dependencies. No false completion or invented results. Limit edits to this repository. Secrets, macOS privileges, network publication and bills are out of scope without authorization.

If `IDE_HARNESS_CHILD=1`, perform the assigned task directly and **never spawn** `scripts/harness.py run`, `agent -p`, `codex exec`, `claude -p`, or another orchestration loop. Always return measurable command results.
