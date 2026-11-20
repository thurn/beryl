//! Writing an element's box into the flat layer (docs/beryl-3d.md, "Matching the web interface's
//! look"): its outer box shadows, its panel (a flat color, its gradients and its border), its
//! inset box shadows, the clip and mask its `mask` makes, and the color filters and blur its
//! `filter` and its ancestors' apply. It allocates nothing.

use glam::Vec2;

use super::Node;
use super::shadows::{ShadowBox, write_box_shadows};
use super::write::{Look, intersection, scaled};
use crate::element::paint::{blur_of, filtered};
use crate::element::{Filter, Gradient, Shadow, Shape, Shown, Stop};
use crate::frame::{Band, Draw, ElementLook, FrameBuilder, Role, StateBits, gradient_reach};
use crate::input::Rect;
use crate::table::Camera;
use crate::text::faded;
use crate::tokens::{color, shadow, size};

/// The most filter functions an element and its ancestors apply together; any past these, the
/// outermost ancestors', are left out.
const MOST_FILTERS: usize = 6;

/// The filter functions an element is drawn through, its own first and then each ancestor's, as
/// CSS applies a parent's `filter` to what its children drew.
#[derive(Clone, Copy, Debug)]
pub(super) struct Chain {
    filters: [Filter; MOST_FILTERS],
    count: u8,
}

impl Default for Chain {
    fn default() -> Self {
        Self {
            filters: [Filter::Brightness(1.0); MOST_FILTERS],
            count: 0,
        }
    }
}

impl Chain {
    /// The chain of an element whose own filter functions are `own`, under this chain.
    pub(super) fn inner(self, own: &[Filter]) -> Self {
        if own.is_empty() {
            return self;
        }
        let mut out = Self::default();
        for filter in own.iter().chain(self.list()).take(MOST_FILTERS) {
            out.filters[usize::from(out.count)] = *filter;
            out.count += 1;
        }
        out
    }

    fn list(&self) -> &[Filter] {
        &self.filters[..usize::from(self.count)]
    }

    /// `color` as the chain's color filters change it.
    pub(super) fn color(&self, color: [u8; 4]) -> [u8; 4] {
        filtered(self.list(), color)
    }

    /// The standard deviation of the chain's blur in points as `WebKit` draws it, 0 with none.
    fn sigma(&self) -> f32 {
        blur_of(self.list()) * Shadow::WEBKIT_DEVIATION
    }

    /// The tint and desaturation (`Draw::desaturated`) that stand for the chain on a card's face,
    /// which no color of the record holds: its brightnesses multiply the tint `tint`, and its
    /// grayscales and saturations multiply into how far the face keeps its saturation.
    pub(super) fn face(&self, tint: [u8; 4]) -> ([u8; 4], f32) {
        let (mut bright, mut keep) = (1.0_f32, 1.0_f32);
        for filter in self.list() {
            match *filter {
                Filter::Brightness(amount) => bright *= amount,
                Filter::Grayscale(amount) => keep *= 1.0 - amount.clamp(0.0, 1.0),
                Filter::Saturate(amount) => keep *= amount,
                Filter::Blur(_) => {}
            }
        }
        let tint = Filter::Brightness(bright).color(tint);
        (tint, (1.0 - keep).clamp(0.0, 1.0))
    }
}

/// The panel an element is drawn on.
pub(super) struct Panel {
    pub(super) tint: [u8; 4],
    pub(super) edge: [u8; 4],
    pub(super) radius: f32,
    pub(super) border: f32,
    /// Whether its style's gradients are drawn: not on a disabled button, which is flat stone.
    gradients: bool,
    /// Whether it is 10% brighter, as a button under the pointer is.
    lifted: bool,
}

/// The panel `node` is drawn on, if any: the usual one under every button and the top element
/// of a screen or a float, and one under any element with a `background`, gradients or a
/// `border`. Its style's colors, border width and radius replace the usual ones where it gives
/// them; a radius of 0 keeps the usual one, a panel's or a button's, and is square on any other
/// element.
///
/// The widgets' states change it as `buttons.css` and `icon-button.css` do: a disabled button
/// is flat stone under a dim rim whatever its style says; under the pointer a button is 10%
/// brighter (`filter: brightness(1.1)`), while an icon button's rim brightens instead, and
/// an icon button held down darkens. A highlighted menu row, `active`, is drawn on the gold
/// tint of `.table-menu-row[data-active]` under a soft gold rim.
pub(super) fn panel(node: &Node, top: bool, state: StateBits, active: bool) -> Option<Panel> {
    if active {
        return Some(Panel {
            tint: color::MENU_ACTIVE,
            edge: color::GOLD_SOFT,
            radius: size::MENU_ROW_RADIUS,
            border: size::MENU_BORDER,
            gradients: false,
            lifted: false,
        });
    }
    let style = &node.element.style;
    let usual = super::has_panel(node.element.role, top);
    let painted = !style.background_image.is_empty();
    if !usual && style.background.is_none() && style.border.is_none() && !painted {
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
    let (mut gradients, mut lifted) = (painted, false);
    if state.has(StateBits::DISABLED) && node.element.role == Role::Button && !icon {
        (tint, edge, gradients) = (color::DISABLED, color::DISABLED_RIM, false);
    } else if icon {
        if state.has(StateBits::HELD) {
            tint = color::ICON_BUTTON_PRESSED;
        }
        if state.has(StateBits::HOVERED) || state.has(StateBits::FOCUSED) {
            edge = color::ICON_BUTTON_RIM_HOVERED;
        }
    } else if state.has(StateBits::HOVERED) && super::write::pressable(node) {
        tint = brighter(tint);
        edge = brighter(edge);
        lifted = true;
    }
    let border = style.border.map_or(border, |(_, width)| width);
    // A panel with a clear face, no gradient and no visible edge draws nothing
    // (`element::holder`).
    if tint[3] == 0 && !gradients && (edge[3] == 0 || border == 0.0) {
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
        gradients,
        lifted,
    })
}

/// The box shadows `node` is drawn with in `state`, as `buttons.css` and `icon-button.css`
/// change them: a disabled button with words has a faint dark ring whatever its style says, and
/// an icon button's rim brightens under the pointer or with the focus. Any other element has
/// its style's.
pub(super) fn box_shadows(node: &Node, state: StateBits) -> &[Shadow] {
    let element = &node.element;
    let icon = matches!(element.shows, Some(Shown::Icon(..)));
    if element.role != Role::Button || element.style.box_shadow.is_empty() {
        return &element.style.box_shadow;
    }
    if state.has(StateBits::DISABLED) && !icon {
        &shadow::DISABLED
    } else if icon && (state.has(StateBits::HOVERED) || state.has(StateBits::FOCUSED)) {
        &shadow::ICON_BUTTON_HOVERED
    } else {
        &element.style.box_shadow
    }
}

/// The gradients `panel` draws of `node`'s style.
fn gradients<'a>(node: &'a Node, panel: Option<&Panel>) -> &'a [Gradient] {
    match panel {
        Some(panel) if panel.gradients => &node.element.style.background_image,
        _ => &[],
    }
}

/// Writes `node`'s box, which fills `rect`: its outer box shadows (in `state`), its `panel` if
/// it has one, and its inset box shadows, faded, filtered and clipped by `look`.
///
/// A panel with gradients, or under a blur, is written as layers, as CSS paints a box: its
/// color, then its gradients last first, then its border over them. A blurred color is drawn as
/// a gradient of one stop, whose record can blur its edges.
pub(super) fn write_box(
    node: &Node,
    state: StateBits,
    panel: Option<&Panel>,
    rect: Rect,
    look: &Look,
    builder: &mut FrameBuilder,
    camera: &Camera,
) {
    let style = &node.element.style;
    let shadows = box_shadows(node, state);
    let cast = ShadowBox {
        rect,
        radius: panel.map_or(style.radius, |panel| panel.radius),
        border: panel.map_or_else(
            || style.border.map_or(0.0, |(_, width)| width),
            |panel| panel.border,
        ),
        opacity: look.opacity,
        clip: look.clip.map_or(0, |(_, entry)| entry),
        filters: look.filters,
    };
    write_box_shadows(shadows, &cast, false, builder, camera);
    if let Some(panel) = panel {
        let layers = gradients(node, Some(panel));
        let sigma = look.filters.sigma();
        if layers.is_empty() && sigma <= 0.0 {
            write_panel(panel, panel.tint, panel.border, rect, look, builder, camera);
        } else {
            if panel.tint[3] > 0 && sigma > 0.0 {
                let flat = Gradient::linear(180.0, &[Stop::new(panel.tint)]);
                write_gradient(&flat, rect, panel.radius, look, builder, camera);
            } else if panel.tint[3] > 0 {
                write_panel(panel, panel.tint, 0.0, rect, look, builder, camera);
            }
            for layer in layers.iter().rev() {
                let layer = if panel.lifted {
                    layer.map_colors(brighter)
                } else {
                    *layer
                };
                write_gradient(&layer, rect, panel.radius, look, builder, camera);
            }
            if panel.border > 0.0 && panel.edge[3] > 0 {
                write_panel(panel, [0; 4], panel.border, rect, look, builder, camera);
            }
        }
    }
    write_box_shadows(shadows, &cast, true, builder, camera);
}

/// Writes the record of `panel` filling `rect` in the color `tint`, with a border `border` wide,
/// faded, filtered and clipped by `look`.
fn write_panel(
    panel: &Panel,
    tint: [u8; 4],
    border: f32,
    rect: Rect,
    look: &Look,
    builder: &mut FrameBuilder,
    camera: &Camera,
) {
    let paint = |color: [u8; 4]| faded(look.filters.color(color), look.opacity);
    let mut draw = Draw::panel(
        camera.layer_transform(rect),
        paint(tint),
        paint(panel.edge),
        Vec2::new(rect.width, rect.height),
        panel.radius,
        border,
    );
    let clip = look.clip.map_or(0, |(_, entry)| entry);
    if clip > 0 {
        draw = draw.with_clip(clip);
    }
    builder.draw(Band::Layer, draw);
}

/// Writes the record of `gradient` filling `rect`, with corners of `radius`, faded, filtered,
/// blurred and clipped by `look`. A gradient past the frame's table is not drawn
/// (`Paints::full`).
fn write_gradient(
    gradient: &Gradient,
    rect: Rect,
    radius: f32,
    look: &Look,
    builder: &mut FrameBuilder,
    camera: &Camera,
) {
    let size = Vec2::new(rect.width, rect.height);
    let resolved = gradient
        .map_colors(|color| look.filters.color(color))
        .resolve(size);
    let Some(entry) = builder.gradient(&resolved) else {
        return;
    };
    let sigma = look.filters.sigma();
    let reach = gradient_reach(sigma);
    let quad = Rect {
        x: rect.x - reach,
        y: rect.y - reach,
        width: rect.width + 2.0 * reach,
        height: rect.height + 2.0 * reach,
    };
    let mut draw = Draw::gradient(
        camera.layer_transform(quad),
        faded([255; 4], look.opacity),
        size,
        radius,
        sigma,
        &resolved.geometry,
        entry,
    );
    let clip = look.clip.map_or(0, |(_, entry)| entry);
    if clip > 0 {
        draw = draw.with_clip(clip);
    }
    builder.draw(Band::Layer, draw);
}

/// The clip that `mask`, an element's `mask-image` over its box `rect`, puts the element and
/// everything under it in: `rect` cut to the clip `look` is already under, with the mask on it.
/// `None` for a mask that is not a linear gradient, which is not drawn, or when the frame's
/// clips or gradients are all taken (a fault the checks report).
pub(super) fn write_mask(
    mask: &Gradient,
    rect: Rect,
    look: &Look,
    scale: f32,
    builder: &mut FrameBuilder,
) -> Option<(Rect, u8)> {
    let Shape::Linear { angle } = mask.shape else {
        return None;
    };
    let cut = look
        .clip
        .map_or(Some(rect), |(clip, _)| intersection(clip, rect))
        .unwrap_or(Rect {
            width: 0.0,
            height: 0.0,
            ..rect
        });
    let resolved = mask.resolve(Vec2::new(rect.width, rect.height));
    let gradient = builder.gradient(&resolved)?;
    let entry = builder.clip(scaled(cut, scale))?;
    builder.mask(entry, scaled(rect, scale), angle.to_radians(), gradient);
    Some((cut, entry))
}

/// The mask `node` is drawn under: the one its `scroll_fade` makes where it is scrolled, fading
/// out each end with more content beyond it, or its style's `mask`.
pub(super) fn mask_of(node: &Node) -> Option<Gradient> {
    let style = &node.element.style;
    let Some(fade) = style.scroll_fade else {
        return style.mask;
    };
    let (clear, solid) = ([0; 4], [0, 0, 0, 255]);
    let more_above = node.scroll.y > 0.5;
    let more_below = node.scroll.y < node.most_scroll().y - 0.5;
    match (more_above, more_below) {
        (false, false) => None,
        // `linear-gradient(to top, transparent, #000 <fade>)`.
        (false, true) => Some(Gradient::linear(
            0.0,
            &[Stop::new(clear), Stop::points(solid, fade)],
        )),
        (true, false) => Some(Gradient::linear(
            180.0,
            &[Stop::new(clear), Stop::points(solid, fade)],
        )),
        (true, true) => Some(Gradient::linear(
            180.0,
            &[
                Stop::new(clear),
                Stop::points(solid, fade),
                Stop::points(solid, node.rect.height - fade),
                Stop::new(clear),
            ],
        )),
    }
}

/// How `node`, the resolved element at `index`, is drawn on `panel` in `ink`, for the tools that
/// compare Beryl 3D with the web interface (`Frame::looks`), with its gradients, mask and filter.
pub(super) fn look_of(
    node: &Node,
    (index, state): (u32, StateBits),
    (panel, mask): (Option<&Panel>, Option<&Gradient>),
    ink: [u8; 4],
    builder: &mut FrameBuilder,
) {
    let style = &node.element.style;
    let mut text = style.text;
    if let Some(block) = &node.block {
        text.size = block.base.style.size;
    }
    let look = ElementLook {
        element: index,
        background: panel.map(|panel| panel.tint),
        border: panel
            .filter(|panel| panel.border > 0.0)
            .map(|panel| (panel.edge, panel.border)),
        radius: panel.map_or(0.0, |panel| panel.radius),
        color: ink,
        text,
    };
    builder.look(look, box_shadows(node, state), &style.drop_shadow);
    let lifted = [Filter::Brightness(1.1)];
    let filter: &[Filter] = if panel.is_some_and(|panel| panel.lifted) {
        &lifted
    } else {
        super::write::own_filter(&node.element)
    };
    builder.look_paints(index, gradients(node, panel), mask, filter);
}

/// `color` 10% brighter, as CSS's `brightness(1.1)` makes it.
pub(super) fn brighter(color: [u8; 4]) -> [u8; 4] {
    Filter::Brightness(1.1).color(color)
}
