//! Writing an element's CSS box shadows into the flat layer (docs/beryl-3d.md, "Matching the web
//! interface's look"): one `BoxShadow` record per shadow, outer ones before the element's panel
//! and inset ones after it, as CSS paints a box's outer shadows, then its background, then its
//! inset shadows and its border. Shadows are written last first, since CSS draws the first one
//! on top. An icon's drop shadows are written under it, one `Icon` record for each shadow from
//! its cell (`widgets::icon_shadows`, `write_icon_shadows`). It allocates nothing.

use glam::Vec2;

use crate::element::Shadow;
use crate::frame::{Band, Draw, FrameBuilder, Missing, box_shadow_reach};
use crate::input::Rect;
use crate::table::Camera;
use crate::text::faded;
use crate::widgets::icons::Icons;
use crate::widgets::{IconId, icon_shadows};

/// The element box the shadows of one element are cast from.
#[derive(Clone, Copy, Debug)]
pub(super) struct ShadowBox {
    /// The border box, in logical points of the window.
    pub rect: Rect,
    /// The border box's corner radius.
    pub radius: f32,
    /// The border's widths, top, right, bottom and left, which an inset shadow stays inside.
    pub border: [f32; 4],
    /// How opaque the element is, which fades each shadow's color.
    pub opacity: f32,
    /// The clip's entry in the frame's clips, or 0 for none.
    pub clip: u8,
    /// The filters each shadow's color is drawn through.
    pub filters: super::panel::Chain,
}

/// Writes the outer box shadows of `shadows`, or the `inset` ones, cast from `cast`.
pub(super) fn write_box_shadows(
    shadows: &[Shadow],
    cast: &ShadowBox,
    inset: bool,
    builder: &mut FrameBuilder,
    camera: &Camera,
) {
    for shadow in shadows.iter().rev().filter(|s| s.inset == inset) {
        if shadow.color[3] == 0 {
            continue;
        }
        let (rect, radius) = if inset {
            padding_box(cast)
        } else {
            (cast.rect, cast.radius)
        };
        let size = Vec2::new(rect.width, rect.height);
        let spread = if inset { -shadow.spread } else { shadow.spread };
        let reach = box_shadow_reach(shadow);
        let quad = if inset {
            rect
        } else {
            Rect {
                x: rect.x + shadow.x - reach,
                y: rect.y + shadow.y - reach,
                width: rect.width + 2.0 * reach,
                height: rect.height + 2.0 * reach,
            }
        };
        let mut draw = Draw::box_shadow(
            camera.layer_transform(quad),
            faded(cast.filters.color(shadow.color), cast.opacity),
            size,
            radius,
            shadow,
            shape_radius(size, radius, spread),
        );
        if cast.clip > 0 {
            draw = draw.with_clip(cast.clip);
        }
        builder.draw(Band::Layer, draw);
    }
}

/// The padding box of `cast` and its corner radius: the border box less the border, with the
/// widest side's width taken off the radius, as CSS's inner border radius is where the sides
/// are the same width.
fn padding_box(cast: &ShadowBox) -> (Rect, f32) {
    let [top, right, bottom, left] = cast.border.map(|width| width.max(0.0));
    let rect = Rect {
        x: cast.rect.x + left,
        y: cast.rect.y + top,
        width: (cast.rect.width - left - right).max(0.0),
        height: (cast.rect.height - top - bottom).max(0.0),
    };
    let widest = top.max(right).max(bottom).max(left);
    (rect, (cast.radius - widest).max(0.0))
}

/// The corner radius of a shadow's shape: a box `size` large with corners of `radius`, grown
/// on every side by `spread` (shrunk for a negative one). `WebKit` scales the radius by how much
/// the box's shorter side grew (`FloatRoundedRect::inflateWithRadii`), not by adding the spread
/// as the CSS specification says, and `WebKit` is the reference.
#[must_use]
pub fn shape_radius(size: Vec2, radius: f32, spread: f32) -> f32 {
    let short = size.x.min(size.y);
    if short <= 0.0 || radius <= 0.0 {
        return 0.0;
    }
    radius * ((short + 2.0 * spread).max(0.0) / short)
}

/// Writes the drop shadows `shadows` cast by `icon`, held in the atlas at `side` points and
/// drawn over `at` in logical points of the window, bottom first: the cell of each shadow of
/// the chain of `icon_shadows::CHAINS` that equals `shadows` (`Icons::shadow`), in the shadow's
/// color faded by `opacity`. A cell's top left corner is `stage_pad` physical pixels at `scale`
/// above and left of the icon's, and each of its texels is `stage_step` pixels; its offset is
/// already in it. Returns a note of the first cell no page holds, which draws nothing, or of the
/// chain when none is listed.
#[expect(
    clippy::too_many_arguments,
    reason = "one icon's draw, as `write_shown` has it"
)]
pub(super) fn write_icon_shadows(
    shadows: &[Shadow],
    icons: &Icons,
    (icon, side): (IconId, u16),
    at: Rect,
    (scale, opacity): (f32, f32),
    clip: u8,
    builder: &mut FrameBuilder,
    camera: &Camera,
) -> Option<Missing> {
    if shadows.is_empty() {
        return None;
    }
    let missed = Missing::Icon {
        icon: icon.0,
        size: side,
    };
    let Some(chain) = icon_shadows::chain_of(icon, shadows) else {
        return Some(missed);
    };
    let shadows = icon_shadows::shadows(chain);
    let mut missing = None;
    for stage in (0..shadows.len()).rev() {
        let Some(sprite) = icons.shadow(icon, side, icon_shadows::code(chain, stage)) else {
            missing.get_or_insert(missed);
            continue;
        };
        let pad = f32::from(icon_shadows::stage_pad(shadows, stage, scale)) / scale;
        let texel = f32::from(icon_shadows::stage_step(shadows, stage, scale)) / scale;
        let rect = Rect {
            x: at.x - pad,
            y: at.y - pad,
            width: f32::from(sprite.rect[2]) * texel,
            height: f32::from(sprite.rect[3]) * texel,
        };
        let draw = Draw::icon(
            camera.layer_transform(rect),
            faded(shadows[stage].color, opacity),
            sprite,
        );
        builder.draw(
            Band::Layer,
            if clip > 0 { draw.with_clip(clip) } else { draw },
        );
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spread_scales_the_radius_by_the_shorter_side() {
        // An 8 point radius on an 80 point tall box grown by 3 points on every side: the
        // shorter side grows from 80 to 86.
        let found = shape_radius(Vec2::new(200.0, 80.0), 8.0, 3.0);
        assert!((found - 8.6).abs() < 1e-5, "{found}");
        assert!(shape_radius(Vec2::new(200.0, 80.0), 0.0, 3.0).abs() < f32::EPSILON);
    }
}
