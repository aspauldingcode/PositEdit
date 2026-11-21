# Graph Engineering Operating System

## Graph semantics

Persistent node types: `requirement`, `component`, `contract`, `law`, `proof`, `task`, `test`, `defect`, `risk`, `decision`, `artifact`, `benchmark`, `release`. Edges: `DEPENDS_ON`, `IMPLEMENTS`, `CONSTRAINED_BY`, `VERIFIED_BY`, `PRODUCES`, `BLOCKS`, `AFFECTS`, `REGRESSES`, `SUPERSEDES`.

Each node has a stable ID, human summary, `phase`, `priority`, `risk`, `status`, `owner`, `acceptance` (shell commands), `file_scope` and timestamps. Separate `declared requirement`, `implementation`, `observed evidence`, and `accepted decision`. The SQLite database in `engineering/state/graph.sqlite` is a runtime cache and evidence log; canonical seed data in `engineering/graph/seed.json` is version controlled.

## Scheduler

Only tasks with every `DEPENDS_ON` task `done` and no active blockers are runnable. Priority: accepted-law violations; security; reproducible regressions; critical dependency unblock; implementation value/risk. A selection algorithm can favor critical-path depth, inverse expected effort and aged tasks but may not bypass dependencies. Interleave framework setup with implementation and do not launch concurrent tasks touching the same source scope.

## Agent work transaction

1. Begin with a single runnable task and file scope. Read neighbors and accepted laws.
2. Create an implementation plan, record dependencies and tests.
3. Work in a branch/worktree if required; direct small changes are allowed on a clean local branch.
4. Build and run specific tests. Never fake verdicts or invent tooling.
5. Log exact commands, elapsed times, exit codes, compiler versions, test reports, screen capture files, failure signatures, and hashes of important evidence.
6. Update graph state and close task only after predefined acceptance commands pass. When interface changes invalidate downstream proofs/tests, reopen affected nodes.

## Failure strategy

Attempts per task are bounded by policy (default 3). After repeated failure, mark blocked with root cause, error logs and an unblocking plan; continue other eligible work. Do not resolve CI failures by skipping, xfail, weakening tests, removing law clauses, or making no-op mock implementations.

## Context engineering

Use task-neighborhood retrieval: current task, direct dependencies, concrete interfaces, applicable laws, tests, recent evidence, execution constraints and only relevant source ranges. Long-term state persists in graph, version control, and artifacts. Summarize decisions with context, alternatives, evidence and consequence. Avoid unbounded chat history, giant prompts, and duplicated snippets.

## Model-to-code traceability

`law` -> `Bend model function` -> `proof obligation` -> `Rust implementation contract` -> `property/differential test` -> `real integration evidence`. A formal Bend proof cannot establish that the Rust code obeys the theorem unless a checked refinement/translation or sound conformance argument exists. Require executable cross-checks and platform tests.

## Graph mutation CLI

Use `python3 scripts/graphctl.py add --type defect --id DEFECT-001 --title 'Problem description'`, `python3 scripts/graphctl.py add --type task --id TASK-200 --title 'Fix X' --depends TASK-060`, and `python3 scripts/graphctl.py link TASK-200 AFFECTS C07`. To invalidate a completed contract and its downstream tasks, use `python3 scripts/graphctl.py reopen TASK-060 --downstream`. The CLI validates IDs/dependency acyclicity, writes canonical seed, synchronizes SQLite and does not permit owner-controlled law editing. New tasks require strong custom acceptance criteria; the default scaffold check is only a placeholder.
