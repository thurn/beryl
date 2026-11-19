//! Design tokens: the spacing, type styles, sizes and colors that flat-layer screens are built
//! from (docs/beryl-3d.md, "Elements": styles come from design tokens). The build script writes
//! them from `data/design-tokens.json`, the file that `just design-tokens` also turns into the
//! web interface's CSS variables (`ui/src/design/tokens.css`). Every token of kind `color`
//! there is a constant of [`color`], and the file's `flat` object holds what only the flat layer
//! uses. A new value is added by editing that file alone. A token's `note` is its doc comment,
//! written as prose for the CSS comment too, so clippy's `doc_markdown` is allowed on the
//! generated modules.

/// Box shadows and drop shadows, each a list as CSS's `box-shadow` or a `filter`'s
/// `drop-shadow()`s give it.
#[allow(clippy::doc_markdown)]
pub mod shadow {
    use crate::element::Shadow;

    include!(concat!(env!("OUT_DIR"), "/tokens/shadow.rs"));
}

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

/// Numbers that are not sizes in points: the share a small button keeps, a quarter to round a
/// corner by, percent and the nanoseconds in a millisecond.
#[allow(clippy::doc_markdown, clippy::unreadable_literal)]
pub mod ratio {
    include!(concat!(env!("OUT_DIR"), "/tokens/ratio.rs"));
}

/// Colors as RGBA, not premultiplied: each token of kind `color`, named for it in upper case
/// (`--surface-raised` is `SURFACE_RAISED`), then the flat layer's own (`PANEL`, `BUTTON`), then
/// the effects' colors, which the web interface writes in `ui/src/stage/effects.css` rather than
/// as tokens.
#[allow(clippy::doc_markdown)]
pub mod color {
    include!(concat!(env!("OUT_DIR"), "/tokens/color.rs"));

    /// The light of an arrow or line, by what it says (UI Conventions, exception 4), from the
    /// web stage's `TONES` (`ui/src/stage/light.ts`): green at a target its chooser controls
    /// and red at one they do not, cyan an attack's line, gold a block's, and pale the line from
    /// an ability to its source.
    pub const FRIENDLY_TARGET: [u8; 4] = [20, 235, 128, 255];
    pub const HOSTILE_TARGET: [u8; 4] = [255, 43, 26, 255];
    pub const ATTACK_LINE: [u8; 4] = [26, 214, 242, 255];
    pub const BLOCK_LINE: [u8; 4] = [255, 168, 26, 255];
    pub const SOURCE_LINE: [u8; 4] = [219, 184, 117, 255];

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

    /// The halo of the `glow` effect: the warm outer shadow of `.stage-effect-glow` in
    /// `ui/src/stage/effects.css`, at its 75% opacity.
    pub const GLOW: [u8; 4] = [255, 214, 140, 191];
    /// The burst of the `impact` effect: the middle stop of `.stage-effect-impact`'s gradient.
    pub const IMPACT: [u8; 4] = [255, 196, 120, 255];
    /// The flare of the `flash` effect: the `--light` of `.stage-effect-flash` at 85% opacity.
    pub const FLASH: [u8; 4] = [255, 246, 222, 217];
    /// The cone of the `beam` effect: white warmed toward yellow by `warm` 0.35 in `beamLight`
    /// (`ui/src/stage/flare.ts`).
    pub const BEAM: [u8; 4] = [255, 238, 190, 255];
    /// The ring of the `pulse` effect: the outer shadow of `.stage-effect-pulse`, at its 80%
    /// opacity.
    pub const PULSE: [u8; 4] = [255, 226, 170, 204];
    /// The streak of the `trail` effect: the `--light` of `.stage-effect`, white light.
    pub const TRAIL: [u8; 4] = [255, 246, 222, 255];

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
    /// The burst of light over a permanent that lands (`settle`): the glow of the
    /// `.stage-effect-settle` rule's `box-shadow` in `ui/src/stage/effects.css`.
    pub const SETTLE_BURST: [u8; 4] = [255, 226, 170, 217];
}

/// The colors of mana symbols in text (`text::symbols`): each symbol's disc, and the ink of its
/// glyph, from `flat.mana`.
#[allow(clippy::doc_markdown)]
pub mod mana {
    include!(concat!(env!("OUT_DIR"), "/tokens/mana.rs"));
}

/// The colors of the badges on a permanent (`table::badges`), from the web interface's CSS
/// rules for the same marks in `ui/src/design/components`. Each is RGBA, not premultiplied.
pub mod badge {
    /// The plaque's fill, by the card's frame: the `--bar` of `.frame-w`, `.frame-u`,
    /// `.frame-b`, `.frame-r` and `.frame-g` in `card-palettes.css`, then `.frame-m` for a
    /// card of two or more colors and `.frame-c` for a colorless one.
    pub const PLAQUE_WHITE: [u8; 4] = [0xe2, 0xdf, 0xda, 0xff];
    pub const PLAQUE_BLUE: [u8; 4] = [0xb9, 0xd3, 0xea, 0xff];
    pub const PLAQUE_BLACK: [u8; 4] = [0xb4, 0xad, 0xa8, 0xff];
    pub const PLAQUE_RED: [u8; 4] = [0xe0, 0xb4, 0xa0, 0xff];
    pub const PLAQUE_GREEN: [u8; 4] = [0xb6, 0xbc, 0xb0, 0xff];
    pub const PLAQUE_GOLD: [u8; 4] = [0xda, 0xb8, 0x72, 0xff];
    pub const PLAQUE_COLORLESS: [u8; 4] = [0xd0, 0xc7, 0xbc, 0xff];
    /// The plaque's border, `.card-pt`'s `border` in `card-frame.css`.
    pub const PLAQUE_BORDER: [u8; 4] = [0x14, 0x11, 0x0f, 0xff];
    /// A number on the plaque that reads as printed, `.card-pt b`'s `--pt-ink`.
    pub const PLAQUE_INK: [u8; 4] = [0x19, 0x21, 0x3a, 0xff];
    /// A counter badge's fill, the middle of `.mark-badge`'s gradient in `card-frame.css`.
    pub const COUNTER: [u8; 4] = [0x2b, 0x28, 0x24, 0xff];
    /// The pile count's fill, the middle of `.card-count`'s gradient in
    /// `card-marks-and-piles.css`, and its border and ink.
    pub const COUNT: [u8; 4] = [0xec, 0xda, 0xb9, 0xff];
    pub const COUNT_BORDER: [u8; 4] = [0xb8, 0x92, 0x4e, 0xff];
    pub const COUNT_INK: [u8; 4] = [0x1d, 0x1b, 0x19, 0xff];
    /// The dark ring around each symbol of a card's cost, `.mana-cost .mana-symbol`'s
    /// `box-shadow` in `mana-symbols.css`.
    pub const COST_RING: [u8; 4] = [0x10, 0x16, 0x1c, 0xff];
    /// The panel under a permanent's note or chosen value, `.card-note`'s `background` in
    /// `card-frame.css`, whose text is `color::TYPE`.
    pub const NOTE: [u8; 4] = [0x1c, 0x1a, 0x17, 0xe6];
    /// A copy's "Copy" label, `.token-label` in `icon-button.css`: its fill, border and ink.
    pub const COPY: [u8; 4] = [0x42, 0x35, 0x24, 0xff];
    pub const COPY_BORDER: [u8; 4] = [0xc9, 0xb9, 0x8c, 0xff];
    pub const COPY_INK: [u8; 4] = [0xff, 0xf4, 0xd6, 0xff];
}
