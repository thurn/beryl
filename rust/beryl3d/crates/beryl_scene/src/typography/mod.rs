//! Bundled fonts and the type styles used by design tokens and text shaping.

pub mod fonts;

use fonts::Font;

/// How a run of text looks: its font, weight, whether it is italic, size in logical points and
/// line height as a multiple of its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    pub font: Font,
    /// The CSS weight, from 100 to 900. The bundled text fonts are variable, so any weight is
    /// drawn from the one file; the serif face picks its regular or bold file ([`Font::face`]).
    pub weight: u16,
    /// Whether it is italic (CSS `font-style: italic`). Only the serif face has italic files,
    /// and only where it is Times New Roman ([`Font::face`]); other fonts are drawn upright.
    pub italic: bool,
    pub size: f32,
    pub line_height: f32,
    /// Extra space after every letter, in logical points (CSS `letter-spacing`).
    pub letter_spacing: f32,
}
