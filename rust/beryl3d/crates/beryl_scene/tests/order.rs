//! `FrameBuilder::finish` puts records in drawing order, counts the frame's load and allocates
//! nothing once the frame has its capacity.

// A global allocator is an `unsafe impl`; it only forwards to the system allocator.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use beryl_scene::frame::{Band, CLIPS, DRAWS, Draw, Frame, FrameBuilder, OBJECTS, Sprite, sprites};
use beryl_scene::input::{FaceSize, Rect};
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH};
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Mat4, Vec2, Vec3};

/// Counts the allocations made on the thread that turned counting on, so that other tests
/// running at the same time are not counted.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

fn count() {
    if COUNTING.with(Cell::get) {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// A tint whose red channel names the record, so a test can read back the order.
fn tag(id: u8) -> [u8; 4] {
    [id, 0, 0, 255]
}

fn at(depth: f32) -> Affine3A {
    Affine3A::from_translation(Vec3::new(0.0, 0.0, depth))
}

fn card(id: u8, depth: f32, size: FaceSize) -> Draw {
    Draw::card(at(depth), tag(id), size, 1, 0, 0.0, 0.0)
}

fn quad(id: u8, depth: f32) -> Draw {
    Draw::glyph(at(depth), tag(id), Sprite::default())
}

#[test]
fn records_are_put_in_drawing_order_and_cut_into_draw_calls() {
    let mut frame = Frame::with_capacity(0, 32, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    // With the identity camera, a record's depth is its z.
    builder.camera(Mat4::IDENTITY);
    builder.draw(Band::LayerCover, quad(14, 0.0));
    builder.draw(Band::Layer, quad(11, 0.9));
    builder.draw(Band::Layer, quad(12, 0.1));
    builder.draw(Band::Opaque, Draw::table(at(0.95), tag(3)));
    builder.draw(Band::Opaque, card(2, 0.5, FaceSize::Board));
    builder.draw(
        Band::Glows,
        Draw::glow(at(0.4), tag(10), Sprite::default(), 0.5),
    );
    builder.draw(Band::Opaque, card(1, 0.2, FaceSize::Board));
    builder.draw(Band::RaisedMarks, quad(9, 0.1));
    builder.draw(Band::TableMarks, card(8, 0.5, FaceSize::Board).edge());
    builder.draw(
        Band::ContactShadows,
        Draw::contact_shadow(at(0.3), tag(5), 1.0),
    );
    builder.draw(
        Band::ContactShadows,
        Draw::contact_shadow(at(0.6), tag(4), 1.0),
    );
    builder.draw(Band::RaisedMarks, quad(6, 0.7));
    builder.draw(Band::LayerCards, card(13, 0.0, FaceSize::Full));
    let frame = builder.finish();

    let order: Vec<u8> = frame.draws.iter().map(|draw| draw.tint[0]).collect();
    assert_eq!(
        order,
        [
            // Opaque: cards nearest first, then the table.
            1, 2, 3, //
            // Contact shadows, farthest first.
            4, 5, //
            // Soft edges, then raised marks farthest first, then glows.
            8, 6, 9, 10, //
            // The flat layer in the order it was pushed, then cards in it, then what covers them.
            11, 12, 13, 14,
        ]
    );
    assert_eq!(frame.opaque, 3);
    assert_eq!(frame.layer, 9);
    // The opaque records are one call. The shadows share the card mesh and the atlas, the edge
    // strip is its own mesh, and the marks and the glow share the quad and the atlas. The flat
    // layer starts a new call, and the full-size card and the panel that covers it are a call
    // each.
    assert_eq!(frame.load.draw_calls, 7);
    assert_eq!(frame.load.instances, 13);
    assert_eq!(frame.load.upload_bytes, 13 * 80);
}

#[test]
fn a_clip_ends_a_draw_call() {
    let mut frame = Frame::with_capacity(0, 8, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    let clip = builder.clip(Rect::default()).unwrap();
    builder.draw(Band::Layer, quad(1, 0.0));
    builder.draw(Band::Layer, quad(2, 0.0).with_clip(clip));
    builder.draw(Band::Layer, quad(3, 0.0).with_clip(clip));
    builder.draw(Band::Layer, quad(4, 0.0));
    assert_eq!(builder.finish().load.draw_calls, 3);
}

#[test]
fn screen_areas_are_counted_in_screens() {
    let mut frame = Frame::with_capacity(0, 8, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    builder.camera(Mat4::IDENTITY);
    // A card scaled to half the screen's width and height covers a quarter of a screen, which
    // is two units wide in clip space.
    let half_card = Affine3A::from_scale(Vec3::new(1.0 / CARD_WIDTH, 1.0 / CARD_HEIGHT, 1.0));
    builder.draw(
        Band::Opaque,
        Draw::card(half_card, tag(1), FaceSize::Board, 0, 0, 0.0, 0.0),
    );
    // A unit quad covers a quarter of the screen.
    builder.draw(Band::Glows, quad(2, 0.0));
    // A quad four screens across covers the whole screen and no more.
    builder.draw(
        Band::Layer,
        Draw::panel(
            Affine3A::from_scale(Vec3::new(8.0, 8.0, 1.0)),
            tag(3),
            [0; 4],
            Vec2::ONE,
            0.0,
            0.0,
        ),
    );
    // A quad wholly behind the camera covers nothing.
    builder.draw(Band::Glows, quad(4, -1.0));
    let areas = builder.finish().areas();
    assert!((areas.opaque - 0.25).abs() < 1e-6, "{areas:?}");
    assert!((areas.blended - 1.25).abs() < 1e-6, "{areas:?}");
}

/// Where a small record lies with the identity camera: `x` across the screen, which is 2 wide,
/// and `depth` into it. `size` is how far across it is.
fn small(x: f32, depth: f32, size: Vec2) -> Affine3A {
    Affine3A::from_scale_rotation_translation(
        size.extend(1.0),
        glam::Quat::IDENTITY,
        Vec3::new(x, 0.0, depth),
    )
}

/// A fading card 0.1 across, at `x` and `depth`.
fn fading(id: u8, x: f32, depth: f32) -> Draw {
    let size = Vec2::new(0.1 / CARD_WIDTH, 0.1 / CARD_HEIGHT);
    Draw::card(
        small(x, depth, size),
        [id, 0, 0, 128],
        FaceSize::Board,
        1,
        0,
        0.0,
        0.0,
    )
}

/// A badge 0.1 across, at `x` and `depth`, drawn with the quad mesh and the atlas.
fn badge(id: u8, x: f32, depth: f32) -> Draw {
    Draw::icon(
        small(x, depth, Vec2::splat(0.1)),
        tag(id),
        Sprite::default(),
    )
}

/// The order of `frame`'s records, by the ids in their tints.
fn order(frame: &Frame) -> Vec<u8> {
    frame.draws.iter().map(|draw| draw.tint[0]).collect()
}

#[test]
fn records_drawn_alike_that_share_no_pixel_are_drawn_in_one_call() {
    let mut frame = Frame::with_capacity(0, 8, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    builder.camera(Mat4::IDENTITY);
    // Fading cards and badges, apart on the screen, alternating from far to near.
    builder.draw(Band::TableMarks, fading(1, -0.8, 0.9));
    builder.draw(Band::TableMarks, badge(2, -0.4, 0.8));
    builder.draw(Band::TableMarks, fading(3, 0.0, 0.7));
    builder.draw(Band::TableMarks, badge(4, 0.4, 0.6));
    let frame = builder.finish();
    assert_eq!(order(frame), [1, 3, 2, 4]);
    assert_eq!(frame.load.draw_calls, 2);
}

#[test]
fn a_record_is_never_drawn_before_one_under_it() {
    let mut frame = Frame::with_capacity(0, 8, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    builder.camera(Mat4::IDENTITY);
    // The second card lies over the badge, so it waits for it. Nothing lies over the first.
    builder.draw(Band::TableMarks, fading(1, -0.5, 0.9));
    builder.draw(Band::TableMarks, badge(2, 0.5, 0.8));
    builder.draw(Band::TableMarks, fading(3, 0.52, 0.7));
    let frame = builder.finish();
    assert_eq!(order(frame), [1, 2, 3]);
    assert_eq!(frame.load.draw_calls, 3);
}

#[test]
fn light_that_only_adds_is_drawn_in_one_call_where_it_overlaps() {
    let mut frame = Frame::with_capacity(0, 8, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    builder.camera(Mat4::IDENTITY);
    // Two outlines with a burst between them, all over one another. Light adds, so the order
    // makes no difference and the outlines come together.
    let glow =
        |id, sprite, depth| Draw::glow(small(0.0, depth, Vec2::splat(0.5)), tag(id), sprite, 0.0);
    builder.draw(Band::Glows, glow(1, sprites::OUTLINE, 0.9));
    builder.draw(Band::Glows, glow(2, sprites::BURST, 0.8));
    builder.draw(Band::Glows, glow(3, sprites::OUTLINE, 0.7));
    let frame = builder.finish();
    assert_eq!(order(frame), [1, 3, 2]);
    assert_eq!(frame.load.draw_calls, 2);
}

#[test]
fn an_outline_counts_only_the_ring_its_mesh_covers() {
    let mut frame = Frame::with_capacity(0, 8, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    builder.camera(Mat4::IDENTITY);
    builder.draw(
        Band::Glows,
        Draw::glow(Affine3A::IDENTITY, tag(1), sprites::OUTLINE, 0.0),
    );
    let areas = builder.finish().areas();
    // A unit quad covers a quarter of the screen; the ring is the outer rectangle less the
    // hole, as shares of the quad.
    let [outer, hole] = [sprites::OUTLINE_OUTER, sprites::OUTLINE_HOLE];
    let ring = 4.0 * (outer[0] * outer[1] - hole[0] * hole[1]);
    assert!((areas.blended - 0.25 * ring).abs() < 1e-6, "{areas:?}");
}

#[test]
fn a_sixteenth_clip_sets_the_flag() {
    let mut frame = Frame::with_capacity(0, 0, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    for entry in 1..CLIPS {
        assert_eq!(builder.clip(Rect::default()), Some(entry as u8));
    }
    assert_eq!(builder.clip(Rect::default()), None);
    assert!(builder.finish().clips_full);
    // A new frame starts with the flag clear.
    assert!(!FrameBuilder::new(&mut frame).finish().clips_full);
}

/// Writes `cards` cards spread over the table with their edges and shadows, and a few
/// flat-layer records, in the order a scene would push them.
fn build(frame: &mut Frame, cards: u16) {
    let mut builder = FrameBuilder::new(frame);
    builder.camera(
        directx::perspective(0.8, 1.6, 10.0, 2000.0)
            * look_at_mat4(Vec3::new(0.0, -400.0, 800.0), Vec3::ZERO, Vec3::Y),
    );
    for index in 0..cards {
        let x = f32::from(index % 40) * 20.0 - 400.0;
        let y = f32::from(index / 40) * 30.0 - 300.0;
        let transform = Affine3A::from_translation(Vec3::new(x, y, f32::from(index) * 0.3));
        let card = Draw::card(transform, [255; 4], FaceSize::Board, index, 0, 0.0, 0.0);
        builder.draw(Band::Opaque, card);
        builder.draw(Band::TableMarks, card.edge());
        // Now and then a fading card among the edges, so that the band is batched.
        if index % 7 == 0 {
            let fading = Draw::card(
                transform,
                [255, 255, 255, 128],
                FaceSize::Board,
                index,
                0,
                0.0,
                0.0,
            );
            builder.draw(Band::TableMarks, fading);
        }
        if index % 10 == 0 {
            builder.draw(
                Band::ContactShadows,
                Draw::contact_shadow(transform, [0, 0, 0, 128], 2.0),
            );
        }
    }
    builder.draw(
        Band::Opaque,
        Draw::table(Affine3A::IDENTITY, [20, 60, 30, 255]),
    );
    for index in 0..40 {
        builder.draw(Band::Layer, quad(index, 0.0));
    }
    builder.finish();
}

#[test]
fn finishing_600_cards_allocates_nothing_after_the_first_frame() {
    let mut frame = Frame::with_capacity(OBJECTS, DRAWS, 0);
    build(&mut frame, 600);
    let first = frame.draws.clone();
    COUNTING.with(|counting| counting.set(true));
    for _ in 0..3 {
        build(&mut frame, 600);
    }
    COUNTING.with(|counting| counting.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(frame.draws, first, "the same records give the same order");
}
