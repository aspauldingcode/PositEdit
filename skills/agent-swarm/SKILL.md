---
name: agent-swarm
description: Bounded multiagent execution and context control
---

# Bounded multiagent execution and context control

Use isolated worktrees for parallel nodes, file ownership and conflict checks; one orchestrator at a time; no recursive new agent CLI invocations from child task. Use task-local evidence and budgets, checkpoint on interruption.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
