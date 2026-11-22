//! Host-owned graphics arbiter. Only the host terminal writer emits escapes.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphicsBackend {
    #[default]
    Text,
    Kitty,
    Sixel,
    ITerm2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceId(pub u64);

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ManagedSurface {
    pub id: SurfaceId,
    pub owner_pane: usize,
    pub backend: GraphicsBackend,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub revision: u64,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct GraphicsArbiter {
    next_id: u64,
    surfaces: HashMap<u64, ManagedSurface>,
    probed: bool,
    backend: GraphicsBackend,
}

impl GraphicsArbiter {
    pub fn probe_env(term: Option<&str>, term_program: Option<&str>) -> GraphicsBackend {
        let term = term.unwrap_or("").to_ascii_lowercase();
        let program = term_program.unwrap_or("").to_ascii_lowercase();
        // Ghostty speaks the Kitty graphics protocol for the dedicated Canvas.
        if program.contains("ghostty")
            || program.contains("kitty")
            || term.contains("xterm-kitty")
            || term.contains("ghostty")
            || std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
            || std::env::var_os("KITTY_WINDOW_ID").is_some()
        {
            GraphicsBackend::Kitty
        } else if program.contains("iterm") || std::env::var_os("ITERM_SESSION_ID").is_some() {
            GraphicsBackend::ITerm2
        } else if term.contains("mlterm") || std::env::var_os("TERM_PROGRAM").is_none() && false {
            GraphicsBackend::Sixel
        } else {
            GraphicsBackend::Text
        }
    }

    pub fn ensure_probed(&mut self) {
        let env = Self::probe_env(
            std::env::var("TERM").ok().as_deref(),
            std::env::var("TERM_PROGRAM").ok().as_deref(),
        );
        if self.probed {
            // Allow Text → Kitty upgrade (nested launch / late env), never downgrade.
            if self.backend == GraphicsBackend::Text && env == GraphicsBackend::Kitty {
                self.backend = GraphicsBackend::Kitty;
            }
            return;
        }
        self.backend = env;
        self.probed = true;
    }

    pub fn backend(&mut self) -> GraphicsBackend {
        self.ensure_probed();
        self.backend
    }

    /// Force Kitty when the host speaks the protocol (minimap/Canvas share this).
    pub fn prefer_kitty_from_env(&mut self) {
        self.ensure_probed();
        if Self::probe_env(
            std::env::var("TERM").ok().as_deref(),
            std::env::var("TERM_PROGRAM").ok().as_deref(),
        ) == GraphicsBackend::Kitty
        {
            self.backend = GraphicsBackend::Kitty;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &mut self,
        owner_pane: usize,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        revision: u64,
        label: impl Into<String>,
    ) -> SurfaceId {
        self.ensure_probed();
        self.next_id += 1;
        let id = SurfaceId(self.next_id);
        self.surfaces.insert(
            id.0,
            ManagedSurface {
                id: id.clone(),
                owner_pane,
                backend: self.backend,
                x,
                y,
                width,
                height,
                revision,
                label: label.into(),
            },
        );
        id
    }

    #[allow(dead_code)]
    pub fn clip_to_pane(
        &mut self,
        id: SurfaceId,
        pane_x: u16,
        pane_y: u16,
        pane_w: u16,
        pane_h: u16,
    ) {
        if let Some(surface) = self.surfaces.get_mut(&id.0) {
            let right = surface
                .x
                .saturating_add(surface.width)
                .min(pane_x.saturating_add(pane_w));
            let bottom = surface
                .y
                .saturating_add(surface.height)
                .min(pane_y.saturating_add(pane_h));
            surface.x = surface.x.max(pane_x);
            surface.y = surface.y.max(pane_y);
            surface.width = right.saturating_sub(surface.x);
            surface.height = bottom.saturating_sub(surface.y);
        }
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, id: SurfaceId) {
        self.surfaces.remove(&id.0);
    }

    #[allow(dead_code)]
    pub fn remove_owner(&mut self, owner_pane: usize) {
        self.surfaces
            .retain(|_, surface| surface.owner_pane != owner_pane);
    }

    pub fn clear(&mut self) {
        self.surfaces.clear();
    }

    pub fn text_fallback(&self, id: SurfaceId) -> Option<String> {
        self.surfaces.get(&id.0).map(|s| match s.backend {
            GraphicsBackend::Text => format!("{} ({})", s.label, "text"),
            other => format!("{} ({other:?})", s.label),
        })
    }

    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        self.surfaces.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_and_cleanup() {
        assert_eq!(
            GraphicsArbiter::probe_env(Some("xterm-kitty"), Some("kitty")),
            GraphicsBackend::Kitty
        );
        assert_eq!(
            GraphicsArbiter::probe_env(Some("xterm-ghostty"), Some("ghostty")),
            GraphicsBackend::Kitty
        );
        assert_eq!(
            GraphicsArbiter::probe_env(Some("xterm-256color"), Some("Apple_Terminal")),
            GraphicsBackend::Text
        );
        let mut arbiter = GraphicsArbiter::default();
        arbiter.backend = GraphicsBackend::Text;
        arbiter.probed = true;
        let id = arbiter.submit(0, 1, 1, 10, 5, 1, "img");
        arbiter.clip_to_pane(id.clone(), 0, 0, 4, 4);
        assert_eq!(arbiter.surfaces[&id.0].width, 3);
        arbiter.remove_owner(0);
        assert_eq!(arbiter.count(), 0);
    }
}
