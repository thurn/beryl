//! Writing the flat layer into a frame (docs/beryl-3d.md, "The frame"): a resolved element for
//! each kept element, a panel record for the top of the screen, the top of each float and each
//! button, a glyph record for each glyph a page holds (`text::write_block`), and the boxes of
//! the text's lines. It reads only what `Client::handle` laid out, and allocates nothing.

mod layer;

use std::borrow::Cow;

use glam::{Affine3A, Vec2};

use super::edit::Marks;
use super::panel::{Chain, look_of, mask_of, panel, write_box, write_mask};
use super::shadows::write_icon_shadows;
use super::{Layer, Node, text_top};
use crate::element::{Anchor, Display, Element, Overflow, Property, Shown, TextAlign};
use crate::input::{FaceId, FaceSize, Rect};
use crate::widgets::icons::{LayeredIcon, cell_width};
use kit_base::geometry::plane::{ScreenPlane, Zoom};
use kit_base::tokens::color;
use kit_frame::frame::text::{BlockLook, faded, write_block};
use kit_frame::frame::{
    Anchored, Band, Draw, ElementId, FrameBuilder, Missing, NameRef, Place, Resolved, Role,
    StateBits,
};
use kit_text::text;

/// Looks up texture layers for faces shown in the flat layer.
pub struct FaceLayers<'a> {
    pub back: FaceId,
    pub layer: &'a dyn Fn(FaceId, FaceSize) -> Option<u16>,
}

/// How a node and everything under it is drawn: where its tree's top left corner stands, how
/// opaque it is, the color of its text, the clip it is drawn under, whether the pointer goes
/// through it, the faces its cards are drawn from, and the filters it is drawn through.
#[derive(Clone, Copy)]
pub(super) struct Look<'a> {
    point: Vec2,
    pub(super) opacity: f32,
    tint: Option<[u8; 4]>,
    /// The clip's rectangle on the window, in logical points, and its entry in the frame's
    /// clips (`FrameBuilder::clip`), 0 when the frame's clips were all taken.
    pub(super) clip: Option<(Rect, u8)>,
    /// It or an element over it lets the pointer through (`Style::passes`).
    passes: bool,
    faces: Option<&'a FaceLayers<'a>>,
    /// The filter functions of it and the elements over it (`Style::filter`).
    pub(super) filters: Chain,
    /// Whether what waits for the table to settle is drawn (`Element::waits`).
    open: bool,
    /// The point the float is scaled about and the scale (`FloatLook::scale`), as the camera
    /// draws it: the boxes recorded for the frame are scaled the same way.
    pub(super) zoom: Zoom,
    departing: bool,
}

/// Paint outgoing children under their replacements; neither uses the other's layout slot.
fn children(node: &Node, departing: bool) -> impl Iterator<Item = (&Node, bool)> {
    node.departing
        .iter()
        .map(|child| (child, true))
        .chain(node.children.iter().map(move |child| (child, departing)))
}

#[derive(Clone, Copy)]
struct Drawing<'a, Preview> {
    camera: &'a ScreenPlane,
    preview: Preview,
}

/// Whether `element`, drawn as `look` in `rect`, takes a press. An element scrolled out of its
/// clip cannot be pressed, and one wholly transparent, as at the first frame it fades in, cannot
/// be seen to be. A text field takes a press, which gives it the focus, though it has no `Press`.
fn takes_press(element: &Element, look: &Look<'_>, rect: Rect) -> bool {
    !look.departing
        && (element.press.is_some() || element.focus)
        && look.opacity > 0.0
        && rect.width > 0.0
        && rect.height > 0.0
        && look.clip.is_none_or(|(clip, _)| overlaps(clip, rect))
}

impl Layer {
    /// How far a refused press shakes `node` across the screen (`Layer::shake`): every element
    /// that takes that press moves, with what is under it.
    fn shake_of(&self, node: &Node) -> Vec2 {
        match &self.shaken {
            Some((press, across)) if node.element.press.as_ref() == Some(press) => {
                Vec2::new(*across, 0.0)
            }
            _ => Vec2::ZERO,
        }
    }

    /// A held button sinks a point, and a refused press can shake its control.
    pub(super) fn node_sink(&self, node: &Node, state: StateBits) -> Vec2 {
        let sink = if state.has(StateBits::HELD) && sinks(node) {
            Vec2::Y
        } else {
            Vec2::ZERO
        };
        sink + self.shake_of(node)
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
        mut look: Look<'_>,
        builder: &mut FrameBuilder,
        drawing: Drawing<'_, impl Fn(ElementId) -> StateBits + Copy>,
    ) -> Option<u32> {
        if node.element.style.display == Display::None || (node.element.waits && !look.open) {
            return None;
        }
        builder.visit();
        look.opacity *= node.opacity();
        look.passes |= look.departing || node.element.style.passes;
        look.point += node.translation();
        look.filters = look.filters.inner(own_filter(&node.element));
        let element = &node.element;
        let mut state = StateBits(self.state(node).0 | (drawing.preview)(node.id).0);
        let camera = drawing.camera;
        if look.passes {
            state.0 |= StateBits::PASSES;
        }
        let sink = self.node_sink(node, state);
        let rect = moved(node.paint_rect(), look.point + node.offset() + sink);
        let scaled_camera;
        let camera = if node.scaled() {
            look.zoom = node.zoom(look.zoom, rect);
            scaled_camera = camera.with_zoom(look.zoom);
            &scaled_camera
        } else {
            camera
        };
        let painted = look.zoom.rect(rect);
        // A menu row is drawn highlighted while it has the focus or its submenu is open.
        let active = element.role == Role::MenuItem
            && (state.has(StateBits::FOCUSED) || self.submenu == Some(node.id));
        let panel = panel(node, top, state, active);
        if panel.is_some() || !element.style.backdrop_filter.is_empty() {
            state.0 |= StateBits::PAINTED;
        }
        let index = builder.resolved(
            &element.name,
            Resolved {
                id: node.id,
                parent,
                role: element.role,
                name: NameRef::default(),
                place: Place::Layer,
                pose: None,
                corners: corners(painted),
                depth: 0.0,
                opacity: look.opacity,
                clip: look.clip.map(|(rect, _)| rect),
                interactive: takes_press(element, &look, painted),
                state,
                face: None,
                copies: 1,
                really: kit_frame::frame::NameRef::default(),
                note: kit_frame::frame::NameRef::default(),
            },
        );
        if !look.departing {
            node.record_show(index, builder);
        }
        let mask = mask_of(node);
        if builder.looking() {
            let ink = ink(node, state, look.tint);
            look_of(
                node,
                (index, state),
                (panel.as_ref(), mask.as_ref()),
                ink,
                builder,
            );
        }
        // A mask puts the element and everything under it in a clip of its box.
        let masked = (mask.as_ref())
            .and_then(|mask| write_mask(mask, painted, &look, self.window.scale, builder));
        if masked.is_some() {
            look.clip = masked;
        }
        write_box(node, state, panel.as_ref(), rect, &look, builder, camera);
        let shown = Look {
            point: look.point + node.offset() + sink,
            ..look
        };
        self.write_content(node, (index, state), shown, builder, camera);
        let mut inside = look;
        if element.style.overflow != Overflow::Visible {
            // A mask's clip is already the box.
            if masked.is_none() {
                inside.clip = Some(clip_inside(painted, &look, self.window.scale, builder));
            }
            inside.point -= node.scroll;
        }
        self.write_children(
            node,
            (index, top),
            inside,
            builder,
            Drawing { camera, ..drawing },
        );
        Some(index)
    }

    /// Writes the children in paint order, with each anchor outside the parent's clip.
    fn write_children(
        &self,
        node: &Node,
        (index, top): (u32, bool),
        inside: Look<'_>,
        builder: &mut FrameBuilder,
        drawing: Drawing<'_, impl Fn(ElementId) -> StateBits + Copy>,
    ) {
        for (child, departing) in children(node, inside.departing) {
            // `write_with` writes these after the floats.
            if top && !inside.departing && above_floats(child) {
                continue;
            }
            let mut look = inside;
            look.departing = departing;
            if child.element.anchor == Some(Anchor::Tip) {
                if !departing {
                    self.write_tip(child, index, look, builder, drawing);
                }
                continue;
            }
            if child.element.anchor.is_some() {
                let Some(corner) = child.corner() else {
                    continue;
                };
                // An anchored child stands in a tree of its own, outside its parent's box, so
                // the clip of a list it is in does not cut it off, as `Layer::hit` lets it take
                // presses there.
                look.point = corner;
                look.clip = None;
            }
            self.write_node(child, index, false, look, builder, drawing);
        }
    }

    fn write_tip(
        &self,
        node: &Node,
        parent: u32,
        mut look: Look<'_>,
        builder: &mut FrameBuilder<'_>,
        drawing: Drawing<'_, impl Fn(ElementId) -> StateBits + Copy>,
    ) {
        let rect = node.paint_rect();
        let size = Vec2::new(rect.width, rect.height);
        let Some((anchor, target)) = self.tip_anchor(node.id, size, look.open) else {
            return;
        };
        look.point = anchor.corner(target, size);
        look.clip = None;
        if let Some(element) = self.write_node(node, parent, false, look, builder, drawing) {
            builder.anchored(Anchored {
                element,
                anchor,
                size,
                scale: 1.0,
            });
        }
    }

    /// Writes the content over the panel, with the field's selection behind its text and its
    /// caret above it.
    fn write_content(
        &self,
        node: &Node,
        (index, state): (u32, StateBits),
        shown: Look<'_>,
        builder: &mut FrameBuilder,
        camera: &ScreenPlane,
    ) {
        let edited = (self.editing.as_ref()).filter(|editing| editing.id == node.id);
        if let Some(editing) = edited {
            write_marks(node, &editing.marks, true, shown, builder, camera);
        }
        if node.block.is_some() {
            self.write_text(node, index, state, shown, builder, camera);
        }
        self.write_shown(node, index, state, shown, builder, camera);
        if let Some(editing) = edited {
            write_marks(node, &editing.marks, false, shown, builder, camera);
        }
    }

    /// Writes what `node` shows in its content box (`Shown`), moved, colored and clipped by
    /// `look`: an icon centred in it, or a face drawn flat to fill it. An icon the atlas holds no
    /// cell for at its size draws nothing, and is noted against the resolved element at `index`.
    fn write_shown(
        &self,
        node: &Node,
        index: u32,
        state: StateBits,
        look: Look<'_>,
        builder: &mut FrameBuilder,
        camera: &ScreenPlane,
    ) {
        let clip = look.clip.map_or(0, |(_, entry)| entry);
        let content = moved(node.content, look.point);
        let draw = match node.element.shows {
            Some(Shown::Icon(icon, side)) => {
                let Some(sprite) = self.icons.get(icon, side) else {
                    builder.undrawn(
                        index,
                        Missing::Icon {
                            icon: icon.0,
                            size: side,
                        },
                    );
                    return;
                };
                let points = f32::from(side);
                let at = Rect {
                    x: content.x + (content.width - points) / 2.0,
                    y: content.y + (content.height - points) / 2.0,
                    width: points,
                    height: points,
                };
                let shadowed = write_icon_shadows(
                    &node.element.style.drop_shadow,
                    &self.icons,
                    (icon, side),
                    at,
                    (self.window.scale, look.opacity),
                    clip,
                    builder,
                    camera,
                );
                if let Some(missing) = shadowed {
                    builder.undrawn(index, missing);
                }
                let tint = faded(
                    look.filters.color(ink(node, state, look.tint)),
                    look.opacity,
                );
                Draw::icon(camera.layer_transform(at), tint, sprite)
            }
            Some(Shown::Face(face, size) | Shown::TappedFace(face, size)) => {
                let Some(faces) = look.faces else {
                    return;
                };
                let mut transform = camera.layer_transform(content);
                if matches!(node.element.shows, Some(Shown::TappedFace(..))) {
                    transform *= Affine3A::from_rotation_z(-std::f32::consts::FRAC_PI_2);
                }
                let Some(mut draw) = face_draw(faces, (face, size), transform, builder.card())
                else {
                    return;
                };
                // A face's color, where it has one, darkens it, as a way that cannot be chosen
                // is dimmed (`prompts::choose`).
                let tint = node.element.style.color.unwrap_or([255; 4]);
                let (tint, desaturate) = look.filters.face(tint);
                draw.tint = faded(tint, look.opacity);
                if desaturate > 0.0 {
                    draw = draw.desaturated(desaturate);
                }
                draw
            }
            Some(Shown::Layers(icon)) => {
                let missing = self.write_icon_layers(icon, content, look, clip, builder, camera);
                if let Some(missing) = missing {
                    builder.undrawn(index, missing);
                }
                return;
            }
            None => return,
        };
        let draw = if clip > 0 { draw.with_clip(clip) } else { draw };
        builder.draw(Band::Layer, draw);
    }

    /// Writes the cells of a layered icon in art order. Each cell is centred in its artwork
    /// box, so rounding its width to whole points moves its edges by under half a point.
    /// Returns the first cell missing from the atlas.
    fn write_icon_layers(
        &self,
        icon: LayeredIcon,
        content: Rect,
        look: Look<'_>,
        clip: u8,
        builder: &mut FrameBuilder,
        camera: &ScreenPlane,
    ) -> Option<Missing> {
        let units = icon.scale;
        let [left, top] = icon.origin;
        let mut missing = None;
        for layer in icon.layers {
            if layer.highlight && !icon.highlighted {
                continue;
            }
            let [x, y, w, h] = layer.view;
            let width = cell_width(w, units);
            let Some(sprite) = self.icons.get(layer.icon, width) else {
                missing.get_or_insert(Missing::Icon {
                    icon: layer.icon.0,
                    size: width,
                });
                continue;
            };
            let width = f32::from(width);
            let height = width * h / w;
            let centre = Vec2::new(
                content.x + (x + w / 2.0 - left) * units,
                content.y + (y + h / 2.0 - top) * units,
            );
            let at = Rect {
                x: centre.x - width / 2.0,
                y: centre.y - height / 2.0,
                width,
                height,
            };
            let draw = Draw::icon(
                camera.layer_transform(at),
                faded(look.filters.color(layer.color), look.opacity),
                sprite,
            );
            builder.draw(
                Band::Layer,
                if clip > 0 { draw.with_clip(clip) } else { draw },
            );
        }
        missing
    }

    /// Writes the lines and glyphs of `node`'s text, which the resolved element at `index`
    /// shows, moved, colored and clipped by `look`.
    fn write_text(
        &self,
        node: &Node,
        index: u32,
        state: StateBits,
        look: Look<'_>,
        builder: &mut FrameBuilder,
        camera: &ScreenPlane,
    ) {
        let Some(block) = &node.block else {
            return;
        };
        let element = &node.element;
        let fade = |color: [u8; 4]| faded(look.filters.color(color), look.opacity);
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
                node.content.y + text_top(node, block.height),
            );
        // The words of each line are kept only while the frame is `looking`, so that no frame
        // of the app builds them.
        let source = match self
            .editing
            .as_ref()
            .filter(|editing| editing.id == node.id)
        {
            Some(editing) => editing.shown(node),
            None => element.text.as_deref().unwrap_or(""),
        };
        let words = if builder.looking() {
            text::shaped_words(source)
        } else {
            Cow::Borrowed("")
        };
        for line in &block.lines {
            builder.line(
                index,
                look.zoom.rect(Rect {
                    x: origin.x + line.rect.x,
                    y: origin.y + line.rect.y,
                    ..line.rect
                }),
                block.base.style.size,
                origin.y + line.baseline,
                line.words(&words),
            );
        }
        let tint = fade(ink(node, state, look.tint));
        let block_look = BlockLook {
            tint,
            opacity: look.opacity,
            scale: self.window.scale,
            glyphs: &self.glyphs,
            camera,
            clip: look.clip.map_or(0, |(_, entry)| entry),
            shadows: &element.style.drop_shadow,
        };
        let missing = write_block(builder, block, origin, &block_look);
        if missing > 0 {
            builder.undrawn(index, Missing::Glyphs(missing));
        }
    }
}

/// The clip the children of an element that clips or scrolls are drawn under: its box `rect`
/// cut to the clip `look` is under, and its entry in the frame's clips (0 when they were all
/// taken). A clip made under a masked one keeps its mask.
pub(super) fn clip_inside(
    rect: Rect,
    look: &Look,
    scale: f32,
    builder: &mut FrameBuilder,
) -> (Rect, u8) {
    let cut = look
        .clip
        .map_or(Some(rect), |(clip, _)| intersection(clip, rect));
    let cut = cut.unwrap_or(Rect {
        width: 0.0,
        height: 0.0,
        ..rect
    });
    let pixels = scaled(cut, scale);
    let mask = look.clip.and_then(|(_, outer)| builder.mask_of(outer));
    let entry = if mask.is_some() {
        builder.clip(pixels)
    } else {
        builder.scissor(pixels)
    }
    .unwrap_or(0);
    if let (Some((rect, angle, gradient)), true) = (mask, entry > 0) {
        builder.mask(entry, rect, angle, gradient);
    }
    (cut, entry)
}

/// Writes the marks of the field `node` while it is edited, moved and clipped by `look`:
/// the selection, drawn `behind` its text, or the caret and the composing text's underline,
/// drawn over it.
fn write_marks(
    node: &Node,
    marks: &Marks,
    behind: bool,
    look: Look,
    builder: &mut FrameBuilder,
    camera: &ScreenPlane,
) {
    // A field's text starts at the top of its content box, as `write_text` draws it.
    let origin = look.point + Vec2::new(node.content.x, node.content.y);
    let clip = look.clip.map_or(0, |(_, entry)| entry);
    let mut mark = |rect: Rect, tint: [u8; 4]| {
        let rect = moved(rect, origin);
        let tint = faded(look.filters.color(tint), look.opacity);
        let size = Vec2::new(rect.width, rect.height);
        let mut draw = Draw::panel(camera.layer_transform(rect), tint, tint, size, 0.0, 0.0);
        if clip > 0 {
            draw = draw.with_clip(clip);
        }
        builder.draw(Band::Layer, draw);
    };
    if behind {
        for &rect in &marks.selection {
            mark(rect, color::CYAN_SOFT);
        }
    } else {
        for rect in marks.caret.iter().chain(&marks.underline) {
            mark(*rect, color::TYPE);
        }
    }
}

/// The record that draws `face` flat in the box `transform` takes the unit square to: a card
/// with the `Card` material, from its full-size layer when `size` asks for one and it has
/// landed, and otherwise from its board-size layer, enlarged. `None` while it has neither.
fn face_draw(
    faces: &FaceLayers<'_>,
    (face, size): (FaceId, FaceSize),
    transform: Affine3A,
    card: Vec2,
) -> Option<Draw> {
    let full = match size {
        FaceSize::Full => (faces.layer)(face, FaceSize::Full),
        FaceSize::Board => None,
    };
    let (size, layer) = match full {
        Some(layer) => (FaceSize::Full, layer),
        None => (FaceSize::Board, (faces.layer)(face, FaceSize::Board)?),
    };
    let back = (faces.layer)(faces.back, size).unwrap_or(0);
    // The card mesh is `card` millimetres about its middle; the box is the unit square.
    let card = Affine3A::from_scale(card.recip().extend(1.0));
    Some(Draw::card(
        transform * card,
        [255; 4],
        size,
        layer,
        back,
        0.0,
        0.0,
    ))
}

/// Whether a press on `node` does something now.
pub(super) fn pressable(node: &Node) -> bool {
    node.element.press.is_some() && node.element.enabled
}

/// Whether `node` sinks while it is held down: a button with words, as `.button:active` moves
/// it a point down. An icon button darkens instead.
fn sinks(node: &Node) -> bool {
    node.element.role == Role::Button
        && pressable(node)
        && !matches!(node.element.shows, Some(Shown::Icon(..)))
}

/// The color of `node`'s text and icon: `tint`, a float's, if given; while it is disabled,
/// whatever its style says, a disabled button's dim label color (`.button:disabled`) on a
/// button with words and the faint type color on anything else; its style's color; or the type
/// color. An icon with no color of its own is drawn in the dim text
/// color, and in the text color under the pointer or with the focus (`.icon-button`).
fn ink(node: &Node, state: StateBits, tint: Option<[u8; 4]>) -> [u8; 4] {
    let element = &node.element;
    if let Some(tint) = tint {
        return tint;
    }
    if !element.enabled {
        let words = element.role == Role::Button && !matches!(element.shows, Some(Shown::Icon(..)));
        return if words {
            color::DISABLED_INK
        } else {
            color::TEXT_FAINT
        };
    }
    if let Some(color) = node.paint_color(Property::Color).or(element.style.color) {
        return color;
    }
    match element.shows {
        Some(Shown::Icon(..))
            if !(state.has(StateBits::HOVERED) || state.has(StateBits::FOCUSED)) =>
        {
            color::TEXT_DIM
        }
        Some(Shown::Icon(..)) => color::TEXT,
        _ => color::TYPE,
    }
}

impl Layer {
    /// The state `node` is drawn in and its resolved element has: disabled, focused, selected,
    /// primary, under the pointer and held down by a press.
    pub(super) fn state(&self, node: &Node) -> StateBits {
        let element = &node.element;
        let mut state = 0;
        if !element.enabled {
            state |= StateBits::DISABLED;
        }
        if self.focused == Some(node.id) {
            state |= StateBits::FOCUSED;
        }
        if element.selected {
            state |= StateBits::SELECTED;
        }
        if element.primary {
            state |= StateBits::PRIMARY;
        }
        if self.hovered == Some(node.id) {
            state |= StateBits::HOVERED;
        }
        if self.pressed == Some(node.id) {
            state |= StateBits::HELD;
        }
        StateBits(state)
    }
}

/// The part of the window that both `a` and `b` cover, if any.
pub(super) fn intersection(a: Rect, b: Rect) -> Option<Rect> {
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
pub(super) fn scaled(rect: Rect, scale: f32) -> Rect {
    Rect {
        x: rect.x * scale,
        y: rect.y * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}

/// `rect` moved by `offset`.
pub(super) fn moved(rect: Rect, offset: Vec2) -> Rect {
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

/// Visible modal overlays are painted in front of regular screen contents and floats.
pub(super) fn above_floats(node: &Node) -> bool {
    node.element.anchor.is_some() && (node.element.modal || over_floats(node))
}

/// Whether `node`, a child of a top element, is painted with a backdrop filter: an anchored element
/// with a backdrop filter (`Style::backdrop`), which filters the floats with the rest of what is
/// drawn under it. Only the screen's are written; a float's are not written at all.
pub(super) fn over_floats(node: &Node) -> bool {
    let style = &node.element.style;
    node.element.anchor.is_some() && style.backdrop && !style.filter.is_empty()
}

/// The filter functions `element` is drawn through itself: none when they filter what is drawn
/// under it instead (`Style::backdrop`).
pub(super) fn own_filter(element: &Element) -> &[crate::element::Filter] {
    if element.style.backdrop {
        &[]
    } else {
        &element.style.filter
    }
}
