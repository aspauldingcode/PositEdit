---
name: lsp-dap
description: Correct implementation of two different protocols
---

# Correct implementation of two different protocols

LSP uses JSON-RPC 2.0 and Content-Length; DAP uses its own request-response-event messages with Content-Length. Validate lifecycle and capability negotiation. Run real and mock server tests. Keep executor external.

**Procedure:** inspect actual environment -> identify contracts and acceptance tests -> execute minimal change -> capture concrete proof/build/test evidence -> update graph -> stop or continue only if permitted by policy. Read `AGENTS.md`, `ARCHITECTURE.md` and `engineering/VERIFICATION.md`. See `REFERENCES.md` for official sources.
