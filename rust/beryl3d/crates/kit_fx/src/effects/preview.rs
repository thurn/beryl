//! The effects on a plain rectangle in a toolkit scene.

use glam::{Vec2, Vec3};

use super::{Effect, EffectRun, Reach, card, player};
use kit_base::data::effect::{Effects, NumberOrigin, Primitive};
use kit_base::geometry::{Rect, plane::ScreenPlane};
use kit_base::pose::Pose;
use kit_base::tokens::color;
use kit_frame::frame::{Band, Draw, FrameBuilder};
use kit_ui::element::{FloatId, text};
use kit_ui::input::Tier;
use kit_ui::layout::{FloatLook, Layer};

/// The length of the standalone preview, in nanoseconds.
pub const DURATION: u64 = 600_000_000;

/// Whether the rectangle moves to show an effect that follows its path.
#[must_use]
pub fn moves(data: &Effects, effect: Effect) -> bool {
    has_layer(data, effect, |layer| {
        matches!(layer, Primitive::Streak { .. })
    })
}

/// The position of the rectangle on the preview clock.
#[must_use]
pub fn position(data: &Effects, effect: Effect, rect: Rect, now: u64) -> Rect {
    if !moves(data, effect) {
        return rect;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "elapsed nanoseconds in a subsecond preview"
    )]
    let progress = now.min(DURATION) as f32 / DURATION as f32;
    let distance = rect.width * 3.0 * progress;
    Rect {
        x: rect.x + distance,
        ..rect
    }
}

/// Draws a painted rectangle and the effect on it. A color wash uses the same tint and
/// fade as a card. A previous-face layer swaps between two solid paints.
pub fn write(
    data: &Effects,
    effect: Effect,
    rect: Rect,
    now: u64,
    plane: &ScreenPlane,
    builder: &mut FrameBuilder<'_>,
) {
    let run = EffectRun::preview(effect, DURATION);
    let on = run.on;
    let runs = [run];
    let reach = |before| {
        let rect = position(data, effect, rect, now.saturating_sub(before));
        Reach {
            pose: Pose {
                position: plane.layer_transform(rect).translation.into(),
                ..Pose::default()
            },
            half: Vec2::new(rect.width, rect.height) / 2.0,
        }
    };
    let mut look = card::Look {
        pose: reach(0).pose,
        tint: [75, 104, 144],
        dissolve: 0.0,
        whiten: 0.0,
        face: Some(0),
        full: None,
        state: 0,
        interactive: false,
    };
    card::look((data, builder.tier()), &runs, on, now, Some(1), &mut look);
    if look.face == Some(1) {
        look.tint = [112, 77, 131];
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "normalized channels"
    )]
    let byte = |value: f32| value.round() as u8;
    let [r, g, b] = look
        .tint
        .map(|channel| byte(f32::from(channel) + (255.0 - f32::from(channel)) * look.whiten));
    let alpha = byte(255.0 * look.pose.opacity * (1.0 - look.dissolve));
    let shown = position(data, effect, rect, now);
    builder.draw(
        Band::RaisedMarks,
        Draw::panel(
            plane.layer_transform(shown),
            [r, g, b, alpha],
            [0; 4],
            Vec2::new(rect.width, rect.height),
            0.0,
            0.0,
        ),
    );
    card::write(data, &runs, on, now, &look, builder);
    super::write(
        data,
        &runs,
        now,
        |_, before| Some(reach(before)),
        (Vec3::ZERO, plane.scale()),
        builder,
    );
}

const TOTAL: FloatId = FloatId(0);
const CHANGE: FloatId = FloatId(1);

/// Lays out the sample total and change before the effect starts.
pub fn prepare_text(data: &Effects, effect: Effect, layer: &mut Layer) {
    if has_layer(data, effect, |layer| {
        matches!(layer, Primitive::Number { .. })
    }) {
        layer.prepare(TOTAL, 0, || text("20"));
        layer.prepare(CHANGE, 0, || text("+3"));
        layer.show_prepared(TOTAL, Some(0));
    }
}

/// Positions the prepared sample text and its changing number on the story clock.
pub fn position_text(
    data: &Effects,
    effect: Effect,
    rect: Rect,
    now: u64,
    tier: Tier,
    layer: &mut Layer,
) {
    let look = player::text(
        data,
        EffectRun::preview(effect, DURATION),
        now,
        tier,
        color::LIFE,
    )
    .unwrap_or_default();
    let center = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    layer.set_float_look(
        TOTAL,
        FloatLook {
            point: center,
            anchor: Vec2::splat(0.5),
            tint: look.tint,
            ..FloatLook::default()
        },
    );
    layer.show_prepared(CHANGE, look.number.map(|_| 0));
    if let Some(number) = look.number {
        let above = number.origin == NumberOrigin::Above;
        let gap = (rect.height * 0.5 + 6.0).copysign(if above { -1.0 } else { 1.0 });
        let point = center + Vec2::new(0.0, gap + 32.0 * number.drift);
        let anchor = Vec2::new(0.5, if above { 1.0 } else { 0.0 });
        layer.set_float_look(CHANGE, number.place(point, anchor, look.tint));
    }
}

fn has_layer(data: &Effects, effect: Effect, matches: impl Fn(&Primitive) -> bool) -> bool {
    data.get(effect.name()).is_some_and(|definition| {
        definition
            .layers
            .iter()
            .any(|layer| matches(&layer.primitive))
    })
}
