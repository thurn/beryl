//! The table's cloth painted as the web interface's `--tabletop`, checked against the token's
//! own gradient in `data/design-tokens.json`, so that the shader's copy of the stops cannot drift
//! from it unseen.

use beryl_render::Target;
use beryl_scene::frame::{Band, Draw, Frame, FrameBuilder};
use beryl_scene::input::Tier;
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH};
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Mat4, Quat, Vec2, Vec3};
use serde_json::Value;

use super::render::{BOTH, Picture, renderer};

const TOKENS: &str = include_str!("../../../../../../data/design-tokens.json");
/// How much of the gradient's color the web's grain leaves on average, as the shader's `GRAIN`
/// says: measured over the web interface's tabletop at scale 2.
const GRAIN: f64 = 0.952;
/// How far a channel may be from the token's color, out of 255: the shader's stops are the
/// token's bytes, and the rest is rounding.
const TOLERANCE: u8 = 2;

/// The `--tabletop` token's color stops, each an offset (1 is 100%) and its color.
fn stops() -> Vec<(f64, [f64; 3])> {
    let tokens: Value = serde_json::from_str(TOKENS).expect("design-tokens.json is JSON");
    let token = tokens["tokens"]
        .as_array()
        .expect("`tokens` is a list")
        .iter()
        .find(|token| token["name"] == "tabletop")
        .expect("there is a `tabletop` token");
    let css = token["value"].as_str().expect("its value is a string");
    let inner = css
        .split_once("radial-gradient(")
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .expect("the tabletop is a radial gradient");
    let mut parts = inner.split(',').map(str::trim);
    assert_eq!(
        parts.next(),
        Some("ellipse 50% 50% at 50% 50%"),
        "the shader draws the gradient centred on the window, spanning it"
    );
    parts
        .enumerate()
        .map(|(i, part)| {
            let (hex, offset) = part.split_once(' ').unwrap_or((part, "0%"));
            assert!(i == 0 || offset != "0%", "stop `{part}` has no offset");
            let offset: f64 = offset
                .strip_suffix('%')
                .and_then(|percent| percent.parse().ok())
                .unwrap_or_else(|| panic!("`{offset}` is not a percentage"));
            let hex = hex.strip_prefix('#').expect("each stop is a hex color");
            let channel = |at: usize| f64::from(u8::from_str_radix(&hex[at..at + 2], 16).unwrap());
            (offset / 100.0, [channel(0), channel(2), channel(4)])
        })
        .collect()
}

/// The web's tabletop at pixel `pixel` (its centre) of a window `size` pixels large.
fn expected(stops: &[(f64, [f64; 3])], pixel: (u32, u32), size: (u32, u32)) -> [u8; 3] {
    let x = (f64::from(pixel.0) + 0.5) / f64::from(size.0) * 2.0 - 1.0;
    let y = (f64::from(pixel.1) + 0.5) / f64::from(size.1) * 2.0 - 1.0;
    let t = x.hypot(y);
    let mut color = stops[0].1;
    for pair in stops.windows(2) {
        let ((low, _), (high, next)) = (pair[0], pair[1]);
        let along = ((t - low) / (high - low)).clamp(0.0, 1.0);
        for (channel, next) in color.iter_mut().zip(next) {
            *channel += (next - *channel) * along;
        }
    }
    color.map(|channel| (channel * GRAIN).round() as u8)
}

/// A camera looking straight down on the cloth, whatever the target's shape.
fn camera() -> Mat4 {
    directx::orthographic(-100.0, 100.0, -100.0, 100.0, 0.0, 2000.0)
        * look_at_mat4(Vec3::new(0.0, 0.0, 1000.0), Vec3::ZERO, Vec3::Y)
}

/// The cloth, far larger than the view, in a wide target, shaken by `shake` pixels.
fn draw_cloth(renderer: &mut beryl_render::Renderer, size: (u32, u32), shake: Vec2) -> Picture {
    draw_shaded_cloth(renderer, size, shake, None)
}

/// The cloth as `draw_cloth` draws it, darkened by the far shade at `far_shade`'s centre and
/// radius across, in pixels, if given.
fn draw_shaded_cloth(
    renderer: &mut beryl_render::Renderer,
    size: (u32, u32),
    shake: Vec2,
    far_shade: Option<(Vec2, f32)>,
) -> Picture {
    let mut cloth = Draw::tabletop(
        Affine3A::from_scale_rotation_translation(
            Vec3::new(4000.0 / CARD_WIDTH, 4000.0 / CARD_HEIGHT, 1.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.0, -0.5),
        ),
        [255; 4],
    );
    if let Some((centre, radius)) = far_shade {
        cloth = cloth.with_far_shade(centre, radius);
    }
    let mut frame = Frame::with_capacity(0, 1, 0);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    builder.camera(camera());
    builder.shake(shake);
    builder.draw(Band::Opaque, cloth);
    let frame = builder.finish();
    let mut target = Target::offscreen(renderer, size.0, size.1);
    renderer.render(frame, &mut target);
    Picture {
        pixels: renderer.read(&target),
        width: size.0,
    }
}

/// Across a wide window, from the middle out to the corners, every pixel of the cloth is the
/// token's gradient times the grain's darkening: an ellipse as wide and tall as the window.
#[test]
fn the_cloth_paints_the_tabletop() {
    let stops = stops();
    assert_eq!(stops.len(), 11, "the shader's `tabletop_at` holds 11 stops");
    let size = (320, 180);
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let picture = draw_cloth(&mut renderer, size, Vec2::ZERO);
        let mut faults = Vec::new();
        for y in (0..size.1).step_by(7) {
            for x in (0..size.0).step_by(7) {
                let found = picture.at(x, y);
                let wanted = expected(&stops, (x, y), size);
                if found
                    .iter()
                    .zip(wanted)
                    .any(|(&f, w)| f.abs_diff(w) > TOLERANCE)
                {
                    faults.push(format!("({x}, {y}): {found:?}, not {wanted:?}"));
                }
            }
        }
        assert!(faults.is_empty(), "{limits:?}: {}", faults.join("; "));
        // The far corners are near black, and the middle warm brown.
        assert!(picture.at(0, 0)[0] < 35, "{limits:?}");
        assert!(picture.at(160, 90)[0] > 120, "{limits:?}");
    }
}

/// A shake moves the cloth's picture with the table, as the web's cloth moves with its stage.
#[test]
fn a_shake_moves_the_tabletop_with_the_table() {
    let size = (320, 180);
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let still = draw_cloth(&mut renderer, size, Vec2::ZERO);
        let shaken = draw_cloth(&mut renderer, size, Vec2::new(12.0, -6.0));
        for (x, y) in [(20, 30), (100, 60), (250, 150)] {
            assert_eq!(
                shaken.at(x + 12, y - 6),
                still.at(x, y),
                "{limits:?} at ({x}, {y})"
            );
        }
    }
}

/// The far shade's opacity `t` of the way out to its ellipse's edge, as the web's `.far-shade`
/// gives its stops in `ui/src/screens/styles/table.css`.
fn far_shade_opacity(t: f64) -> f64 {
    let stops = [(0.0, 0.8), (0.5, 0.64), (0.8, 0.32), (1.0, 0.0)];
    let mut opacity = stops[0].1;
    for pair in stops.windows(2) {
        let ((low, from), (high, to)) = (pair[0], pair[1]);
        if t > low {
            opacity = from + (to - from) * ((t - low) / (high - low)).min(1.0);
        }
    }
    opacity
}

/// The far shade darkens the cloth toward `rgb(10 7 5)` by the web's stops along an ellipse
/// 300 by 85 in shape, and leaves the cloth beyond the ellipse as it was.
#[test]
fn the_far_shade_darkens_the_cloth_by_the_webs_stops() {
    let stops = stops();
    let size = (320, 180);
    let centre = Vec2::new(160.0, 20.0);
    let radius = 120.0;
    let aspect = 85.0 / 300.0;
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let plain = draw_cloth(&mut renderer, size, Vec2::ZERO);
        let shaded = draw_shaded_cloth(&mut renderer, size, Vec2::ZERO, Some((centre, radius)));
        let mut faults = Vec::new();
        for y in (0..60).step_by(3) {
            for x in (20..300).step_by(9) {
                let dx = (f64::from(x) + 0.5 - f64::from(centre.x)) / f64::from(radius);
                let dy = (f64::from(y) + 0.5 - f64::from(centre.y)) / (f64::from(radius) * aspect);
                let opacity = far_shade_opacity(dx.hypot(dy));
                let cloth = expected(&stops, (x, y), size);
                let shade = [10.0, 7.0, 5.0];
                let wanted: [u8; 3] = std::array::from_fn(|at| {
                    let c = f64::from(cloth[at]);
                    (c + (shade[at] - c) * opacity).round() as u8
                });
                let found = shaded.at(x, y);
                if found
                    .iter()
                    .zip(wanted)
                    .any(|(&f, w)| f.abs_diff(w) > TOLERANCE)
                {
                    faults.push(format!("({x}, {y}): {found:?}, not {wanted:?}"));
                }
            }
        }
        assert!(faults.is_empty(), "{limits:?}: {}", faults.join("; "));
        assert!(
            shaded.at(160, 20)[0] < 40,
            "{limits:?}: the centre is near black"
        );
        assert_eq!(
            shaded.at(160, 120),
            plain.at(160, 120),
            "{limits:?}: below it"
        );
        assert_eq!(shaded.at(10, 20), plain.at(10, 20), "{limits:?}: beside it");
    }
}
