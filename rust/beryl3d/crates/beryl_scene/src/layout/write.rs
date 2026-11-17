//! Writing the flat layer into a frame (docs/beryl-3d.md, "The frame"): a resolved element for
//! each kept element, a panel record for the top of the screen, the top of each float and each
//! button, a glyph record for each glyph a page holds (`text::write_block`), and the boxes of
//! the text's lines. It reads only what `Client::handle` laid out, and allocates nothing.

use std::borrow::Cow;

use beryl_protocol::protocol::Side;
use glam::{Affine3A, Vec2, Vec3};

use super::edit::Marks;
use super::{Layer, Node, has_panel, text_top};
use crate::art::orb;
use crate::element::{Display, Element, Overflow, Shown, TextAlign};
use crate::faces::{CARD_BACK, Faces};
use crate::frame::{
    Band, Draw, ElementLook, FrameBuilder, NameRef, Place, Resolved, Role, StateBits,
};
use crate::input::{FaceId, FaceSize, Rect};
use crate::table::{CARD_HEIGHT, CARD_WIDTH, Camera};
use crate::text::{self, BlockLook, faded};
use crate::tokens::{color, size};

/// How a node and everything under it is drawn: where its tree's top left corner stands, how
/// opaque it is, the color of its text, the clip it is drawn under, whether the pointer goes
/// through it, and the faces its cards are drawn from.
#[derive(Clone, Copy)]
struct Look<'a> {
    point: Vec2,
    opacity: f32,
    tint: Option<[u8; 4]>,
    /// The clip's rectangle on the window, in logical points, and its entry in the frame's
    /// clips (`FrameBuilder::clip`), 0 when the frame's clips were all taken.
    clip: Option<(Rect, u8)>,
    /// It or an element over it lets the pointer through (`Style::passes`).
    passes: bool,
    faces: Option<&'a Faces>,
}

/// Whether `element`, drawn as `look` in `rect`, takes a press. An element scrolled out of its
/// clip cannot be pressed, and one wholly transparent, as at the first frame it fades in, cannot
/// be seen to be. A text field takes a press, which gives it the focus, though it has no `Press`.
fn takes_press(element: &Element, look: &Look<'_>, rect: Rect) -> bool {
    (element.press.is_some() || element.focus)
        && look.opacity > 0.0
        && look.clip.is_none_or(|(clip, _)| overlaps(clip, rect))
}

impl Layer {
    /// Writes the flat layer into `builder`, on the plane `camera` puts in front of the table:
    /// the screen if it is `open`, then every float, whether the screen is open or not.
    pub fn write(&self, builder: &mut FrameBuilder, camera: &Camera, open: bool) {
        self.write_with(builder, camera, open, None);
    }

    /// `write`, drawing each face an element shows (`Shown::Face`) from the layers `faces`
    /// gives it. With no faces, such an element draws no face.
    pub fn write_with(
        &self,
        builder: &mut FrameBuilder,
        camera: &Camera,
        open: bool,
        faces: Option<&Faces>,
    ) {
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
                    passes: false,
                    faces,
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
                passes: false,
                faces,
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
        mut look: Look<'_>,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        if node.element.style.display == Display::None {
            return;
        }
        builder.visit();
        look.opacity *= node.opacity();
        look.passes |= node.element.style.passes;
        let element = &node.element;
        let mut state = self.state(node);
        if look.passes {
            state.0 |= StateBits::PASSES;
        }
        // A transition moves the node alone: each node under it has its own. A button held down
        // sinks a point, as the `.button:active` rule moves it.
        let sink = if state.has(StateBits::HELD) && sinks(node) {
            Vec2::Y
        } else {
            Vec2::ZERO
        };
        let rect = moved(node.rect, look.point + node.offset() + sink);
        // A menu row is drawn highlighted while it has the focus or its submenu is open.
        let active = element.role == Role::MenuItem
            && (state.has(StateBits::FOCUSED) || self.submenu == Some(node.id));
        let panel = panel(node, top, state, active);
        if panel.is_some() {
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
                corners: corners(rect),
                depth: 0.0,
                opacity: look.opacity,
                clip: look.clip.map(|(rect, _)| rect),
                interactive: takes_press(element, &look, rect),
                state,
                face: None,
                copies: 1,
                really: crate::frame::NameRef::default(),
                note: crate::frame::NameRef::default(),
            },
        );
        if builder.looking() {
            builder.look(look_of(
                node,
                index,
                panel.as_ref(),
                ink(node, state, look.tint),
            ));
        }
        if let Some(panel) = panel {
            write_panel(&panel, rect, &look, builder, camera);
        }
        let shown = Look {
            point: look.point + node.offset() + sink,
            ..look
        };
        let edited = (self.editing.as_ref()).filter(|editing| editing.id == node.id);
        if let Some(editing) = edited {
            write_marks(node, &editing.marks, true, shown, builder, camera);
        }
        if node.block.is_some() {
            self.write_text(node, index, state, shown, builder, camera);
        }
        self.write_shown(node, state, shown, builder, camera);
        if let Some(editing) = edited {
            write_marks(node, &editing.marks, false, shown, builder, camera);
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
                // An anchored child stands in a tree of its own, outside its parent's box, so
                // the clip of a list it is in does not cut it off, as `Layer::hit` lets it take
                // presses there.
                look.point = corner;
                look.clip = None;
            }
            self.write_node(child, index, false, look, builder, camera);
        }
    }

    /// Writes what `node` shows in its content box (`Shown`), moved, colored and clipped by
    /// `look`: an icon centred in it, or a face drawn flat to fill it.
    fn write_shown(
        &self,
        node: &Node,
        state: StateBits,
        look: Look<'_>,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        let clip = look.clip.map_or(0, |(_, entry)| entry);
        let content = moved(node.content, look.point);
        let draw = match node.element.shows {
            Some(Shown::Icon(icon, side)) => {
                let Some(sprite) = self.icons.get(icon, side) else {
                    return;
                };
                let side = f32::from(side);
                let at = Rect {
                    x: content.x + (content.width - side) / 2.0,
                    y: content.y + (content.height - side) / 2.0,
                    width: side,
                    height: side,
                };
                let tint = faded(ink(node, state, look.tint), look.opacity);
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
                let Some(mut draw) = face_draw(faces, face, size, transform) else {
                    return;
                };
                // A face's color, where it has one, darkens it, as a way that cannot be chosen
                // is dimmed (`prompts::choose`).
                let tint = node.element.style.color.unwrap_or([255; 4]);
                draw.tint = faded(tint, look.opacity);
                draw
            }
            Some(Shown::Orb {
                side,
                active,
                scale,
            }) => {
                self.write_orb((side, active, scale), content, look, clip, builder, camera);
                return;
            }
            Some(Shown::Object(_)) | None => return,
        };
        let draw = if clip > 0 { draw.with_clip(clip) } else { draw };
        builder.draw(Band::Layer, draw);
    }

    /// Writes the layers of `side`'s life orb (`art::orb::LAYERS`) in order, its art's box
    /// filling `content`. Each layer's cell is centred where its box lies in the art, so that a
    /// cell a whole number of points wide is off by under half a point at its edges.
    fn write_orb(
        &self,
        (side, active, scale): (Side, bool, u8),
        content: Rect,
        look: Look<'_>,
        clip: u8,
        builder: &mut FrameBuilder,
        camera: &Camera,
    ) {
        let units = orb::SCALES[usize::from(scale)];
        let [left, top, ..] = orb::ART;
        for (at, layer) in orb::LAYERS.iter().enumerate() {
            if layer.active && !active {
                continue;
            }
            let [x, y, w, h] = layer.views[orb::seat(side)];
            let width = orb::cell_width(w, units);
            let Some(sprite) = self.icons.get(orb::icon(side, at), width) else {
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
                faded(layer.color, look.opacity),
                sprite,
            );
            builder.draw(
                Band::Layer,
                if clip > 0 { draw.with_clip(clip) } else { draw },
            );
        }
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
                Rect {
                    x: origin.x + line.rect.x,
                    y: origin.y + line.rect.y,
                    ..line.rect
                },
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
        };
        text::write_block(builder, block, origin, &block_look);
    }
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
    camera: &Camera,
) {
    // A field's text starts at the top of its content box, as `write_text` draws it.
    let origin = look.point + Vec2::new(node.content.x, node.content.y);
    let clip = look.clip.map_or(0, |(_, entry)| entry);
    let mut mark = |rect: Rect, tint: [u8; 4]| {
        let rect = moved(rect, origin);
        let tint = faded(tint, look.opacity);
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
fn face_draw(faces: &Faces, face: FaceId, size: FaceSize, transform: Affine3A) -> Option<Draw> {
    let full = match size {
        FaceSize::Full => faces.full_layer_of(face),
        FaceSize::Board => None,
    };
    let (size, layer) = match full {
        Some(layer) => (FaceSize::Full, layer),
        None => (FaceSize::Board, faces.board_layer_of(face)?),
    };
    let back = match size {
        FaceSize::Full => faces.full_layer_of(CARD_BACK),
        FaceSize::Board => faces.board_layer_of(CARD_BACK),
    }
    .unwrap_or(0);
    // The card mesh is a card's size in millimetres about its middle; the box is the unit square.
    let card = Affine3A::from_scale(Vec3::new(1.0 / CARD_WIDTH, 1.0 / CARD_HEIGHT, 1.0));
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
///
/// The widgets' states change it as `buttons.css` and `icon-button.css` do: a disabled button
/// is flat stone under a dim rim whatever its style says; under the pointer a button is 10%
/// brighter (`filter: brightness(1.1)`), while an icon button's rim brightens instead, and
/// an icon button held down darkens. A highlighted menu row, `active`, is drawn on the gold
/// tint of `.table-menu-row[data-active]` under a soft gold rim.
fn panel(node: &Node, top: bool, state: StateBits, active: bool) -> Option<Panel> {
    if active {
        return Some(Panel {
            tint: color::MENU_ACTIVE,
            edge: color::GOLD_SOFT,
            radius: size::MENU_ROW_RADIUS,
            border: size::MENU_BORDER,
        });
    }
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
    let mut tint = style.background.unwrap_or(tint);
    let mut edge = style.border.map_or(color::METAL, |(edge, _)| edge);
    let icon = matches!(node.element.shows, Some(Shown::Icon(..)));
    if state.has(StateBits::DISABLED) && node.element.role == Role::Button && !icon {
        (tint, edge) = (color::DISABLED, color::DISABLED_RIM);
    } else if icon {
        if state.has(StateBits::HELD) {
            tint = color::ICON_BUTTON_PRESSED;
        }
        if state.has(StateBits::HOVERED) || state.has(StateBits::FOCUSED) {
            edge = color::ICON_BUTTON_RIM_HOVERED;
        }
    } else if state.has(StateBits::HOVERED) && pressable(node) {
        tint = brighter(tint);
        edge = brighter(edge);
    }
    let border = style.border.map_or(border, |(_, width)| width);
    // A panel with a clear face and no visible edge draws nothing (`element::holder`).
    if tint[3] == 0 && (edge[3] == 0 || border == 0.0) {
        return None;
    }
    Some(Panel {
        tint,
        edge,
        radius: if style.radius > 0.0 {
            style.radius
        } else {
            radius
        },
        border,
    })
}

/// Writes the record of `panel` filling `rect`, faded and clipped by `look`.
fn write_panel(
    panel: &Panel,
    rect: Rect,
    look: &Look,
    builder: &mut FrameBuilder,
    camera: &Camera,
) {
    let fade = |color: [u8; 4]| faded(color, look.opacity);
    let mut draw = Draw::panel(
        camera.layer_transform(rect),
        fade(panel.tint),
        fade(panel.edge),
        Vec2::new(rect.width, rect.height),
        panel.radius,
        panel.border,
    );
    let clip = look.clip.map_or(0, |(_, entry)| entry);
    if clip > 0 {
        draw = draw.with_clip(clip);
    }
    builder.draw(Band::Layer, draw);
}

/// How `node`, the resolved element at `index`, is drawn on `panel` in `ink`, for the tools that
/// compare Beryl 3D with the web interface (`Frame::looks`).
fn look_of(node: &Node, index: u32, panel: Option<&Panel>, ink: [u8; 4]) -> ElementLook {
    let mut text = node.element.style.text;
    if let Some(block) = &node.block {
        text.size = block.base.style.size;
    }
    ElementLook {
        element: index,
        background: panel.map(|panel| panel.tint),
        border: panel
            .filter(|panel| panel.border > 0.0)
            .map(|panel| (panel.edge, panel.border)),
        radius: panel.map_or(0.0, |panel| panel.radius),
        color: ink,
        text,
    }
}

/// Whether a press on `node` does something now.
fn pressable(node: &Node) -> bool {
    node.element.press.is_some() && node.element.enabled
}

/// Whether `node` sinks while it is held down: a button with words, as `.button:active` moves
/// it a point down. An icon button darkens instead.
fn sinks(node: &Node) -> bool {
    node.element.role == Role::Button
        && pressable(node)
        && !matches!(node.element.shows, Some(Shown::Icon(..)))
}

/// `color` 10% brighter, as CSS's `brightness(1.1)` makes it.
fn brighter(color: [u8; 4]) -> [u8; 4] {
    let lift = |channel: u8| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let lifted = (f32::from(channel) * 1.1).round().min(255.0) as u8;
        lifted
    };
    [lift(color[0]), lift(color[1]), lift(color[2]), color[3]]
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
    if let Some(color) = element.style.color {
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
    fn state(&self, node: &Node) -> StateBits {
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
