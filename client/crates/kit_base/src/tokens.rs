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
    use crate::paint::Shadow;

    include!(concat!(env!("OUT_DIR"), "/tokens/shadow.rs"));
}

/// Gradients, each as CSS's `linear-gradient()` or `radial-gradient()` gives it.
#[allow(clippy::doc_markdown)]
pub mod gradient {
    use crate::paint::{Extent, Gradient, Length, Stop};

    include!(concat!(env!("OUT_DIR"), "/tokens/gradient.rs"));
}

/// Gaps and padding, in logical points.
#[allow(clippy::doc_markdown)]
pub mod space {
    include!(concat!(env!("OUT_DIR"), "/tokens/space.rs"));
}

/// Type styles.
#[allow(clippy::doc_markdown)]
pub mod type_style {
    use crate::typography::TypeStyle;
    use crate::typography::fonts::Font;

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

    /// The light of an arrow or line, by what it says: green at a friendly target and red at a
    /// hostile target (UI Conventions, exception 4), electric cyan while aiming, rose-crimson for
    /// a confirmed attack line, solar gold for a block line, and pale teal-silver for an ability's
    /// source line (matching Arena).
    pub const AIM_LINE: [u8; 4] = [20, 194, 255, 255];
    pub const FRIENDLY_TARGET: [u8; 4] = [20, 235, 128, 255];
    pub const HOSTILE_TARGET: [u8; 4] = [255, 43, 26, 255];
    pub const ATTACK_LINE: [u8; 4] = [235, 58, 78, 255];
    pub const BLOCK_LINE: [u8; 4] = [255, 168, 26, 255];
    pub const SOURCE_LINE: [u8; 4] = [136, 208, 192, 255];
    /// The hot core of each of those, which lights the middle of its ribbon.
    pub const AIM_LINE_CORE: [u8; 4] = [220, 252, 255, 255];
    pub const FRIENDLY_TARGET_CORE: [u8; 4] = [219, 255, 230, 255];
    pub const HOSTILE_TARGET_CORE: [u8; 4] = [255, 219, 179, 255];
    pub const ATTACK_LINE_CORE: [u8; 4] = [255, 224, 230, 255];
    pub const BLOCK_LINE_CORE: [u8; 4] = [255, 247, 204, 255];
    pub const SOURCE_LINE_CORE: [u8; 4] = [224, 248, 242, 255];

    /// The tint of a white streak: `white` in `TONES` (`ui/src/stage/light.ts`).
    pub const STREAK_WHITE: [u8; 4] = [255, 230, 158, 255];
    /// The tint of a blue streak: `blue` in `TONES`.
    pub const STREAK_BLUE: [u8; 4] = [64, 168, 245, 255];
    /// The tint of a black streak: `black` in `TONES`.
    pub const STREAK_BLACK: [u8; 4] = [153, 92, 214, 255];
    /// The tint of a red streak: `red` in `TONES`.
    pub const STREAK_RED: [u8; 4] = [242, 84, 43, 255];
    /// The tint of a green streak: `green` in `TONES`.
    pub const STREAK_GREEN: [u8; 4] = [64, 191, 79, 255];
    /// The tint of a colorless streak: `pale` in `TONES`, which `TONE` in
    /// `ui/src/stage/flare.ts` falls back to for a look it does not name.
    pub const STREAK_COLORLESS: [u8; 4] = [219, 184, 117, 255];
    /// The tint of a damage streak: `ember` in `TONES`, which `TONE` names for damage.
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
    /// The white-hot rim of an attacker's wedge: the core of `ember` in `TONES`.
    pub const WEDGE_HOT: [u8; 4] = [255, 204, 128, 255];
    /// The crescent a blocker wears: the glow of `gold` in `TONES`, the tone `fighterLight`
    /// gives it.
    pub const CRESCENT: [u8; 4] = [255, 168, 26, 255];
    /// Ember sparks drifting off an unblocked attacker's Enhanced chevron.
    pub const COMBAT_SPARK: [u8; 4] = [255, 206, 108, 255];
    /// Golden sparks drifting off a blocked attacker's Enhanced chevron.
    pub const COMBAT_SPARK_BLOCKED: [u8; 4] = [255, 232, 132, 220];
    /// A summoning-sick permanent's moon: the `--mark` of `.mark-sick`
    /// (`ui/src/design/components/card-frame.css`).
    pub const MOON: [u8; 4] = CYAN;
    /// A token's coin: the `--mark` of `.mark-token` in the same file.
    pub const COIN: [u8; 4] = [214, 210, 204, 255];
    /// The outer halo round a card shown away from the zone it is in: a cool violet, apart from
    /// the cyan of a card that may be played.
    pub const AWAY: [u8; 4] = [176, 140, 255, 150];
    /// The inner halo round the same card, nearly white.
    pub const AWAY_CORE: [u8; 4] = [236, 228, 255, 120];
}

/// The colors of symbol discs and their glyphs, from `flat.symbol`.
#[allow(clippy::doc_markdown)]
pub mod symbol {
    include!(concat!(env!("OUT_DIR"), "/tokens/symbol.rs"));
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
    /// A counter's disc on a permanent: Arena's grey metal disc, its darker rim and its pale
    /// number, read off Arena's table in its 1080p recordings. A +1/+1 or -1/-1 counter's number keeps
    /// the bright `color::STAT_UP` or `color::STAT_DOWN` instead.
    pub const COUNTER_DISC: [u8; 4] = [0x64, 0x68, 0x6f, 0xff];
    pub const COUNTER_DISC_RIM: [u8; 4] = [0x22, 0x24, 0x28, 0xff];
    pub const COUNTER_DISC_INK: [u8; 4] = [0xf5, 0xf3, 0xf0, 0xff];
    /// The pile count's fill, the middle of `.card-count`'s gradient in
    /// `card-marks-and-piles.css`, and its border and ink.
    pub const COUNT: [u8; 4] = [0xec, 0xda, 0xb9, 0xff];
    pub const COUNT_BORDER: [u8; 4] = [0xb8, 0x92, 0x4e, 0xff];
    pub const COUNT_INK: [u8; 4] = [0x1d, 0x1b, 0x19, 0xff];
    /// The panel under a permanent's note or chosen value, `.card-note`'s `background` in
    /// `card-frame.css`, whose text is `color::TYPE`.
    pub const NOTE: [u8; 4] = [0x1c, 0x1a, 0x17, 0xe6];
    /// A copy's "Copy" label, `.token-label` in `icon-button.css`: its fill, border and ink.
    pub const COPY: [u8; 4] = [0x42, 0x35, 0x24, 0xff];
    pub const COPY_BORDER: [u8; 4] = [0xc9, 0xb9, 0x8c, 0xff];
    pub const COPY_INK: [u8; 4] = [0xff, 0xf4, 0xd6, 0xff];
}
