//! Writing the flat layer into a frame (docs/beryl-3d.md, "The frame"): a resolved element for
//! each kept element, a panel record for the top of the screen, the top of each float and each
//! button, a glyph record for each glyph a page holds (`text::write_block`), and the boxes of
//! the text's lines. It reads only what `Client::handle` laid out, and allocates nothing.

use glam::Vec2;

use super::{Layer, Node, has_panel};
use crate::element::{Display, Overflow, TextAlign};
use crate::frame::{Band, Draw, FrameBuilder, NameRef, Place, Resolved, StateBits};
use crate::input::Rect;
use crate::table::Camera;
use crate::text::{self, BlockLook, faded};
use crate::tokens::{color, size};

/// How a node and everything under it is drawn: where its tree's top left corner stands, how
/// opaque it is, the color of its text, and the clip it is drawn under.
#[derive(Clone, Copy)]
struct Look {
    point: Vec2,
    opacity: f32,
    tint: Option<[u8; 4]>,
    /// The clip's rectangle on the window, in logical points, and its entry in the frame's
    /// clips (`FrameBuilder::clip`), 0 when the frame's clips were all taken.
    clip: Option<(Rect, u8)>,
}

impl Layer {
    /// Writes the flat layer into `builder`, on the plane `camera` puts in front of the table:
    /// the screen if it is `open`, then every float, whether the screen is open or not.
    pub fn write(&self, builder: &mut FrameBuilder, camera: &Camera, open: bool) {
        if open && let Some(node) = &self.screen {
            // An anchored screen stands at its corner, and is not drawn while that is unknown.
            let corner = match node.element.anchor {
                Some(_) => node.corner(),
                None => Some(Vec2::ZERO),
            };
            if let Some(point) = corner {
                let look = Look {
                    point,
                    opacity: 1.0,
                    tint: None,
                    clip: None,
                };
                self.write_node(node, u32::MAX, true, look, builder, camera);
            }
        }
        for float in &self.floats {
            let look = Look {
                point: float.corner(),
                opacity: float.look.opacity,
                tint: float.look.tint,
                clip: None,
            };
            self.write_node(&float.node, u32::MAX, true, look, builder, camera);
        }
    }

    /// Writes `node` and everything under it, moved so that its tree's top left corner stands
    /// at `look.point`, with `look`'s opacity, text color and clip. A node shown with
    /// `Display::None` is not written, and an anchored child is written at its own corner, or
    /// not at all while the object it is anchored to is not shown. A node that clips or
    /// scrolls writes its children under a clip of its box, the box of any clip it is under
    /// cut to it, moved by its scroll offset.
    fn write_node(
        &self,
        node: &Node,
        parent: u32,
        top: bool,
        mut look: Look,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        if node.element.style.display == Display::None {
            return;
        }
        builder.visit();
        look.opacity *= node.opacity();
        let element = &node.element;
        // A transition moves the node alone: each node under it has its own.
        let rect = moved(node.rect, look.point + node.offset());
        let fade = |color: [u8; 4]| faded(color, look.opacity);
        let clip = look.clip.map_or(0, |(_, entry)| entry);
        let mut state = 0;
        if !element.enabled {
            state |= StateBits::DISABLED;
        }
        if self.focused == Some(node.id) {
            state |= StateBits::FOCUSED;
        }
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
                clip: look.clip.map(|(rect, _)| rect),
                // An element scrolled out of its clip cannot be pressed, and one wholly
                // transparent, as at the first frame it fades in, cannot be seen to be.
                interactive: element.press.is_some()
                    && look.opacity > 0.0
                    && look.clip.is_none_or(|(clip, _)| overlaps(clip, rect)),
                state: StateBits(state),
                face: None,
                really: crate::frame::NameRef::default(),
            },
        );
        if let Some(panel) = panel(node, top) {
            let mut draw = Draw::panel(
                camera.layer_transform(rect),
                fade(panel.tint),
                fade(panel.edge),
                Vec2::new(rect.width, rect.height),
                panel.radius,
                panel.border,
            );
            if clip > 0 {
                draw = draw.with_clip(clip);
            }
            builder.draw(Band::Layer, draw);
        }
        if node.block.is_some() {
            let shown = Look {
                point: look.point + node.offset(),
                ..look
            };
            self.write_text(node, index, shown, builder, camera);
        }
        let mut inside = look;
        if element.style.overflow != Overflow::Visible {
            let cut = look
                .clip
                .map_or(Some(rect), |(clip, _)| intersection(clip, rect));
            let cut = cut.unwrap_or(Rect {
                width: 0.0,
                height: 0.0,
                ..rect
            });
            let entry = builder.clip(scaled(cut, self.window.scale)).unwrap_or(0);
            inside.clip = Some((cut, entry));
            inside.point -= node.scroll;
        }
        for child in &node.children {
            let mut look = inside;
            if child.element.anchor.is_some() {
                let Some(corner) = child.corner() else {
                    continue;
                };
                look.point = corner;
            }
            self.write_node(child, index, false, look, builder, camera);
        }
    }

    /// Writes the lines and glyphs of `node`'s text, which the resolved element at `index`
    /// shows, moved, colored and clipped by `look`.
    fn write_text(
        &self,
        node: &Node,
        index: u32,
        look: Look,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        let Some(block) = &node.block else {
            return;
        };
        let element = &node.element;
        let fade = |color: [u8; 4]| faded(color, look.opacity);
        let free = Vec2::new(
            node.content.width - block.width,
            node.content.height - block.height,
        );
        let origin = look.point
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
        let tint = fade(match look.tint.or(element.style.color) {
            Some(tint) => tint,
            None if element.enabled => color::TYPE,
            None => color::TEXT_FAINT,
        });
        let block_look = BlockLook {
            tint,
            opacity: look.opacity,
            scale: self.window.scale,
            glyphs: &self.glyphs,
            camera,
            clip: look.clip.map_or(0, |(_, entry)| entry),
        };
        text::write_block(builder, block, origin, &block_look);
    }
}

/// The panel an element is drawn on.
struct Panel {
    tint: [u8; 4],
    edge: [u8; 4],
    radius: f32,
    border: f32,
}

/// The panel `node` is drawn on, if any: the usual one under every button and the top element
/// of a screen or a float, and one under any element with a `background` or a `border`. Its
/// style's colors, border width and radius replace the usual ones where it gives them; a radius
/// of 0 keeps the usual one, a panel's or a button's, and is square on any other element.
fn panel(node: &Node, top: bool) -> Option<Panel> {
    let style = &node.element.style;
    let usual = has_panel(node.element.role, top);
    if !usual && style.background.is_none() && style.border.is_none() {
        return None;
    }
    let (tint, radius, border) = match (usual, top) {
        (true, true) => (color::PANEL, size::PANEL_RADIUS, size::BORDER),
        (true, false) => (color::BUTTON, node.rect.height / 2.0, size::BORDER),
        (false, _) => ([0; 4], 0.0, 0.0),
    };
    Some(Panel {
        tint: style.background.unwrap_or(tint),
        edge: style.border.map_or(color::METAL, |(edge, _)| edge),
        radius: if style.radius > 0.0 {
            style.radius
        } else {
            radius
        },
        border: style.border.map_or(border, |(_, width)| width),
    })
}

/// The part of the window that both `a` and `b` cover, if any.
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let (left, top) = (a.x.max(b.x), a.y.max(b.y));
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > left && bottom > top).then_some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// Whether `a` and `b` cover any of the window in common.
fn overlaps(a: Rect, b: Rect) -> bool {
    intersection(a, b).is_some()
}

/// A rectangle in logical points as physical pixels, at `scale` pixels to the point.
fn scaled(rect: Rect, scale: f32) -> Rect {
    Rect {
        x: rect.x * scale,
        y: rect.y * scale,
        width: rect.width * scale,
        height: rect.height * scale,
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
