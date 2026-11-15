//! The renderer drawing known scenes offscreen, checked by sampling single pixels. Every test
//! runs twice: on a device with wgpu's default limits and on one with its downlevel limits,
//! the floor's feature set. wgpu's default handler for an uncaptured error panics, so a
//! validation error fails the test.

use beryl_assets::Format;
use beryl_render::{BACK_LAYER, GLOW_SPRITE, Limits, Renderer, Target};
use beryl_scene::frame::{Band, DRAWS, Draw, Frame, FrameBuilder};
use beryl_scene::input::FaceSize;
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH, Camera, DESKTOP, Window};
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Mat4, Quat, Vec2, Vec3};

const BOTH: [Limits; 2] = [Limits::Default, Limits::Downlevel];
/// How far a sampled channel may be from the expected value, out of 255.
const TOLERANCE: u8 = 2;

fn renderer(limits: Limits) -> Renderer {
    let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
    let format = Format::choose(&renderer.formats());
    renderer.set_faces(&["", "Island", "Plains", "Counterspell"], format);
    renderer
}

/// A camera looking straight down on the table, with the 200 by 200 pixel target showing
/// x and y from -100 to 100 millimetres: the point (x, y) is drawn at pixel (x + 100, 100 - y).
/// Heights from -1000 to 1000 are in view; higher is nearer.
fn overhead() -> Mat4 {
    directx::orthographic(-100.0, 100.0, -100.0, 100.0, 0.0, 2000.0)
        * look_at_mat4(Vec3::new(0.0, 0.0, 1000.0), Vec3::ZERO, Vec3::Y)
}

/// A table-material rectangle `size` millimetres large, centred at `centre`, at height `z`.
/// The table material draws its tint, so the rectangle is that solid color.
fn rectangle(centre: Vec2, size: Vec2, z: f32, rgb: [u8; 3]) -> Draw {
    let transform = Affine3A::from_scale_rotation_translation(
        Vec3::new(size.x / CARD_WIDTH, size.y / CARD_HEIGHT, 1.0),
        Quat::IDENTITY,
        centre.extend(z),
    );
    Draw::table(transform, [rgb[0], rgb[1], rgb[2], 255])
}

/// A card lying face up at `position`, showing placeholder face `face`.
fn card(position: Vec3, face: u16, tint: [u8; 4]) -> Draw {
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
    (width, height): (u32, u32),
    records: &[(Band, Draw)],
) -> (Picture, u32) {
    let mut frame = Frame::with_capacity(0, records.len(), 0);
    let mut builder = FrameBuilder::new(&mut frame);
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

struct Picture {
    pixels: Vec<u8>,
    width: u32,
}

impl Picture {
    fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }

    /// The pixel showing the overhead camera's point (x, y).
    fn overhead(&self, x: f32, y: f32) -> [u8; 3] {
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
        GLOW_SPRITE,
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

/// Twenty cards on the desktop layout's table through its perspective camera, three of them
/// raised above the table with contact shadows beneath, written to a PNG picture.
#[test]
fn twenty_cards_with_three_raised_draw_in_perspective() {
    let window = Window::default();
    let camera = Camera::fit(DESKTOP.camera, window);
    let (width, height) = (window.size.width as u32, window.size.height as u32);
    let raised = [3, 9, 15];
    let mut opaque = Vec::new();
    let mut shadows = Vec::new();
    let mut edges = Vec::new();
    let mut shadow_points = Vec::new();
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
        opaque.push((Band::Opaque, record));
        edges.push((Band::TableMarks, record.edge()));
    }
    let cloth = Affine3A::from_scale_rotation_translation(
        Vec3::new(4000.0 / CARD_WIDTH, 4000.0 / CARD_HEIGHT, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, -0.5),
    );
    let cloth_rgb = [38, 66, 58];
    opaque.push((Band::Opaque, Draw::table(cloth, [38, 66, 58, 255])));
    let records: Vec<(Band, Draw)> = opaque.into_iter().chain(shadows).chain(edges).collect();
    for limits in BOTH {
        let mut renderer = renderer(limits);
        let mut frame = Frame::with_capacity(0, records.len(), 0);
        let mut builder = FrameBuilder::new(&mut frame);
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
        // The cloth far from every card is its own color.
        assert!(
            close(pixel(Vec3::new(0.0, 180.0, -0.5)), cloth_rgb),
            "{limits:?}"
        );
        for point in &shadow_points {
            let shaded = pixel(*point);
            assert!(
                shaded.iter().zip(cloth_rgb).all(|(&s, c)| s + 8 < c),
                "{limits:?}: the shadow at {point} is {shaded:?}"
            );
        }
    }
}
