---
name: graph-engineering
description: Persistent requirements/tasks/evidence DAG
---

# Persistent requirements/tasks/evidence DAG

Read graph.sqlite and seed.json; represent dependency, implementation, proof, risk and evidence edges. Choose highest-priority ready task. Bound retry count and agent fanout, reopen invalidated checks. Save durable logs and checkpoint each invocation.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
