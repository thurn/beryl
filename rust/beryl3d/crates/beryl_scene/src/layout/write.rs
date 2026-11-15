//! Writing the flat layer into a frame (docs/beryl-3d.md, "The frame"): a resolved element for
//! each kept element, a panel record for the screen and each button, a glyph record for each
//! glyph a page holds, and the boxes of the text's lines. It reads only what `Client::handle`
//! laid out, and allocates nothing.

use glam::Vec2;

use super::{Layer, Node, glyphs, has_panel};
use crate::element::TextAlign;
use crate::frame::{Band, Draw, FrameBuilder, NameRef, Place, Resolved, Sprite, StateBits};
use crate::input::Rect;
use crate::table::Camera;
use crate::tokens::{color, size};

impl Layer {
    /// Writes the flat layer into `builder`, on the plane `camera` puts in front of the table.
    pub fn write(&self, builder: &mut FrameBuilder, camera: &Camera) {
        if let Some(node) = &self.screen {
            self.write_node(node, u32::MAX, true, builder, camera);
        }
    }

    fn write_node(
        &self,
        node: &Node,
        parent: u32,
        top: bool,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        builder.visit();
        let element = &node.element;
        let index = builder.resolved(
            &element.name,
            Resolved {
                id: node.id,
                parent,
                role: element.role,
                name: NameRef::default(),
                place: Place::Layer,
                corners: corners(node.rect),
                depth: 0.0,
                opacity: 1.0,
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
                    camera.layer_transform(node.rect),
                    tint,
                    color::METAL,
                    Vec2::new(node.rect.width, node.rect.height),
                    if top {
                        size::PANEL_RADIUS
                    } else {
                        node.rect.height / 2.0
                    },
                    size::BORDER,
                ),
            );
        }
        if let Some(block) = &node.block {
            let style = element.style.text;
            let free = Vec2::new(
                node.content.width - block.width,
                node.content.height - block.height,
            );
            let origin = Vec2::new(
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
            let tint = if element.enabled {
                color::TYPE
            } else {
                color::TYPE_FAINT
            };
            let scale = self.window.scale;
            for glyph in &block.glyphs {
                let Some(placed) = self.glyphs.get(glyphs::key(style, scale, glyph.glyph)) else {
                    continue;
                };
                let [_, _, width, height] = placed.rect;
                let rect = Rect {
                    x: origin.x + glyph.x + f32::from(placed.bearing[0]) / scale,
                    y: origin.y + glyph.y + f32::from(placed.bearing[1]) / scale,
                    width: f32::from(width) / scale,
                    height: f32::from(height) / scale,
                };
                builder.draw(
                    Band::Layer,
                    Draw::glyph(
                        camera.layer_transform(rect),
                        tint,
                        Sprite {
                            page: placed.page,
                            rect: placed.rect,
                        },
                    ),
                );
            }
        }
        for child in &node.children {
            self.write_node(child, index, false, builder, camera);
        }
    }
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
