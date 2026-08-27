//! Embedded assets served to gpui — the app's SVG icons.
//!
//! gpui's `svg()` element renders them as alpha masks tinted with the element's text
//! color, so they follow the palette like any text glyph. Bytes are compiled in so the
//! binary stays self-contained. The `win-*` caption glyphs are hand-drawn strokes
//! (copied from echo) rather than Segoe glyph codepoints so they render identically on
//! Win10/11 — and at all on Linux.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $((
                concat!("icons/", $name, ".svg"),
                include_bytes!(concat!("../icons/", $name, ".svg")),
            )),*
        ];
    };
}

icons!(
    "pause",
    "play",
    "radio",
    "rotate-ccw",
    "settings",
    "step-back",
    "step-forward",
    // Titlebar caption buttons.
    "win-close",
    "win-maximize",
    "win-minimize",
    "win-restore",
);

/// JetBrains Mono (OFL, see assets/fonts/OFL.txt), embedded because the timing tower's
/// column layout depends on these exact mono metrics — gpui matches font families by
/// exact name against the system database, so "have it installed" is not a strategy.
pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
