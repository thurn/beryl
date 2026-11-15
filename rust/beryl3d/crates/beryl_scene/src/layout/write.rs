//! Writing the flat layer into a frame (docs/beryl-3d.md, "The frame"): a resolved element for
//! each kept element, a panel record for the top of the screen, the top of each float and each
//! button, a glyph record for each glyph a page holds (`text::write_block`), and the boxes of
//! the text's lines. It reads only what `Client::handle` laid out, and allocates nothing.

use glam::Vec2;

use super::{FloatLook, Layer, Node, has_panel};
use crate::element::TextAlign;
use crate::frame::{Band, Draw, FrameBuilder, NameRef, Place, Resolved, StateBits};
use crate::input::Rect;
use crate::table::Camera;
use crate::text::{self, BlockLook};
use crate::tokens::{color, size};

impl Layer {
    /// Writes the flat layer into `builder`, on the plane `camera` puts in front of the table:
    /// the screen if it is `open`, then every float, whether the screen is open or not.
    pub fn write(&self, builder: &mut FrameBuilder, camera: &Camera, open: bool) {
        if open && let Some(node) = &self.screen {
            self.write_node(node, u32::MAX, true, FloatLook::default(), builder, camera);
        }
        for float in &self.floats {
            let look = FloatLook {
                point: float.corner(),
                anchor: Vec2::ZERO,
                ..float.look
            };
            self.write_node(&float.node, u32::MAX, true, look, builder, camera);
        }
    }

    /// Writes `node` and everything under it, moved so that its tree's top left corner stands
    /// at `look.point`, with `look`'s opacity and text color.
    fn write_node(
        &self,
        node: &Node,
        parent: u32,
        top: bool,
        look: FloatLook,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        builder.visit();
        let element = &node.element;
        let offset = look.point;
        let rect = moved(node.rect, offset);
        let fade = |color: [u8; 4]| faded(color, look.opacity);
        let index = builder.resolved(
            &element.name,
            Resolved {
                id: node.id,
                parent,
                role: element.role,
                name: NameRef::default(),
                place: Place::Layer,
                corners: corners(rect),
                depth: 0.0,
                opacity: look.opacity,
                clip: None,
                interactive: element.press.is_some(),
                state: StateBits(if element.enabled {
                    0
                } else {
                    StateBits::DISABLED
                }),
                face: None,
            },
        );
        if has_panel(element.role, top) {
            let tint = if top { color::PANEL } else { color::BUTTON };
            builder.draw(
                Band::Layer,
                Draw::panel(
                    camera.layer_transform(rect),
                    fade(tint),
                    fade(color::METAL),
                    Vec2::new(rect.width, rect.height),
                    if top {
                        size::PANEL_RADIUS
                    } else {
                        rect.height / 2.0
                    },
                    size::BORDER,
                ),
            );
        }
        if node.block.is_some() {
            self.write_text(node, index, look, builder, camera);
        }
        for child in &node.children {
            self.write_node(child, index, false, look, builder, camera);
        }
    }

    /// Writes the lines and glyphs of `node`'s text, which the resolved element at `index`
    /// shows, moved and colored by `look`.
    fn write_text(
        &self,
        node: &Node,
        index: u32,
        look: FloatLook,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        let Some(block) = &node.block else {
            return;
        };
        let element = &node.element;
        let offset = look.point;
        let fade = |color: [u8; 4]| faded(color, look.opacity);
        let style = element.style.text;
        let free = Vec2::new(
            node.content.width - block.width,
            node.content.height - block.height,
        );
        let origin = offset
            + Vec2::new(
                node.content.x
                    + match element.style.text_align {
                        TextAlign::Start => 0.0,
                        TextAlign::Center => free.x / 2.0,
                    },
                node.content.y + free.y / 2.0,
            );
        for line in &block.lines {
            builder.line(
                index,
                Rect {
                    x: origin.x + line.x,
                    y: origin.y + line.y,
                    ..*line
                },
            );
        }
        let tint = fade(match look.tint {
            Some(tint) => tint,
            None if element.enabled => color::TYPE,
            None => color::TEXT_FAINT,
        });
        let look = BlockLook {
            style,
            tint,
            scale: self.window.scale,
            glyphs: &self.glyphs,
            camera,
        };
        text::write_block(builder, block, origin, &look);
    }
}

/// `rect` moved by `offset`.
fn moved(rect: Rect, offset: Vec2) -> Rect {
    Rect {
        x: rect.x + offset.x,
        y: rect.y + offset.y,
        ..rect
    }
}

/// `color` with its alpha scaled by `opacity`.
fn faded(color: [u8; 4], opacity: f32) -> [u8; 4] {
    let [red, green, blue, alpha] = color;
    // An alpha times a value from 0 to 1 stays in 0 to 255, so the cast loses nothing but the
    // fraction.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the product is in 0 to 255"
    )]
    let alpha = (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}

/// A box's corners as a resolved element holds them: bottom left, bottom right, top right and
/// top left, on screen.
fn corners(rect: Rect) -> [Vec2; 4] {
    let (right, bottom) = (rect.x + rect.width, rect.y + rect.height);
    [
        Vec2::new(rect.x, bottom),
        Vec2::new(right, bottom),
        Vec2::new(right, rect.y),
        Vec2::new(rect.x, rect.y),
    ]
}
