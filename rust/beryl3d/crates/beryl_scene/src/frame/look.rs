//! What the agent tools that compare Beryl 3D with the web interface read of a frame
//! (`b3d tree --records`): how each flat-layer element was drawn and the words of each line of
//! text. A frame records these only while `Frame::looking` is set, so the app's frames do no
//! work for them.

use super::FrameBuilder;
use crate::element::{Filter, Gradient, Shadow};
use crate::text::TypeStyle;

/// How each flat-layer element of a frame was drawn, and its shadows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Looks {
    /// One for each flat-layer element, in the order they were added.
    pub elements: Vec<ElementLook>,
    /// The shadows of every element of `elements` that has any, in its style's order.
    pub shadows: Vec<LookShadow>,
    /// The gradients, masks and filters of every element of `elements` that has any, in its
    /// style's order.
    pub paints: Vec<LookPaint>,
}

/// One gradient, mask or filter function of a flat-layer element, as it was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LookPaint {
    /// One layer of its `background-image`, with the index in `Frame::resolved` of the element.
    Gradient(u32, Gradient),
    /// Its `mask-image`.
    Mask(u32, Gradient),
    /// One function of its `filter`.
    Filter(u32, Filter),
}

impl LookPaint {
    /// The index in `Frame::resolved` of the element it belongs to.
    #[must_use]
    pub fn element(&self) -> u32 {
        match *self {
            Self::Gradient(element, _) | Self::Mask(element, _) | Self::Filter(element, _) => {
                element
            }
        }
    }
}

/// One shadow of a flat-layer element's style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookShadow {
    /// The index in `Frame::resolved` of the element.
    pub element: u32,
    /// Whether it is one of the element's drop shadows (`Style::drop_shadow`) rather than its
    /// box shadows.
    pub drop: bool,
    pub shadow: Shadow,
}

impl Looks {
    pub fn clear(&mut self) {
        self.elements.clear();
        self.shadows.clear();
        self.paints.clear();
    }

    /// Records `look` and its element's shadows.
    pub fn push(&mut self, look: ElementLook, box_shadow: &[Shadow], drop_shadow: &[Shadow]) {
        let element = look.element;
        self.elements.push(look);
        let shadows = (box_shadow.iter().map(|shadow| (false, shadow)))
            .chain(drop_shadow.iter().map(|shadow| (true, shadow)));
        for (drop, &shadow) in shadows {
            self.shadows.push(LookShadow {
                element,
                drop,
                shadow,
            });
        }
    }
}

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

impl FrameBuilder<'_> {
    /// Records how the flat-layer element at `element` was painted, while the frame is
    /// `looking`: its gradients, its mask and its filter.
    pub fn look_paints(
        &mut self,
        element: u32,
        gradients: &[Gradient],
        mask: Option<&Gradient>,
        filter: &[Filter],
    ) {
        if !self.frame.looking {
            return;
        }
        let paints = &mut self.frame.looks.paints;
        paints.extend(gradients.iter().map(|g| LookPaint::Gradient(element, *g)));
        paints.extend(mask.map(|g| LookPaint::Mask(element, *g)));
        paints.extend(filter.iter().map(|f| LookPaint::Filter(element, *f)));
    }
}
