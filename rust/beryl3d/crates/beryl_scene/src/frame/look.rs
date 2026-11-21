//! What the agent tools that compare Beryl 3D with the web interface read of a frame
//! (`b3d tree --records`): how each flat-layer element was drawn and the words of each line of
//! text. A frame records these only while `Frame::looking` is set, so the app's frames do no
//! work for them.

use beryl_protocol::script::Subject;

use super::FrameBuilder;
use crate::paint::{Border, Filter, Gradient, Shadow};
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
    /// Each effect the frame draws on a card, an ability or a player, in the order drawn.
    pub effects: Vec<EffectLook>,
}

/// One effect as a frame draws it, which `tree --records` reports as an element of role
/// `effect`, so that `just b3d match` can measure it against the web interface's `.stage-effect`
/// node of the same name on the same thing (docs/beryl-3d.md, "Effects"). `model` places a quad
/// from -0.5 to 0.5 in the table's space, as `Draw::model` does. For an effect that is light,
/// whose web node is only a record the size of what it is on (the light is drawn on a canvas or
/// past the node's edge), both are that node's box and opacity as its CSS rule scales and fades
/// it (`node` in `effects::light`). For one that changes how the card is drawn, `model` is the
/// card's face and `opacity` how far the card is washed white or darkened; it is 1 while a life
/// total is tinted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectLook {
    pub name: &'static str,
    pub on: Option<Subject>,
    pub model: [f32; 12],
    pub opacity: f32,
}

impl FrameBuilder<'_> {
    /// Records `look` while the frame is `looking`; the app's frames record nothing.
    pub fn effect(&mut self, look: EffectLook) {
        if self.frame.looking {
            self.frame.looks.effects.push(look);
        }
    }
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
        self.effects.clear();
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
    /// The panel's border, or `None` when it has no border.
    pub border: Option<Border>,
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
