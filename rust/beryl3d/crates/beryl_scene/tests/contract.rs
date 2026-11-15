//! The contract's types hold their promised shapes.

#![expect(
    clippy::float_cmp,
    reason = "the constructors copy their arguments, so the values are exact"
)]

use beryl_scene::command::Reply;
use beryl_scene::frame::{Draw, Frame, Material, Sprite, Wake};
use beryl_scene::input::{
    Button, FaceSize, Input, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size,
};
use beryl_scene::{Client, Config};
use glam::{Affine3A, Vec2, Vec3};

#[test]
fn a_draw_record_is_80_bytes() {
    assert_eq!(size_of::<Draw>(), 80);
}

const TINT: [u8; 4] = [10, 20, 30, 40];
const SPRITE: Sprite = Sprite {
    page: 3,
    rect: [1, 2, 3, 4],
};

fn transform() -> Affine3A {
    Affine3A::from_translation(Vec3::new(1.0, 2.0, 3.0))
}

/// Each constructor writes the given material, the transform and the tint, and the `uv`,
/// `layer` and `params` the doc's table gives that material, with every other field at zero.
#[test]
fn each_constructor_fills_only_its_materials_fields() {
    let cases = [
        (
            Draw::table(transform(), TINT),
            Material::Table,
            [0; 4],
            0,
            [0.0; 4],
        ),
        (
            Draw::card(transform(), TINT, FaceSize::Board, 7, 9, 0.25, 0.5),
            Material::Card,
            [9, 0, 0, 0],
            7,
            [0.25, 0.5, 0.0, 0.0],
        ),
        (
            Draw::contact_shadow(transform(), TINT, 4.0),
            Material::ContactShadow,
            [0; 4],
            0,
            [4.0, 0.0, 0.0, 0.0],
        ),
        (
            Draw::glow(transform(), TINT, SPRITE, 0.5),
            Material::Glow,
            SPRITE.rect,
            3,
            [0.5, 0.0, 0.0, 0.0],
        ),
        (
            Draw::streak(transform(), TINT, SPRITE, 0.5, 0.25),
            Material::Streak,
            SPRITE.rect,
            3,
            [0.5, 0.25, 0.0, 0.0],
        ),
        (
            Draw::panel(
                transform(),
                TINT,
                [255, 1, 2, 3],
                Vec2::new(100.0, 40.0),
                6.0,
                1.0,
            ),
            Material::Panel,
            [255, 1, 2, 3],
            0,
            [100.0, 40.0, 6.0, 1.0],
        ),
        (
            Draw::glyph(transform(), TINT, SPRITE),
            Material::Glyph,
            SPRITE.rect,
            3,
            [0.0; 4],
        ),
        (
            Draw::icon(transform(), TINT, SPRITE),
            Material::Icon,
            SPRITE.rect,
            3,
            [0.0; 4],
        ),
        (
            Draw::particle(transform(), TINT, SPRITE, 0.75, 11.0),
            Material::Particle,
            SPRITE.rect,
            3,
            [0.75, 11.0, 0.0, 0.0],
        ),
    ];
    for (draw, material, uv, layer, params) in cases {
        assert_eq!(draw.material, material as u8, "{material:?}");
        assert_eq!(draw.uv, uv, "{material:?}");
        assert_eq!(draw.layer, layer, "{material:?}");
        assert_eq!(draw.params, params, "{material:?}");
        assert_eq!(draw.tint, TINT, "{material:?}");
        assert_eq!(draw.clip(), 0, "{material:?}");
        assert_eq!(
            draw.model,
            [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1.0, 3.0],
            "{material:?}"
        );
    }
}

#[test]
fn a_clip_keeps_the_texture_array_and_mesh() {
    let draw = Draw::glyph(transform(), TINT, SPRITE);
    let clipped = draw.with_clip(5);
    assert_eq!(clipped.clip(), 5);
    assert_eq!(clipped.flags & 0x0f, draw.flags & 0x0f);
}

fn inputs() -> Vec<Input> {
    vec![
        Input::Surface {
            size: Size {
                width: 1280.0,
                height: 800.0,
            },
            scale: 2.0,
            safe_area: Insets::default(),
        },
        Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase: PointerPhase::Move,
            position: Vec2::new(300.0, 200.0),
            button: Button::None,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }),
        Input::Reply(Reply::Done(Ok(()))),
    ]
}

fn play() -> Vec<Frame> {
    let mut client = Client::new(Config {
        elements: 64,
        draws: 64,
        name_bytes: 1024,
    });
    let mut frames = Vec::new();
    for (step, input) in inputs().into_iter().enumerate() {
        client.handle(input);
        assert_eq!(client.wake(), Wake::Now);
        let now = 16_666_667 * u64::try_from(step).unwrap();
        frames.push(client.frame(now).clone());
        assert_eq!(client.wake(), Wake::Never);
    }
    frames
}

#[test]
fn the_same_inputs_and_times_give_the_same_frames() {
    assert_eq!(play(), play());
}
