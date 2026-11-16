//! Design tokens: the spacing, type styles, sizes and colors that flat-layer screens are built
//! from (docs/beryl-3d.md, "Elements": styles come from design tokens). The build script writes
//! them from `data/design-tokens.json`, the file that `just design-tokens` also turns into the
//! web interface's CSS variables (`ui/src/design/tokens.css`). Every token of kind `color`
//! there is a constant of [`color`], and the file's `flat` object holds what only the flat layer
//! uses. A new value is added by editing that file alone. A token's `note` is its doc comment,
//! written as prose for the CSS comment too, so clippy's `doc_markdown` is allowed on the
//! generated modules.

/// Gaps and padding, in logical points.
#[allow(clippy::doc_markdown)]
pub mod space {
    include!(concat!(env!("OUT_DIR"), "/tokens/space.rs"));
}

/// Type styles.
#[allow(clippy::doc_markdown)]
pub mod type_style {
    use crate::text::TypeStyle;
    use crate::text::fonts::Font;

    include!(concat!(env!("OUT_DIR"), "/tokens/type_style.rs"));
}

/// Fixed sizes, in logical points.
#[allow(clippy::doc_markdown)]
pub mod size {
    include!(concat!(env!("OUT_DIR"), "/tokens/size.rs"));
}

/// Colors as RGBA, not premultiplied: each token of kind `color`, named for it in upper case
/// (`--surface-raised` is `SURFACE_RAISED`), then the flat layer's own (`PANEL`, `BUTTON`), then
/// the effects' colors, which the web interface writes in `ui/src/stage/effects.css` rather than
/// as tokens.
#[allow(clippy::doc_markdown)]
pub mod color {
    include!(concat!(env!("OUT_DIR"), "/tokens/color.rs"));

    /// A life total that falls: the `--flare` of the `[data-life] > .stage-effect` rule.
    pub const LOSS: [u8; 4] = [255, 106, 85, 255];
    /// A life total that rises: the `--flare` of the `[data-life] > .stage-effect-gain` rule.
    pub const GAIN: [u8; 4] = [127, 224, 138, 255];

    /// The glow of a streak of white mana: `white` in `TONES` (`ui/src/stage/light.ts`).
    pub const MANA_WHITE: [u8; 4] = [255, 230, 158, 255];
    /// The glow of a streak of blue mana: `blue` in `TONES`.
    pub const MANA_BLUE: [u8; 4] = [64, 168, 245, 255];
    /// The glow of a streak of black mana: `black` in `TONES`.
    pub const MANA_BLACK: [u8; 4] = [153, 92, 214, 255];
    /// The glow of a streak of red mana: `red` in `TONES`.
    pub const MANA_RED: [u8; 4] = [242, 84, 43, 255];
    /// The glow of a streak of green mana: `green` in `TONES`.
    pub const MANA_GREEN: [u8; 4] = [64, 191, 79, 255];
    /// The glow of a streak of colorless mana: `pale` in `TONES`, which `TONE` in
    /// `ui/src/stage/flare.ts` falls back to for a look it does not name.
    pub const MANA_COLORLESS: [u8; 4] = [219, 184, 117, 255];
    /// The glow of a streak of damage: `ember` in `TONES`, which `TONE` names for damage.
    pub const DAMAGE: [u8; 4] = [255, 38, 10, 255];
}
