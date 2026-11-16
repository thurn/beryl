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

    /// The outline of what may be picked: the `--tone` of `.stage-mark-eligible` in
    /// `ui/src/stage/stage.css`.
    pub const MARK_ELIGIBLE: [u8; 4] = [51, 220, 254, 255];
    /// The outline of what has been picked: the `--tone` of `.stage-mark-selected`.
    pub const MARK_SELECTED: [u8; 4] = [255, 205, 96, 255];
    /// The ring round what the offered default would use: the border of `.stage-mark-suggested`,
    /// the `--ember` token.
    pub const MARK_SUGGESTED: [u8; 4] = EMBER;
    /// The tap symbol on a permanent picked to pay a cost: the `color` of
    /// `.stage-mark-paying::after`.
    pub const MARK_PAYING: [u8; 4] = [243, 234, 215, 255];
    /// The outline of a target its chooser controls: the `--tone` of
    /// `.stage-mark-friendly-target`.
    pub const MARK_FRIENDLY_TARGET: [u8; 4] = [116, 240, 136, 255];
    /// The outline of a target its chooser does not control: the `--tone` of
    /// `.stage-mark-hostile-target`.
    pub const MARK_HOSTILE_TARGET: [u8; 4] = [255, 92, 74, 255];

    /// The wedge an attacker wears: the glow of `ember` in `TONES`, the tone `fighterLight`
    /// (`ui/src/stage/light.ts`) gives it.
    pub const WEDGE: [u8; 4] = [255, 38, 10, 255];
    /// The crescent a blocker wears: the glow of `gold` in `TONES`, the tone `fighterLight`
    /// gives it.
    pub const CRESCENT: [u8; 4] = [255, 168, 26, 255];
    /// A summoning-sick permanent's moon: the `--mark` of `.mark-sick`
    /// (`ui/src/design/components/card-frame.css`).
    pub const MOON: [u8; 4] = CYAN;
    /// A token's coin: the `--mark` of `.mark-token` in the same file.
    pub const COIN: [u8; 4] = [214, 210, 204, 255];
    /// The edge of a permanent its owner does not control: the border of `.card-stolen-edge`
    /// (`ui/src/design/components/card-marks-and-piles.css`).
    pub const OUTLINE: [u8; 4] = STOLEN;
}
