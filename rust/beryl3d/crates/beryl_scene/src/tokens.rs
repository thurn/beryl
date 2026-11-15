//! Design tokens: the spacing, type styles, sizes and colors that flat-layer screens are built
//! from (docs/beryl-3d.md, "Elements": styles come from design tokens). The values are read off
//! the web interface's CSS variables, `ui/src/design/tokens.css` and the dock's rules in
//! `ui/src/screens/styles/controls.css`. They are plain constants so that a generated module
//! can replace this one, changing only the values.

/// Gaps and padding, in logical points.
pub mod space {
    pub const S: f32 = 4.0;
    /// `--button-gap`.
    pub const M: f32 = 10.0;
    pub const L: f32 = 16.0;
}

/// Type styles.
pub mod type_style {
    use crate::text::TypeStyle;
    use crate::text::fonts::Font;

    /// A prompt's question: the dock's `.prompt-title`, 26 points at weight 600, line height
    /// 1.15.
    pub const TITLE: TypeStyle = TypeStyle {
        font: Font::CrimsonPro,
        weight: 600,
        size: 26.0,
        line_height: 1.15,
    };
    /// Running interface text.
    pub const BODY: TypeStyle = TypeStyle {
        font: Font::OpenSans,
        weight: 400,
        size: 16.0,
        line_height: 1.4,
    };
    /// A button's label: `--button-font`, 24 points at weight 700, line height 1.
    pub const BUTTON: TypeStyle = TypeStyle {
        font: Font::CrimsonPro,
        weight: 700,
        size: 24.0,
        line_height: 1.0,
    };
}

/// Fixed sizes, in logical points.
pub mod size {
    /// `--button-height`: every action button on the game screen.
    pub const BUTTON_HEIGHT: f32 = 48.0;
    /// A button's padding on either side of its label: the `.button` rule's 0.9em at
    /// `--button-font`.
    pub const BUTTON_PADDING: f32 = 21.6;
    /// The radius of a panel's corners.
    pub const PANEL_RADIUS: f32 = 12.0;
    /// The width of a panel's or a button's border: the `.button` rule's 1.5px.
    pub const BORDER: f32 = 1.5;
}

/// Colors as RGBA, not premultiplied.
pub mod color {
    /// `--surface`, slightly translucent for a panel over the table.
    pub const PANEL: [u8; 4] = [0x21, 0x1f, 0x1c, 0xe0];
    /// `--surface-raised`.
    pub const BUTTON: [u8; 4] = [0x2e, 0x2c, 0x29, 0xff];
    /// `--metal`: a thin pale gold border.
    pub const METAL: [u8; 4] = [214, 192, 150, 107];
    /// `--type`: text on the dark surfaces.
    pub const TYPE: [u8; 4] = [0xf5, 0xf3, 0xf0, 0xff];
    /// `--text-faint`: the label of a button that does nothing now.
    pub const TYPE_FAINT: [u8; 4] = [242, 236, 225, 97];
}
