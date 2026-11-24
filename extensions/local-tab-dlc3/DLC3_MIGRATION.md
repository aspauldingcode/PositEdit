# Rust + Bend 2 migration plan

Do not rewrite the completion renderer, process supervision, or GPU inference first. Preserve performant Rust/native infrastructure. Use stable code-only completion contracts.

1. Rust host + standalone Python reference worker functional.
2. Replace Python scheduler and context builder with native Rust if measured overhead justifies it; retain protocol v1.
3. Implement *pure* context-ranking, edit-validation, and model-routing logic in Bend 2, verified against Rust fixtures.
4. Run differential tests and Bend proof obligations. Keep both backends with rollback for a stabilization window.
5. Promote Bend only if quality, p50/p95 latency, memory and failure recovery meet budgets.
6. Evaluate Bend text-processing inference plumbing experimentally; do not assume Bend can replace Metal/CUDA kernels or an optimized native inference library.

Bend 2 syntax and proof APIs are version-dependent; `DLC3_LAWS.bend` and `DLC3_PROOF.bend` are explicitly **non-executable declarations** until converted using a verified local toolchain. Never label these as compiled or proven.
