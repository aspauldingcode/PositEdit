//! F08 rich preview descriptors. Host owns the graphics arbiter.

use crate::SpatialCapabilities;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Svg,
    Image,
    Markdown,
    Latex,
    PdfPage,
    Framebuffer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRequest {
    pub kind: PreviewKind,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceDescriptor {
    pub kind: PreviewKind,
    pub backend: &'static str,
    pub fallback_text: String,
    pub path: String,
}

pub fn describe(kind: PreviewKind, caps: &SpatialCapabilities) -> SurfaceDescriptor {
    let backend = if caps.kitty_graphics {
        "kitty"
    } else if caps.sixel {
        "sixel"
    } else if caps.iterm2_inline {
        "iterm2"
    } else {
        "text"
    };
    let fallback_text = match kind {
        PreviewKind::Svg => "[svg preview]".into(),
        PreviewKind::Image => "[image preview]".into(),
        PreviewKind::Markdown => "[markdown preview]".into(),
        PreviewKind::Latex => "[latex extract]".into(),
        PreviewKind::PdfPage => "[pdf page]".into(),
        PreviewKind::Framebuffer => "[framebuffer]".into(),
    };
    SurfaceDescriptor {
        kind,
        backend,
        fallback_text,
        path: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fallback_without_protocols() {
        let caps = SpatialCapabilities::default();
        let surface = describe(PreviewKind::Image, &caps);
        assert_eq!(surface.backend, "text");
    }
}
