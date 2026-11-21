//! The renderer drawing known scenes offscreen, checked by sampling single pixels. Every test
//! runs twice: on a device with wgpu's default limits and on one with its downlevel limits,
//! the floor's feature set. wgpu's default handler for an uncaptured error panics, so a
//! validation error fails the test.

use beryl_assets::Format;
use beryl_assets::light;
use beryl_render::{BACK_LAYER, Limits, Renderer, Target};
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Mat4, Quat, Vec2, Vec3};
use kit_base::atlas::{FaceSize, Tier};
use kit_base::geometry::{CARD_HEIGHT, CARD_WIDTH, Rect, Window};
use kit_frame::frame::{Band, DRAWS, Draw, Frame, FrameBuilder, sprites};
use kit_space::camera::{Camera, DESKTOP_CAMERA};

pub(super) const BOTH: [Limits; 2] = [Limits::Default, Limits::Downlevel];
/// How far a sampled channel may be from the expected value, out of 255.
const TOLERANCE: u8 = 2;

pub(super) fn renderer(limits: Limits) -> Renderer {
    let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
    let format = Format::choose(&renderer.formats());
    renderer.set_faces(&["", "Island", "Plains", "Counterspell"], format);
    assert!(renderer.upload_page(&light::prepared()));
    renderer
}

/// A camera looking straight down on the table, with the 200 by 200 pixel target showing
/// x and y from -100 to 100 millimetres: the point (x, y) is drawn at pixel (x + 100, 100 - y).
/// Heights from -1000 to 1000 are in view; higher is nearer.
pub(super) fn overhead() -> Mat4 {
    directx::orthographic(-100.0, 100.0, -100.0, 100.0, 0.0, 2000.0)
        * look_at_mat4(Vec3::new(0.0, 0.0, 1000.0), Vec3::ZERO, Vec3::Y)
}

/// A table-material rectangle `size` millimetres large, centred at `centre`, at height `z`.
/// The table material draws its tint, so the rectangle is that solid color.
pub(super) fn rectangle(centre: Vec2, size: Vec2, z: f32, rgb: [u8; 3]) -> Draw {
    let transform = Affine3A::from_scale_rotation_translation(
        Vec3::new(size.x / CARD_WIDTH, size.y / CARD_HEIGHT, 1.0),
        Quat::IDENTITY,
        centre.extend(z),
    );
    Draw::table(transform, [rgb[0], rgb[1], rgb[2], 255])
}

/// A card lying face up at `position`, showing placeholder face `face`.
pub(super) fn card(position: Vec3, face: u16, tint: [u8; 4]) -> Draw {
    Draw::card(
        Affine3A::from_translation(position),
        tint,
        FaceSize::Board,
        face,
        BACK_LAYER,
        0.0,
        0.0,
    )
}

/// Draws `records` (each with its band, in order) seen through `view_projection` into a
/// target `width` by `height`, and returns its pixels and the draw calls it took.
fn draw(
    renderer: &mut Renderer,
    view_projection: Mat4,
    size: (u32, u32),
    records: &[(Band, Draw)],
) -> (Picture, u32) {
    draw_in(Tier::Baseline, renderer, view_projection, size, records)
}

/// `draw`, in a frame of `tier`.
pub(super) fn draw_in(
    tier: Tier,
    renderer: &mut Renderer,
    view_projection: Mat4,
    (width, height): (u32, u32),
    records: &[(Band, Draw)],
) -> (Picture, u32) {
    let mut frame = Frame::with_capacity(0, records.len(), 0);
    let mut builder = FrameBuilder::new(&mut frame, tier);
    builder.camera(view_projection);
    for &(band, record) in records {
        builder.draw(band, record);
    }
    let frame = builder.finish();
    let mut target = Target::offscreen(renderer, width, height);
    let drawn = renderer.render(frame, &mut target);
    let picture = Picture {
        pixels: renderer.read(&target),
        width,
    };
    (picture, drawn.draw_calls)
}

pub(super) struct Picture {
    pub(super) pixels: Vec<u8>,
    pub(super) width: u32,
}

impl Picture {
    pub(super) fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }

    /// The pixel showing the overhead camera's point (x, y).
    pub(super) fn overhead(&self, x: f32, y: f32) -> [u8; 3] {
        self.at((x + 100.0) as u32, (100.0 - y) as u32)
    }
}

fn close(found: [u8; 3], expected: [u8; 3]) -> bool {
    found
        .iter()
        .zip(expected)
        .all(|(&a, b)| a.abs_diff(b) <= TOLERANCE)
}

#[test]
fn solid_rectangles_draw_their_colors() {
    let background = [30, 60, 50];
    let red = [200, 40, 40];
    let green = [40, 180, 60];
    let blue = [50, 70, 220];
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let (picture, _) = draw(
            &mut renderer,
            overhead(),
            (200, 200),
            &[
                // Blue lies above red where the two overlap, though drawn after it.
                (
                    Band::Opaque,
                    rectangle(Vec2::new(-40.0, 0.0), Vec2::splat(60.0), 1.0, red),
                ),
                (
                    Band::Opaque,
                    rectangle(Vec2::new(-10.0, -40.0), Vec2::splat(40.0), 2.0, blue),
                ),
                (
                    Band::Opaque,
                    rectangle(Vec2::new(50.0, 40.0), Vec2::splat(50.0), 1.0, green),
                ),
                (
                    Band::Opaque,
                    rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, background),
                ),
            ],
        );
        for (x, y, expected) in [
            (-80.0, 80.0, background),
            (-50.0, 10.0, red),
            (50.0, 40.0, green),
            (0.0, -50.0, blue),
            (-25.0, -25.0, blue),
        ] {
            let found = picture.overhead(x, y);
            assert!(
                close(found, expected),
                "{limits:?}: at ({x}, {y}) found {found:?}, expected {expected:?}"
            );
        }
    }
}

/// Frames drawn back to back, each into its own target and read only after the last, each
/// show their own color: on Metal the renderer writes each frame's records into one of a few
/// buffers in turn, and must neither overwrite a buffer a submitted frame still reads nor
/// write one that is not mapped again yet.
#[test]
fn frames_drawn_back_to_back_keep_their_own_records() {
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let colors: Vec<[u8; 3]> = (0..8_u8).map(|i| [20 + 25 * i, 200 - 20 * i, 90]).collect();
        let targets: Vec<Target> = colors
            .iter()
            .map(|&color| {
                let mut frame = Frame::with_capacity(0, 1, 0);
                let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
                builder.camera(overhead());
                builder.draw(
                    Band::Opaque,
                    rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, color),
                );
                let frame = builder.finish();
                let mut target = Target::offscreen(&renderer, 200, 200);
                renderer.render(frame, &mut target);
                target
            })
            .collect();
        for (index, (target, expected)) in targets.iter().zip(colors).enumerate() {
            let picture = Picture {
                pixels: renderer.read(target),
                width: 200,
            };
            let found = picture.at(100, 100);
            assert!(
                close(found, expected),
                "{limits:?}: frame {index} found {found:?}, expected {expected:?}"
            );
        }
    }
}

#[test]
fn the_nearer_card_covers_the_farther_whichever_is_drawn_first() {
    let low = card(Vec3::new(-15.0, 0.0, 1.0), 1, [255, 120, 120, 255]);
    let high = card(Vec3::new(15.0, 0.0, 2.0), 2, [120, 120, 255, 255]);
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]);
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let mut picture = |records: &[Draw]| {
            let records: Vec<(Band, Draw)> =
                records.iter().map(|&draw| (Band::Opaque, draw)).collect();
            draw(&mut renderer, overhead(), (200, 200), &records).0
        };
        let low_first = picture(&[low, high, table]);
        let high_first = picture(&[high, low, table]);
        let high_alone = picture(&[high, table]);
        // The two cards overlap from x = -16.5 to 16.5.
        for y in [-20.0, 0.0, 20.0] {
            let expected = high_alone.overhead(0.0, y);
            assert_eq!(low_first.overhead(0.0, y), expected, "{limits:?} at y {y}");
            assert_eq!(high_first.overhead(0.0, y), expected, "{limits:?} at y {y}");
        }
        assert_ne!(
            low_first.overhead(-35.0, 0.0),
            high_alone.overhead(-35.0, 0.0)
        );
    }
}

#[test]
fn a_mark_on_a_covered_card_is_hidden_with_it() {
    let low = card(Vec3::new(-20.0, 0.0, 1.0), 1, [255; 4]);
    let high = card(Vec3::new(20.0, 0.0, 2.0), 2, [255; 4]);
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]);
    // A bright disc lying just above the low card, half under the high card.
    let mark = Draw::glow(
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(40.0),
            Quat::IDENTITY,
            Vec3::new(-5.0, 0.0, 1.1),
        ),
        [255, 255, 255, 255],
        sprites::DISC,
        0.0,
    );
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let opaque = [
            (Band::Opaque, low),
            (Band::Opaque, high),
            (Band::Opaque, table),
        ];
        let (without, _) = draw(&mut renderer, overhead(), (200, 200), &opaque);
        let mut marked = opaque.to_vec();
        marked.push((Band::TableMarks, mark));
        let (with, _) = draw(&mut renderer, overhead(), (200, 200), &marked);
        // Under the high card, which starts at x = -11.5, the mark is hidden.
        assert_eq!(
            with.overhead(0.0, 0.0),
            without.overhead(0.0, 0.0),
            "{limits:?}"
        );
        // On the part of the low card the high one leaves uncovered, it shows.
        assert_ne!(
            with.overhead(-14.0, 0.0),
            without.overhead(-14.0, 0.0),
            "{limits:?}"
        );
    }
}

/// A black table under the overhead camera, with `light` drawn over it as light.
fn over_black(renderer: &mut Renderer, light: Draw) -> Picture {
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]);
    let records = [(Band::Opaque, table), (Band::Glows, light)];
    draw(renderer, overhead(), (200, 200), &records).0
}

#[test]
fn a_glow_of_the_disc_is_brightest_at_its_centre() {
    let glow = Draw::glow(
        Affine3A::from_scale_rotation_translation(
            Vec3::new(160.0, 160.0, 1.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.0, 1.0),
        ),
        [255; 4],
        sprites::DISC,
        0.0,
    );
    for limits in BOTH {
        let picture = over_black(&mut renderer(limits), glow);
        let [centre, ..] = picture.overhead(0.0, 0.0);
        // The disc's quad is 160 millimetres wide, so half way out is 40 from the centre.
        let [half_way, ..] = picture.overhead(40.0, 0.0);
        assert!(centre >= 255 - 4, "{limits:?}: the centre is {centre}");
        assert!(
            centre > half_way,
            "{limits:?}: the centre is {centre} and half way out is {half_way}"
        );
    }
}

#[test]
fn a_streak_lights_only_between_its_tail_and_its_head() {
    // 160 millimetres long from x = -80 to 80, so a point `t` of the way along is at
    // x = 160 t - 80.
    let streak = Draw::streak(
        Affine3A::from_scale_rotation_translation(
            Vec3::new(160.0, 20.0, 1.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.0, 1.0),
        ),
        [255; 4],
        sprites::RIBBON,
        0.5,
        0.25,
    );
    let along = |t: f32| 160.0 * t - 80.0;
    for limits in BOTH {
        let picture = over_black(&mut renderer(limits), streak);
        let [lit, ..] = picture.overhead(along(0.4), 0.0);
        assert!(lit > 64, "{limits:?}: 0.4 of the way along is {lit}");
        for t in [0.1, 0.8] {
            assert_eq!(
                picture.overhead(along(t), 0.0),
                [0, 0, 0],
                "{limits:?}: {t} of the way along"
            );
        }
    }
}

#[test]
fn six_hundred_cards_draw_in_at_most_six_calls() {
    let mut records = Vec::new();
    for i in 0..600 {
        let position = Vec3::new(
            (i % 30) as f32 * 6.0 - 90.0,
            (i / 30) as f32 * 9.0 - 90.0,
            1.0 + i as f32 * 0.3,
        );
        records.push((Band::Opaque, card(position, (i % 3 + 1) as u16, [255; 4])));
    }
    records.push((
        Band::Opaque,
        rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]),
    ));
    for i in 0..20 {
        let shadow = Affine3A::from_translation(Vec3::new(i as f32 * 4.0, 0.0, 200.0));
        records.push((
            Band::ContactShadows,
            Draw::contact_shadow(shadow, [0, 0, 0, 140], 3.0),
        ));
    }
    for i in 0..600 {
        let edge = records[i].1.edge();
        records.push((Band::TableMarks, edge));
    }
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let (_, draw_calls) = draw(&mut renderer, overhead(), (200, 200), &records);
        assert!(draw_calls <= 6, "{limits:?}: {draw_calls} draw calls");
    }
}

/// A frame as full as the engine's largest board allows (`frame::DRAWS` records) draws every
/// record, the last included, from the instance buffer made at startup.
#[test]
fn the_largest_frame_draws_its_last_record() {
    let green = [40, 180, 60];
    let mut records: Vec<(Band, Draw)> = (0..DRAWS - 1)
        .map(|i| {
            let away = Vec2::new(1000.0 + i as f32, 1000.0);
            (Band::Opaque, rectangle(away, Vec2::splat(1.0), 0.0, [0; 3]))
        })
        .collect();
    // The only record of a blended band, so it is ordered last of all.
    records.push((
        Band::TableMarks,
        rectangle(Vec2::ZERO, Vec2::splat(40.0), 1.0, green),
    ));
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let (picture, _) = draw(&mut renderer, overhead(), (200, 200), &records);
        let found = picture.overhead(0.0, 0.0);
        assert!(
            close(found, green),
            "{limits:?}: the last record drew {found:?}, not {green:?}"
        );
    }
}

/// Twenty cards on the desktop layout's table, seen through its perspective camera in the
/// default window, three of them raised above the table with contact shadows beneath.
pub(super) struct TwentyCards {
    pub(super) camera: Camera,
    pub(super) width: u32,
    pub(super) height: u32,
    /// Each record with its band, in order.
    pub(super) records: Vec<(Band, Draw)>,
    /// Points inside each contact shadow that no card covers.
    pub(super) shadow_points: Vec<Vec3>,
    /// The centre of each card.
    pub(super) cards: Vec<Vec3>,
    /// A point on the cloth far from every card.
    pub(super) cloth: Vec3,
}

pub(super) fn twenty_cards() -> TwentyCards {
    let window = Window::default();
    let camera = Camera::fit(DESKTOP_CAMERA, window);
    let raised = [3, 9, 15];
    let mut opaque = Vec::new();
    let mut shadows = Vec::new();
    let mut edges = Vec::new();
    let mut shadow_points = Vec::new();
    let mut cards = Vec::new();
    for i in 0..20 {
        let column = (i % 10) as f32;
        let row = (i / 10) as f32;
        let mut position = Vec3::new(column * 75.0 - 340.0, row * -110.0 - 40.0, 0.0);
        let face = (i % 3 + 1) as u16;
        if raised.contains(&i) {
            position.z = 40.0;
            let under = Affine3A::from_scale_rotation_translation(
                Vec3::splat(1.15),
                Quat::IDENTITY,
                Vec3::new(position.x, position.y - 8.0, 0.6),
            );
            shadows.push((
                Band::ContactShadows,
                Draw::contact_shadow(under, [0, 0, 0, 160], 6.0),
            ));
            // Inside the shadow, toward its near edge, where the raised card does not cover it.
            shadow_points.push(Vec3::new(position.x, position.y - 8.0 - 40.0, 0.6));
        }
        let record = card(position, face, [255; 4]);
        cards.push(position);
        opaque.push((Band::Opaque, record));
        edges.push((Band::TableMarks, record.edge()));
    }
    let cloth = Affine3A::from_scale_rotation_translation(
        Vec3::new(4000.0 / CARD_WIDTH, 4000.0 / CARD_HEIGHT, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, -0.5),
    );
    opaque.push((Band::Opaque, Draw::tabletop(cloth, [255; 4])));
    TwentyCards {
        camera,
        width: window.size.width as u32,
        height: window.size.height as u32,
        records: opaque.into_iter().chain(shadows).chain(edges).collect(),
        shadow_points,
        cards,
        cloth: Vec3::new(0.0, 180.0, -0.5),
    }
}

/// The twenty cards, written to a PNG picture.
#[test]
fn twenty_cards_with_three_raised_draw_in_perspective() {
    let TwentyCards {
        camera,
        width,
        height,
        records,
        shadow_points,
        cloth: cloth_point,
        ..
    } = twenty_cards();
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let mut frame = Frame::with_capacity(0, records.len(), 0);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        builder.camera(camera.view_projection);
        for &(band, record) in &records {
            builder.draw(band, record);
        }
        let frame = builder.finish();
        let mut target = Target::offscreen(&renderer, width, height);
        let drawn = renderer.render(frame, &mut target);
        assert_eq!(drawn.draw_calls, 3, "{limits:?}");
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("twenty-cards-{limits:?}.png"));
        renderer
            .write_png(&target, &path)
            .expect("the picture is written");
        println!("{}", path.display());
        let picture = Picture {
            pixels: renderer.read(&target),
            width,
        };
        let pixel = |point: Vec3| {
            let p = camera.to_screen(point);
            picture.at(p.x as u32, p.y as u32)
        };
        // The cloth far from every card is the tabletop's warm brown (`tabletop.rs` checks its
        // gradient), and darker where a shadow falls on it than in the same place unshadowed.
        let cloth = pixel(cloth_point);
        assert!(
            cloth[0] > 90 && cloth[0] > cloth[2] + 15,
            "{limits:?}: {cloth:?}"
        );
        let unshadowed: Vec<(Band, Draw)> = records
            .iter()
            .copied()
            .filter(|&(band, _)| band != Band::ContactShadows)
            .collect();
        let (bare, _) = draw(
            &mut renderer,
            camera.view_projection,
            (width, height),
            &unshadowed,
        );
        for point in &shadow_points {
            let shaded = pixel(*point);
            let p = camera.to_screen(*point);
            let open = bare.at(p.x as u32, p.y as u32);
            assert!(
                shaded.iter().zip(open).all(|(&s, c)| s + 8 < c),
                "{limits:?}: the shadow at {point} is {shaded:?}, the cloth {open:?}"
            );
        }
    }
}

/// A card half dissolved has broken up from the top down: near its top the table shows
/// through, and near its bottom it is drawn as it is whole.
#[test]
fn a_half_dissolved_card_is_gone_at_its_top_and_whole_at_its_bottom() {
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]);
    let at = |dissolve: f32| {
        Draw::card(
            Affine3A::from_translation(Vec3::new(0.0, 0.0, 1.0)),
            [255; 4],
            FaceSize::Board,
            1,
            BACK_LAYER,
            dissolve,
            0.0,
        )
    };
    // The card reaches from y = -44 to 44.
    let (top, bottom) = (40.0, -40.0);
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let mut picture = |dissolve: f32| {
            let records = [(Band::Opaque, table), (Band::TableMarks, at(dissolve))];
            draw(&mut renderer, overhead(), (200, 200), &records).0
        };
        let whole = picture(0.0);
        let half = picture(0.5);
        let black = [0, 0, 0];
        assert!(!close(whole.overhead(0.0, top), black), "{limits:?}");
        assert!(
            close(half.overhead(0.0, top), black),
            "{limits:?}: near its top the card is {:?}",
            half.overhead(0.0, top)
        );
        assert!(
            close(half.overhead(0.0, bottom), whole.overhead(0.0, bottom)),
            "{limits:?}: near its bottom the card is {:?}, whole {:?}",
            half.overhead(0.0, bottom),
            whole.overhead(0.0, bottom)
        );
    }
}

/// A frame shaken by (8, 0) draws what is on the table 8 pixels to the right, and the flat
/// layer where it was.
#[test]
fn a_shake_moves_the_table_and_not_the_flat_layer() {
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]);
    let lying = card(Vec3::new(-40.0, 0.0, 1.0), 1, [255; 4]);
    // A flat-layer panel, drawn with no depth test after everything on the table.
    let panel = rectangle(
        Vec2::new(50.0, 0.0),
        Vec2::splat(40.0),
        500.0,
        [40, 180, 60],
    );
    let records = [
        (Band::Opaque, lying),
        (Band::Opaque, table),
        (Band::Layer, panel),
    ];
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let mut picture = |shake: Vec2| {
            let mut frame = Frame::with_capacity(0, records.len(), 0);
            let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
            builder.camera(overhead());
            builder.shake(shake);
            for &(band, record) in &records {
                builder.draw(band, record);
            }
            let frame = builder.finish();
            let mut target = Target::offscreen(&renderer, 200, 200);
            renderer.render(frame, &mut target);
            Picture {
                pixels: renderer.read(&target),
                width: 200,
            }
        };
        let still = picture(Vec2::ZERO);
        let shaken = picture(Vec2::new(8.0, 0.0));
        let black = [0, 0, 0];
        // The card's left edge is at x = -71.5, pixel 28.5.
        for y in [-30, 0, 30] {
            let row = (100 - y) as u32;
            assert!(!close(still.at(31, row), black), "{limits:?}");
            assert!(close(shaken.at(31, row), black), "{limits:?} at row {row}");
            for x in [31, 40, 50] {
                assert!(
                    close(shaken.at(x + 8, row), still.at(x, row)),
                    "{limits:?}: ({x}, {row}) is not drawn 8 pixels to the right"
                );
            }
            // The panel's left edge is at x = 30, pixel 130.
            for x in [128, 132, 140] {
                assert_eq!(
                    shaken.at(x, row),
                    still.at(x, row),
                    "{limits:?} at ({x}, {row})"
                );
            }
        }
    }
}

/// A table-space card shown over a screen panel can be halfway through the panel's clip.
#[test]
fn a_shown_card_draws_only_inside_its_elements_clip() {
    let background = [30, 70, 110];
    for limits in BOTH {
        let mut renderer = renderer(limits);
        for tier in [Tier::Baseline, Tier::Enhanced] {
            let mut frame = Frame::with_capacity(0, 2, 0);
            let mut builder = FrameBuilder::new(&mut frame, tier);
            builder.camera(overhead());
            let clip = builder
                .scissor(Rect {
                    x: 0.0,
                    y: 100.0,
                    width: 200.0,
                    height: 100.0,
                })
                .unwrap();
            builder.draw(
                Band::Layer,
                rectangle(Vec2::ZERO, Vec2::splat(200.0), 0.0, background),
            );
            builder.draw(Band::LayerCards, card(Vec3::Z, 1, [255; 4]).with_clip(clip));
            let frame = builder.finish();
            let mut target = Target::offscreen(&renderer, 200, 200);
            renderer.render(frame, &mut target);
            let picture = Picture {
                pixels: renderer.read(&target),
                width: 200,
            };
            let panel = picture.at(40, 80);
            assert_eq!(picture.at(100, 80), panel, "{limits:?} {tier:?}");
            assert_ne!(picture.at(100, 120), panel, "{limits:?} {tier:?}");
        }
    }
}

#[test]
fn a_flight_draws_in_front_of_nearer_resting_cards_and_behind_panels() {
    let nearer = card(Vec3::Z * 10.0, 1, [0, 0, 255, 255]);
    let flying = card(Vec3::Z, 2, [255, 0, 0, 255]);
    let panel = rectangle(Vec2::new(0.0, 10.0), Vec2::splat(16.0), 0.0, [40, 180, 60]);
    let records = [
        (Band::MovingOpaque, flying),
        (Band::Opaque, nearer),
        (Band::Layer, panel),
    ];
    for limits in BOTH {
        let mut renderer = renderer(limits);
        for tier in [Tier::Baseline, Tier::Enhanced] {
            let (resting, _) = draw_in(tier, &mut renderer, overhead(), (200, 200), &records[1..]);
            let (frame, _) = draw_in(tier, &mut renderer, overhead(), (200, 200), &records);
            // The nearer card can cast a shadow in Enhanced. Its color still tells us which
            // card painted the pixel, independent of the amount of light reaching it.
            let [red, _, blue] = resting.at(100, 110);
            assert!(blue > red, "{limits:?} {tier:?}");
            let [red, _, blue] = frame.at(100, 110);
            assert!(red > blue, "{limits:?} {tier:?}");
            assert_eq!(
                frame.at(100, 90),
                resting.at(100, 90),
                "{limits:?} {tier:?}"
            );
        }
    }
}
