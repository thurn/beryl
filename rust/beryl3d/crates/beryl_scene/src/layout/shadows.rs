//! Writing an element's CSS box shadows into the flat layer (docs/beryl-3d.md, "Matching the web
//! interface's look"): one `BoxShadow` record per shadow, outer ones before the element's panel
//! and inset ones after it, as CSS paints a box's outer shadows, then its background, then its
//! inset shadows and its border. Shadows are written last first, since CSS draws the first one
//! on top. It allocates nothing.

use glam::Vec2;

use crate::element::Shadow;
use crate::frame::{Band, Draw, FrameBuilder, box_shadow_reach};
use crate::input::Rect;
use crate::table::Camera;
use crate::text::faded;

/// The element box the shadows of one element are cast from.
#[derive(Clone, Copy, Debug)]
pub(super) struct ShadowBox {
    /// The border box, in logical points of the window.
    pub rect: Rect,
    /// The border box's corner radius.
    pub radius: f32,
    /// The border's width, which an inset shadow stays inside.
    pub border: f32,
    /// How opaque the element is, which fades each shadow's color.
    pub opacity: f32,
    /// The clip's entry in the frame's clips, or 0 for none.
    pub clip: u8,
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
            faded(shadow.color, cast.opacity),
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
/// border's width taken off the radius, as CSS's inner border radius is.
fn padding_box(cast: &ShadowBox) -> (Rect, f32) {
    let border = cast.border.max(0.0);
    let rect = Rect {
        x: cast.rect.x + border,
        y: cast.rect.y + border,
        width: (cast.rect.width - 2.0 * border).max(0.0),
        height: (cast.rect.height - 2.0 * border).max(0.0),
    };
    (rect, (cast.radius - border).max(0.0))
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
