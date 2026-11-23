//! Live terminal palette via OSC 10/11/4 — so Kitty minimap RGB tracks theme changes.
//!
//! Env vars like `COLORFGBG` do not update when Ghostty/Kitty toggle light/dark or
//! swap a color scheme, so we query the tty and fingerprint the reply.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::minimap_gfx::MinimapTheme;

/// Resolved default + ANSI (0–15) colors from the host terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermPalette {
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    pub ansi: [(u8, u8, u8); 16],
}

impl TermPalette {
    pub fn fingerprint(self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        let mut absorb = |rgb: (u8, u8, u8)| {
            for b in [rgb.0, rgb.1, rgb.2] {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        absorb(self.fg);
        absorb(self.bg);
        for c in self.ansi {
            absorb(c);
        }
        h
    }

    pub fn is_light(self) -> bool {
        perceived_lightness(self.bg) > perceived_lightness(self.fg)
            || perceived_lightness(self.bg) > 0.5
    }

    pub fn to_minimap_theme(self) -> MinimapTheme {
        let light = self.is_light();
        MinimapTheme {
            fingerprint: self.fingerprint(),
            bg: self.bg,
            fg: self.fg,
            // Match `token_style` ANSI indices → live palette RGB (1:1 with buffer ink).
            digit: self.ansi[3],    // Yellow-ish numbers often; keep Yellow channel
            string: self.ansi[2],   // Green
            punct: self.fg,
            comment: self.ansi[3],  // Yellow
            ident: self.ansi[6],    // Cyan (types/idents read as cyan-ish in buffer)
            keyword: self.ansi[5],  // Magenta
            function: self.ansi[4], // Blue
            markup: self.ansi[8],   // Bright black / DarkGray
            view_delta: if light { -18 } else { 28 },
        }
    }
}

/// Probe OSC 10/11 + ANSI 0–15. Falls back to env heuristic when the host is silent.
pub fn probe(timeout: Duration) -> TermPalette {
    probe_osc(timeout).unwrap_or_else(fallback_from_env)
}

/// OSC-only; `None` when the host did not answer (keep the previous theme).
pub fn probe_osc_quick() -> Option<TermPalette> {
    probe_osc(Duration::from_millis(80))
}

fn fallback_from_env() -> TermPalette {
    let theme = crate::minimap_gfx::terminal_theme_from_env();
    let mut ansi = if theme.is_light() {
        ANSI_LIGHT
    } else {
        ANSI_DARK
    };
    ansi[7] = theme.fg;
    TermPalette {
        fg: theme.fg,
        bg: theme.bg,
        ansi,
    }
}

fn probe_osc(timeout: Duration) -> Option<TermPalette> {
    let mut out = io::stdout();
    // Query fg, bg, then the 16 ANSI slots the editor's named colors resolve to.
    let mut query = Vec::new();
    query.extend_from_slice(b"\x1b]10;?\x1b\\");
    query.extend_from_slice(b"\x1b]11;?\x1b\\");
    for i in 0..16u8 {
        let _ = write!(query, "\x1b]4;{i};?\x1b\\");
    }
    // DA1 — answered by every VT; if it arrives before colors, host lacks OSC color.
    query.extend_from_slice(b"\x1b[c");
    out.write_all(&query).ok()?;
    out.flush().ok()?;

    let deadline = Instant::now() + timeout;
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 512];
    while Instant::now() < deadline {
        let remain = deadline.saturating_duration_since(Instant::now());
        if !stdin_readable(remain) {
            break;
        }
        let n = match read_stdin(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&tmp[..n]);
        let rgb_n = buf.windows(4).filter(|w| *w == b"rgb:").count();
        let saw_da1 = buf.windows(2).any(|w| w == b"\x1b[") && buf.contains(&b'c');
        // Expect fg+bg+16 ANSI = 18 rgb: replies. Stop early only when DA1 arrives
        // *and* we already have a usable set — never leave trailing OSC on stdin.
        if rgb_n >= 18 && saw_da1 {
            break;
        }
        if rgb_n >= 18 {
            // Colors complete; brief wait for DA1 then finish.
            if !stdin_readable(Duration::from_millis(5)) {
                break;
            }
        }
    }

    let parsed = parse_palette_reply(&buf);
    // Ghostty/Kitty may still be flushing OSC chunks. Anything left on stdin is
    // otherwise eaten by crossterm as KeyCode::Char and dumped into `: ` palette.
    drain_stdin_osc_noise(Duration::from_millis(40));
    parsed
}

/// Discard leftover OSC/DA1 bytes so they never become typed characters.
fn drain_stdin_osc_noise(budget: Duration) {
    let deadline = Instant::now() + budget;
    let mut tmp = [0u8; 512];
    while Instant::now() < deadline {
        if !stdin_readable(Duration::from_millis(2)) {
            // One quiet poll — if more arrives mid-drain, keep going until budget.
            if !stdin_readable(Duration::from_millis(5)) {
                break;
            }
        }
        match read_stdin(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

fn parse_palette_reply(buf: &[u8]) -> Option<TermPalette> {
    let text = String::from_utf8_lossy(buf);
    let mut fg = None;
    let mut bg = None;
    let mut ansi = if looks_light_reply(text.as_ref()) {
        ANSI_LIGHT
    } else {
        ANSI_DARK
    };
    let mut saw_rgb = false;

    // OSC 10 / 11 / 4 ; idx ; rgb:…
    let mut rest: &str = text.as_ref();
    while let Some(start) = rest.find("\u{1b}]") {
        rest = &rest[start + 2..];
        let end = rest
            .find(['\u{7}', '\u{1b}'])
            .unwrap_or(rest.len());
        let body = &rest[..end];
        rest = if end < rest.len() {
            if rest.as_bytes().get(end) == Some(&0x1b) {
                rest.get(end + 1..).unwrap_or("") // skip '\' of ST
            } else {
                &rest[end + 1..]
            }
        } else {
            ""
        };

        let mut parts = body.splitn(3, ';');
        let code = parts.next().unwrap_or("");
        match code {
            "10" => {
                if let Some(rgb) = parts.next().and_then(parse_rgb) {
                    fg = Some(rgb);
                    saw_rgb = true;
                }
            }
            "11" => {
                if let Some(rgb) = parts.next().and_then(parse_rgb) {
                    bg = Some(rgb);
                    saw_rgb = true;
                }
            }
            "4" => {
                let idx = parts.next().and_then(|s| s.parse::<usize>().ok());
                let rgb = parts.next().and_then(parse_rgb);
                if let (Some(i), Some(rgb)) = (idx, rgb) {
                    if i < 16 {
                        ansi[i] = rgb;
                        saw_rgb = true;
                    }
                }
            }
            _ => {}
        }
    }

    if !saw_rgb {
        return None;
    }
    let fg = fg.unwrap_or(ansi[7]);
    let bg = bg.unwrap_or(ansi[0]);
    Some(TermPalette { fg, bg, ansi })
}

fn looks_light_reply(text: &str) -> bool {
    // Rough: if we already parsed nothing, unused. Prefer dark defaults.
    text.contains("rgb:f") || text.contains("rgb:e")
}

/// Parse `rgb:RRRR/GGGG/BBBB` (1–4 hex digits per channel) → 8-bit.
pub fn parse_rgb(spec: &str) -> Option<(u8, u8, u8)> {
    let spec = spec.trim();
    let body = spec.strip_prefix("rgb:")?;
    let mut parts = body.split('/');
    let r = scale_hex(parts.next()?)?;
    let g = scale_hex(parts.next()?)?;
    let b = scale_hex(parts.next()?)?;
    Some((r, g, b))
}

fn scale_hex(component: &str) -> Option<u8> {
    let v = u32::from_str_radix(component, 16).ok()?;
    let out = match component.len() {
        1 => (v * 0x11) as u8,
        2 => v as u8,
        3 => (v >> 4) as u8,
        4 => (v >> 8) as u8,
        _ => return None,
    };
    Some(out)
}

fn perceived_lightness(rgb: (u8, u8, u8)) -> f32 {
    // Same relative-luminance idea colorsaurus uses (sRGB → Y → L*).
    fn lin(c: u8) -> f32 {
        let x = f32::from(c) / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }
    let y = 0.2126 * lin(rgb.0) + 0.7152 * lin(rgb.1) + 0.0722 * lin(rgb.2);
    let threshold = (6.0_f32 / 29.0).powi(3);
    if y <= threshold {
        y * (29.0_f32 / 3.0).powi(3) / 100.0
    } else {
        (y.cbrt() * 116.0 - 16.0) / 100.0
    }
}

fn stdin_readable(timeout: Duration) -> bool {
    #[cfg(unix)]
    {
        let fd = libc::STDIN_FILENO;
        let mut fds = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        unsafe { libc::poll(&mut fds, 1, ms) > 0 && fds.revents & libc::POLLIN != 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = timeout;
        false
    }
}

fn read_stdin(buf: &mut [u8]) -> io::Result<usize> {
    #[cfg(unix)]
    {
        let n = unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
    #[cfg(not(unix))]
    {
        io::stdin().read(buf)
    }
}

/// Classic xterm dark palette (approx).
const ANSI_DARK: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xcd, 0x00, 0x00),
    (0x00, 0xcd, 0x00),
    (0xcd, 0xcd, 0x00),
    (0x00, 0x00, 0xee),
    (0xcd, 0x00, 0xcd),
    (0x00, 0xcd, 0xcd),
    (0xe5, 0xe5, 0xe5),
    (0x7f, 0x7f, 0x7f),
    (0xff, 0x00, 0x00),
    (0x00, 0xff, 0x00),
    (0xff, 0xff, 0x00),
    (0x5c, 0x5c, 0xff),
    (0xff, 0x00, 0xff),
    (0x00, 0xff, 0xff),
    (0xff, 0xff, 0xff),
];

/// Light-host ANSI (darker inks on cream).
const ANSI_LIGHT: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xa4, 0x00, 0x00),
    (0x00, 0x8a, 0x00),
    (0x9a, 0x6b, 0x00),
    (0x00, 0x00, 0xbf),
    (0x9a, 0x00, 0x9a),
    (0x00, 0x8a, 0x8a),
    (0x40, 0x40, 0x40),
    (0x66, 0x66, 0x66),
    (0xcf, 0x00, 0x00),
    (0x00, 0xa8, 0x00),
    (0xb8, 0x80, 0x00),
    (0x00, 0x00, 0xef),
    (0xbf, 0x00, 0xbf),
    (0x00, 0xa8, 0xa8),
    (0x1a, 0x1a, 0x1a),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_osc_rgb_variants() {
        assert_eq!(parse_rgb("rgb:ffff/0000/0000"), Some((255, 0, 0)));
        assert_eq!(parse_rgb("rgb:ff/00/00"), Some((255, 0, 0)));
        assert_eq!(parse_rgb("rgb:f/0/0"), Some((255, 0, 0)));
        assert_eq!(parse_rgb("rgb:1e1e/1e1e/2424"), Some((0x1e, 0x1e, 0x24)));
    }

    #[test]
    fn parse_palette_reply_extracts_fg_bg_ansi() {
        let raw = b"\x1b]10;rgb:cccc/cccc/cccc\x1b\\\x1b]11;rgb:1e1e/1e1e/2424\x1b\\\x1b]4;2;rgb:0000/cdcd/0000\x1b\\\x1b[?1;2c";
        let p = parse_palette_reply(raw).expect("palette");
        assert_eq!(p.fg, (0xcc, 0xcc, 0xcc));
        assert_eq!(p.bg, (0x1e, 0x1e, 0x24));
        assert_eq!(p.ansi[2], (0x00, 0xcd, 0x00));
        assert!(!p.is_light());
        let theme = p.to_minimap_theme();
        assert_eq!(theme.string, (0x00, 0xcd, 0x00));
        assert_eq!(theme.bg, p.bg);
    }

    #[test]
    fn fingerprint_changes_when_bg_changes() {
        let a = TermPalette {
            fg: (200, 200, 200),
            bg: (30, 30, 36),
            ansi: ANSI_DARK,
        };
        let mut b = a;
        b.bg = (250, 245, 230);
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn parse_ignores_partial_leak_shaped_text() {
        // What the user saw in `: ` when OSC leftovers were typed as chars.
        let leak = b"b8b8/bbbb/26264;3;rgb:fafa/bdbd/2f2f4;4;rgb:8383/a5a5/9898";
        assert!(parse_palette_reply(leak).is_none());
    }
}
