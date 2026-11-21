//! Writing an element's box into the flat layer (docs/beryl-3d.md, "Matching the web interface's
//! look"): its outer box shadows, its panel (a flat color, its gradients and its border), its
//! inset box shadows, the clip and mask its `mask` makes, and the color filters and blur its
//! `filter` and its ancestors' apply. It allocates nothing.

use glam::Vec2;

use super::Node;
use super::shadows::{ShadowBox, write_box_shadows};
use super::write::{Look, clip_inside, intersection, scaled};
use crate::element::paint::{blur_of, filtered};
use crate::element::{BackgroundBox, Border, Filter, Gradient, Shadow, Shape, Shown, Stop};
use crate::input::Rect;
use kit_base::geometry::plane::ScreenPlane;
use kit_base::tokens::{color, shadow, size};
use kit_frame::frame::text::faded;
use kit_frame::frame::{
    BackdropPanel, Band, Draw, ElementLook, FrameBuilder, Role, SIDE_BOTTOM, SIDE_LEFT, SIDE_RIGHT,
    SIDE_TOP, StateBits, gradient_reach,
};

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
    /// Its border's colors and widths; a side of no width draws nothing.
    pub(super) border: Border,
    pub(super) radius: f32,
    /// Whether its style's gradients are drawn: not on a disabled button, which is flat stone.
    gradients: bool,
    /// Whether it is 10% brighter, as a button under the pointer is.
    lifted: bool,
    /// For an element whose first child is its legend (`Style::legend`), where that legend
    /// stands in its box.
    legend: Option<Legend>,
}

/// Where a fieldset's legend stands, in points from its box's top left corner: how far down
/// its panel starts and the left and right ends of the opening in its border's top line.
#[derive(Clone, Copy, Debug)]
struct Legend {
    drop: f32,
    left: f32,
    right: f32,
}

impl Legend {
    /// The legend of `node`, which has `border` on its panel, if its style says its first child
    /// is one. Browsers put the middle of the border's top line at the legend's vertical middle
    /// (`RenderFieldset::paintBoxDecorations` in `WebKit`), and leave the line open across the
    /// legend's border box, which takes its `padding-inline` of 2 points on either side.
    fn of(node: &Node, border: &Border) -> Option<Self> {
        if !node.element.style.legend {
            return None;
        }
        let legend = node.children.first()?.rect;
        let middle = legend.y - node.rect.y + legend.height / 2.0;
        Some(Self {
            drop: (middle - border.width[0] / 2.0).max(0.0),
            left: legend.x - node.rect.x,
            right: legend.x - node.rect.x + legend.width,
        })
    }
}

impl Panel {
    /// The box the panel fills when the element's border box is `rect`: the same box, or for a
    /// fieldset the part of it from the legend's middle down.
    fn drawn(&self, rect: Rect) -> Rect {
        let drop = self.legend.map_or(0.0, |legend| legend.drop);
        Rect {
            y: rect.y + drop,
            height: (rect.height - drop).max(0.0),
            ..rect
        }
    }
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
            border: Border::uniform(color::GOLD_SOFT, size::MENU_BORDER),
            radius: size::MENU_ROW_RADIUS,
            gradients: false,
            lifted: false,
            legend: None,
        });
    }
    let style = &node.element.style;
    let usual = super::has_panel(node.element.role, top);
    let painted = !style.background_image.is_empty();
    let background = node
        .paint_color(crate::element::Property::Background)
        .or(style.background);
    if !usual && background.is_none() && style.border.is_none() && !painted {
        return None;
    }
    let (tint, radius, border) = match (usual, top) {
        (true, true) => (color::PANEL, size::PANEL_RADIUS, size::BORDER),
        (true, false) => (color::BUTTON, node.rect.height / 2.0, size::BORDER),
        (false, _) => ([0; 4], 0.0, 0.0),
    };
    let mut tint = background.unwrap_or(tint);
    let mut border = style
        .border
        .unwrap_or(Border::uniform(color::METAL, border));
    let recolor = |border: Border, edge: [u8; 4]| Border {
        color: [edge; 4],
        ..border
    };
    let icon = matches!(node.element.shows, Some(Shown::Icon(..)));
    let (mut gradients, mut lifted) = (painted, false);
    if state.has(StateBits::DISABLED) && node.element.role == Role::Button && !icon {
        (tint, gradients) = (color::DISABLED, false);
        border = recolor(border, color::DISABLED_RIM);
    } else if icon {
        if state.has(StateBits::HELD) {
            tint = color::ICON_BUTTON_PRESSED;
        }
        if state.has(StateBits::HOVERED) || state.has(StateBits::FOCUSED) {
            border = recolor(border, color::ICON_BUTTON_RIM_HOVERED);
        }
    } else if state.has(StateBits::HOVERED) && super::write::pressable(node) {
        tint = brighter(tint);
        border = border.map_colors(brighter);
        lifted = true;
    }
    // A panel with a clear face, no gradient and no visible edge draws nothing
    // (`element::holder`).
    if tint[3] == 0 && !gradients && !visible(&border) {
        return None;
    }
    Some(Panel {
        tint,
        border,
        radius: if style.radius > 0.0 {
            style.radius
        } else {
            radius
        },
        gradients,
        lifted,
        legend: Legend::of(node, &border),
    })
}

/// Whether any side of `border` draws: it has a width and a color that is not clear.
fn visible(border: &Border) -> bool {
    (border.color.iter().zip(border.width)).any(|(edge, width)| edge[3] > 0 && width > 0.0)
}

/// The box inside the border and its corner radius. The painter uses one radius, reduced by
/// the largest border width so it fits inside each side.
pub(super) fn padding_box(rect: Rect, radius: f32, border: [f32; 4]) -> (Rect, f32) {
    let [top, right, bottom, left] = border.map(|width| width.max(0.0));
    let inside = Rect {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    };
    let widest = top.max(right).max(bottom).max(left);
    (inside, (radius - widest).max(0.0))
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

/// Writes the filter behind the panel, or a translucent fill on a tier without a blur pass.
fn write_backdrop(
    filters: &[Filter],
    (rect, radius): (Rect, f32),
    look: &Look<'_>,
    scale: f32,
    builder: &mut FrameBuilder,
    camera: &ScreenPlane,
) {
    if look.opacity <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let bounds = scaled(look.zoom.rect(rect), scale);
    // The parent's clip is already expressed in window points after its own transform.
    let clip = look.clip.map_or(bounds, |(clip, _)| scaled(clip, scale));
    let Some(clip) = intersection(bounds, clip) else {
        return;
    };
    let pixels_per_point = bounds.width / rect.width;
    let panel = BackdropPanel {
        rect: bounds,
        clip,
        radius: radius * pixels_per_point,
        opacity: look.opacity,
    };
    builder.backdrop_panel(filters, pixels_per_point, panel);
    let draw = Draw::panel(
        camera.layer_transform(rect),
        faded(color::BACKDROP_FILTER_FALLBACK, look.opacity),
        [0; 4],
        Vec2::new(rect.width, rect.height),
        radius,
        0.0,
    );
    builder.draw(
        Band::Layer,
        draw.with_clip(look.clip.map_or(0, |(_, entry)| entry)),
    );
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
    camera: &ScreenPlane,
) {
    let style = &node.element.style;
    let shadows = box_shadows(node, state);
    let rect = panel.map_or(rect, |panel| panel.drawn(rect));
    let cast = ShadowBox {
        rect,
        radius: panel.map_or(style.radius, |panel| panel.radius),
        border: panel.map_or_else(
            || style.border.map_or([0.0; 4], |border| border.width),
            |panel| panel.border.width,
        ),
        opacity: look.opacity,
        clip: look.clip.map_or(0, |(_, entry)| entry),
        filters: look.filters,
    };
    write_box_shadows(shadows, &cast, false, builder, camera);
    if !style.backdrop_filter.is_empty() {
        write_backdrop(
            &style.backdrop_filter,
            (rect, cast.radius),
            look,
            camera.scale(),
            builder,
            camera,
        );
    }
    if let Some(panel) = panel {
        let layers = gradients(node, Some(panel));
        let sigma = look.filters.sigma();
        if layers.is_empty() && sigma <= 0.0 {
            write_panel(panel, panel.tint, true, rect, look, builder, camera);
        } else {
            if panel.tint[3] > 0 && sigma > 0.0 {
                let flat = Gradient::linear(180.0, &[Stop::new(panel.tint)]);
                write_gradient(&flat, rect, panel.radius, look, builder, camera);
            } else if panel.tint[3] > 0 {
                write_panel(panel, panel.tint, false, rect, look, builder, camera);
            }
            let boxes = &style.background_boxes;
            for (index, layer) in layers.iter().enumerate().rev() {
                let (area, radius) =
                    if !boxes.is_empty() && boxes[index % boxes.len()] == BackgroundBox::Padding {
                        padding_box(rect, panel.radius, panel.border.width)
                    } else {
                        (rect, panel.radius)
                    };
                if area.width <= 0.0 || area.height <= 0.0 {
                    continue;
                }
                let layer = if panel.lifted {
                    layer.map_colors(brighter)
                } else {
                    *layer
                };
                write_gradient(&layer, area, radius, look, builder, camera);
            }
            if visible(&panel.border) {
                write_panel(panel, [0; 4], true, rect, look, builder, camera);
            }
        }
    }
    write_box_shadows(shadows, &cast, true, builder, camera);
}

/// The bit of each side in a `Draw::panel_sides` record, top, right, bottom and left.
const SIDE_BITS: [u8; 4] = [SIDE_TOP, SIDE_RIGHT, SIDE_BOTTOM, SIDE_LEFT];

/// Writes the records of `panel` filling `rect` in the color `tint`, with its border if
/// `bordered`, faded, filtered and clipped by `look`.
///
/// A border whose sides are all alike is one record. One whose sides differ is one
/// `Draw::panel_sides` record for each color, in the order the sides first use it: the first
/// also fills the panel, and each draws only its sides, every record knowing every side's width
/// so that the corners split where CSS splits them. A fieldset's border (`Panel::legend`) is
/// drawn as one whose sides differ, with its top side in two more records, each clipped to the
/// part of the box on one side of the legend.
fn write_panel(
    panel: &Panel,
    tint: [u8; 4],
    bordered: bool,
    rect: Rect,
    look: &Look,
    builder: &mut FrameBuilder,
    camera: &ScreenPlane,
) {
    let paint = |color: [u8; 4]| faded(look.filters.color(color), look.opacity);
    let clip = look.clip.map_or(0, |(_, entry)| entry);
    let transform = camera.layer_transform(rect);
    let size = Vec2::new(rect.width, rect.height);
    let border = &panel.border;
    let legend = panel.legend.filter(|_| bordered);
    // The top side's two clips, left of the legend and right of it, each cut to the clip the
    // element is under.
    let opening = legend.map(|legend| {
        let scale = camera.scale();
        let left = Rect {
            width: legend.left.max(0.0),
            ..rect
        };
        let right = Rect {
            x: rect.x + legend.right,
            width: (rect.width - legend.right).max(0.0),
            ..rect
        };
        [left, right].map(|part| clip_inside(part, look, scale, builder).1)
    });
    let mut write = |draw: Draw, clip: u8| {
        builder.draw(
            Band::Layer,
            if clip > 0 { draw.with_clip(clip) } else { draw },
        );
    };
    if !bordered || (border.is_uniform() && legend.is_none()) {
        let width = if bordered { border.width[0] } else { 0.0 };
        let edge = paint(border.color[0]);
        write(
            Draw::panel(transform, paint(tint), edge, size, panel.radius, width),
            clip,
        );
        return;
    }
    let mut fill = paint(tint);
    let mut done = 0_u8;
    for (side, edge) in border.color.iter().enumerate() {
        if done & SIDE_BITS[side] != 0 {
            continue;
        }
        let sides = (0..4)
            .filter(|&other| border.color[other] == *edge)
            .fold(0, |bits, other| bits | SIDE_BITS[other]);
        done |= sides;
        let whole = if opening.is_some() {
            sides & !SIDE_TOP
        } else {
            sides
        };
        if whole != 0 || fill[3] > 0 {
            write(
                Draw::panel_sides(
                    transform,
                    fill,
                    paint(*edge),
                    size,
                    panel.radius,
                    (border.width, whole),
                ),
                clip,
            );
            fill = [0; 4];
        }
        if let (Some(parts), true) = (opening, sides & SIDE_TOP != 0) {
            for part in parts {
                let top = (border.width, SIDE_TOP);
                let draw =
                    Draw::panel_sides(transform, [0; 4], paint(*edge), size, panel.radius, top);
                write(draw, part);
            }
        }
    }
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
    camera: &ScreenPlane,
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
            .filter(|panel| panel.border.width.iter().any(|width| *width > 0.0))
            .map(|panel| panel.border),
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
