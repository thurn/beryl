//! What the agent tools that compare Beryl 3D with the web interface read of a frame
//! (`b3d tree --records`): how each flat-layer element was drawn and the words of each line of
//! text. A frame records these only while `Frame::looking` is set, so the app's frames do no
//! work for them.

use crate::text::TypeStyle;

/// How a flat-layer element was drawn: its panel, if it had one, the color of its text and
/// icon, and the style of its text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementLook {
    /// The index in `Frame::resolved` of the element.
    pub element: u32,
    /// The panel's color, before the element's opacity is applied, or `None` when it is drawn
    /// on no panel.
    pub background: Option<[u8; 4]>,
    /// The panel's border color and width, or `None` when it has no border.
    pub border: Option<([u8; 4], f32)>,
    /// The panel's corner radius in logical points, 0 with no panel.
    pub radius: f32,
    /// The color of its text and icon, before its opacity is applied.
    pub color: [u8; 4],
    /// How its text looks, with the size its laid-out text has: a text that shrinks to fit
    /// has shrunk.
    pub text: TypeStyle,
}
