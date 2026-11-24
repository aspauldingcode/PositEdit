# System architecture

## Host boundaries
Existing host TUI owns terminal raw mode, event loop, ratatui frame scheduling, pane geometry, scrolling, animations, focus and input routing. DLC4 contributes optional stateful services via contracts. No parallel terminal writers. All view output is composed into the host's frame/graphics compositor. Use capability detection at runtime; do not assume Kitty, Sixel, iTerm2 or cell-pixel dimensions.

## Layers
1. Host shell: niri-style scrollview; layout/focus/animation unchanged.
2. Spatial context: pane graph, viewport overview, semantic pane relationships, workspace snapshots.
3. Intelligence: Tree-sitter, LSP, project graph, Git, tests, coverage, profiler, source mapping.
4. Observability: DAP variable snapshots, memory probe adapters, structured task output, execution trace providers.
5. Rich media: one graphics arbitration service; surface lifetime, clipping, resize, damage, input, fallback.
6. Ephemeral lenses: modal overlays managed by host; restore focus, scroll and keyboard mode on exit.
7. Policies: Rust implementations initially, optional Bend 2 pure policies behind versioned contract; verify ABI/toolchain.

## Dataflow
Editor event -> host dispatcher -> bounded DLC4 service request (document revision, pane ID, cancellation token) -> immutable result -> revision validation -> host view model -> host frame renderer. Slow tasks never block typing or scroll animation. Trace and image services only activate when requested.

## Integration strategy
Discover host crate boundaries and adapt; DO NOT assume exact paths. Provide `SpatialService` trait and `SpatialAction` command entries; use host-owned `PaneId`, `DocumentId`, `RevisionId` adapters. Existing renderer must own pixel graphics; graphics adapters return surface descriptors rather than writing escape codes independently. Maintain extension capability registry with graceful unavailable states.

## Contracts
`PaneSnapshot`, `SpatialOverview`, `SourceAnchor`, `DiagnosticBlock`, `TraceEvent`, `ProbeSnapshot`, `GraphicsSurface`, `LensSession`, `ChangeImpact`, `WorkspaceSnapshot` are versioned. See `contracts/`. External tool output is untrusted, never interpreted as agent instructions.

## Platform portability
macOS Apple Silicon first, NixOS/Linux second, Windows future. Use optional backends for platform-specific trace, GPU, process monitoring. Never infer availability from OS alone; probe toolchains, terminal capabilities, user permissions and runtime support.
