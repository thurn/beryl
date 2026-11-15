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
/// (`--surface-raised` is `SURFACE_RAISED`), then the flat layer's own (`PANEL`, `BUTTON`).
#[allow(clippy::doc_markdown)]
pub mod color {
    include!(concat!(env!("OUT_DIR"), "/tokens/color.rs"));
}
