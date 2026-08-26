//! The interval dark palette, ported from `frontend/tailwind.config.ts` and the literal
//! hexes in `frontend/src/styles.css` / component classes. The app is dark-only, so these
//! are plain consts rather than a theme system. `fn() -> Hsla` consts (echo's convention)
//! because gpui's `Hsla` construction isn't const-evaluable.

use gpui::{Hsla, Rgba};

fn hex(rgb: u32) -> Hsla {
    Rgba {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

// Core tokens. Originally ported from `frontend/tailwind.config.ts`; the desktop app
// has since moved to a greyscale scheme — the web frontend's mint/lime accents are
// retired, and only functional colors (amber warnings, red danger, team/tyre colors)
// stay chromatic.
pub const CARBON: fn() -> Hsla = || hex(0x111418); // window background
pub const PANEL: fn() -> Hsla = || hex(0x191d23);
pub const PANEL_HI: fn() -> Hsla = || hex(0x232a32);
pub const LINE: fn() -> Hsla = || hex(0x3a424d);
/// The interactive accent: steel grey (was the web frontend's mint).
pub const ACCENT: fn() -> Hsla = || hex(0xaeb7c2);
pub const AMBER: fn() -> Hsla = || hex(0xf3d24f);
pub const DANGER: fn() -> Hsla = || hex(0xff4b4b);
/// Timing values (gaps, lap times): near-white (was the web frontend's lime).
pub const TIMING: fn() -> Hsla = || hex(0xdde3ea);

// Literal hexes carried over from component markup.
pub const BAR_BG: fn() -> Hsla = || hex(0x15191f); // selector/controls bars
pub const BAR_BG_DEEP: fn() -> Hsla = || hex(0x101419);
pub const STINT_CARD: fn() -> Hsla = || hex(0x151a20);
pub const TIMING_HEADER: fn() -> Hsla = || hex(0x20262e);
pub const TEXT: fn() -> Hsla = || hex(0xf7fbff);
pub const LEADER_HALO: fn() -> Hsla = || hex(0xf5d547);

// Track map strokes, darkest casing to lightest hairline.
pub const MAP_CASING: fn() -> Hsla = || hex(0x20262c);
pub const MAP_ROAD: fn() -> Hsla = || hex(0x3a4250);
pub const MAP_MID: fn() -> Hsla = || hex(0x5b6571);
pub const MAP_HAIRLINE: fn() -> Hsla = || hex(0x7b8794);

// System convention, not themed: caption close-button hover (echo's CLOSE_RED).
pub const CLOSE_RED: fn() -> Hsla = || hex(0xe81123);

/// Alpha-composites `color` at `alpha` over `under`, exactly as gpui does when painting a
/// translucent quad: plain sRGB-channel interpolation. Used to turn the frontend's CSS
/// opacity washes into concrete opaque colors.
pub fn blend(color: Hsla, under: Hsla, alpha: f32) -> Hsla {
    let c = Rgba::from(color);
    let u = Rgba::from(under);
    Rgba {
        r: c.r * alpha + u.r * (1.0 - alpha),
        g: c.g * alpha + u.g * (1.0 - alpha),
        b: c.b * alpha + u.b * (1.0 - alpha),
        a: 1.0,
    }
    .into()
}

/// Parse a `team_colour` hex string from the API (`"3671C6"`, optionally `#`-prefixed)
/// into a concrete color; falls back to the hairline grey on malformed input.
pub fn team_colour(raw: &str) -> Hsla {
    u32::from_str_radix(raw.trim_start_matches('#'), 16)
        .map(hex)
        .unwrap_or_else(|_| MAP_HAIRLINE())
}
