# Illustrative host-facing Rust interfaces (adapt to real host)

```rust
pub trait SpatialService: Send + Sync {
    fn capabilities(&self) -> SpatialCapabilities;
    fn dispatch(&self, action: SpatialAction, ctx: &SpatialContext) -> SpatialOutcome;
    fn cancel(&self, request_id: RequestId);
}

pub trait GraphicsArbiter {
    fn submit(&mut self, surface: SurfaceDescriptor) -> Result<SurfaceId, GraphicsError>;
    fn update(&mut self, id: SurfaceId, damage: DamageRegion) -> Result<(), GraphicsError>;
    fn remove(&mut self, id: SurfaceId);
}
```

`SpatialContext` carries host pane ID, document revision, workspace trust and cancellation. UI effects return declarative host actions, never raw terminal escape sequences. Version contracts and implement adapters instead of replacing existing host APIs.
