# Accepted IDE Laws (human-readable; owner-controlled)

These are invariant requirements. The agent can implement/prove/test, but must not weaken or delete laws. When adding formal Bend statements, propose new declarations in `engineering/proposals/` for separate acceptance. Current `LAWS.bend` models **only orchestrated job cancellation**. All remaining statements here are engineering requirements and must not be represented as mathematically proved until properly modeled and proven.

1. Edits are atomic; rejected transactions cannot mutate the buffer.
2. Every accepted transaction increments the document revision; undo is a new state transition, not an untracked mutation.
3. Stale asynchronous responses cannot overwrite newer authoritative state.
4. Cancellation of queued/running jobs reaches Cancelled; cancellation is idempotent; completed work stays completed.
5. The editor remains usable when tools/servers/MCP are absent.
6. The terminal is restored after ordinary exit, cancellation or panic where recoverable.
7. Language/debug capabilities are negotiated; unsupported features must be disclosed rather than faked.
8. An agent may not access arbitrary host secrets, publish, push, provision paid resources, or disable security controls without authorization.
9. Accepted formal requirements cannot be deleted, weakened, or altered by implementation agents.
10. Passing Bend proofs never substitutes for actual Rust conformance tests or real TUI integration tests.
11. Builds and tests must log real exit status; absent tests are not successes.
12. Graceful termination, bounded retries and deterministic recovery are core behaviors.
13. Unknown languages fall back to regex/plain text without crashing or misrepresenting semantic analysis.
14. UI actions must work on every target terminal; active panels cannot be merely decorative.
15. Idle memory, startup, code size and latency are tracked against budgets without compromising correctness.
