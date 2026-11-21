//! A looking frame knows which element each of its records belongs to (`Frame::owners`), in
//! drawing order, and `Frame::keep_only` draws a subtree alone: what `just b3d png --only` and
//! `just b3d match` use to separate one element's pixels from what lies behind it.

use glam::{Affine3A, Mat4, Vec2, Vec3};
use kit_base::atlas::Tier;
use kit_frame::frame::{
    Band, Draw, ElementId, Frame, FrameBuilder, NameRef, Place, Resolved, Role, Sprite, StateBits,
};

/// A flat-layer element whose parent is `parent`, at no particular place.
fn element(id: u32, parent: u32) -> Resolved {
    Resolved {
        id: ElementId(id),
        parent,
        role: Role::Region,
        name: NameRef::default(),
        place: Place::Layer,
        pose: None,
        corners: [Vec2::ZERO; 4],
        depth: 0.0,
        opacity: 1.0,
        clip: None,
        interactive: false,
        state: StateBits::default(),
        face: None,
        copies: 1,
        really: NameRef::default(),
        note: NameRef::default(),
    }
}

/// A record whose red channel names it, so a test can read back which records remain.
fn quad(tag: u8) -> Draw {
    Draw::glyph(Affine3A::IDENTITY, [tag, 0, 0, 255], Sprite::default())
}

fn tags(frame: &Frame) -> Vec<u8> {
    frame.draws.iter().map(|draw| draw.tint[0]).collect()
}

/// A frame of a panel (element 0) holding a label (element 1), a sibling panel (element 2) and
/// the table's cloth, which belongs to no element. Records are pushed out of drawing order: the
/// sibling's record goes to an earlier band than the panel's.
fn built(frame: &mut Frame) {
    frame.looking = true;
    let mut builder = FrameBuilder::new(frame, Tier::Baseline);
    builder.camera(Mat4::IDENTITY);
    builder.draw(
        Band::Opaque,
        Draw::table(Affine3A::from_translation(Vec3::Z * 0.9), [9, 0, 0, 255]),
    );
    let panel = builder.resolved("panel", element(1, u32::MAX));
    builder.draw(Band::LayerCover, quad(10));
    let _label = builder.resolved("label", element(2, panel));
    builder.draw(Band::LayerCover, quad(11));
    builder.draw(Band::LayerCover, quad(12));
    let _sibling = builder.resolved("sibling", element(3, u32::MAX));
    builder.draw(Band::Layer, quad(20));
    builder.finish();
}

#[test]
fn each_record_names_its_element_in_drawing_order() {
    let mut frame = Frame::with_capacity(8, 8, 64);
    built(&mut frame);
    assert_eq!(tags(&frame), vec![9, 20, 10, 11, 12]);
    assert_eq!(frame.owners, vec![u32::MAX, 2, 0, 1, 1]);
}

#[test]
fn a_frame_not_looking_records_no_owners() {
    let mut frame = Frame::with_capacity(8, 8, 64);
    built(&mut frame);
    frame.looking = false;
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    builder.resolved("panel", element(1, u32::MAX));
    builder.draw(Band::Layer, quad(10));
    builder.finish();
    assert!(frame.owners.is_empty());
}

#[test]
fn keeping_a_subtree_drops_everything_else_and_its_bands_follow() {
    let mut frame = Frame::with_capacity(8, 8, 64);
    built(&mut frame);
    frame.keep_only(|owner| owner <= 1);
    assert_eq!(tags(&frame), vec![10, 11, 12], "the panel and its label");
    assert_eq!(frame.owners, vec![0, 1, 1]);
    assert_eq!(frame.opaque, 0, "the cloth belongs to no element");
    assert_eq!(frame.band(0), Some(Band::LayerCover));
    let cover = Band::LayerCover as usize;
    assert_eq!(frame.bands[cover]..frame.bands[cover + 1], 0..3);
    assert_eq!(frame.layer, 0);
}
