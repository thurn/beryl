//! Gradients, masks and filters in the flat layer: a gradient is drawn by a `Gradient` record
//! whose stops are an entry of the frame's paint table, shared by every gradient with the same
//! stops; a mask puts the element and everything under it in a masked clip; a color filter
//! changes the colors of everything under it.

#![expect(
    clippy::float_cmp,
    reason = "a mask entry's flag is written as exactly 0 or 1"
)]

use game_table::table::Table;
use kit_base::tokens::gradient;
use kit_frame::frame::{Frame, FrameBuilder, Material, RADIAL};
use kit_ui::element::{Filter, Gradient, Overflow, Position, Stop, column, holder, row};
use kit_ui::input::Tier;
use kit_ui::layout::Layer;

const GOLD: [u8; 4] = [228, 162, 66, 255];

fn write(layer: &mut Layer) -> Frame {
    let mut frame = Frame::with_capacity(64, 256, 1024);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    layer.write(
        &mut builder,
        &Table::new(
            crate::test_data::data(),
            game_table::table::DESKTOP,
            game_table::table::Window::default(),
        )
        .camera()
        .screen_plane(),
        true,
    );
    builder.finish().clone()
}

fn placed(x: f32, width: f32, height: f32) -> kit_ui::element::Element {
    row()
        .position(Position::Absolute)
        .inset([Some(10.0), None, None, Some(x)])
        .width(width)
        .height(height)
}

#[test]
fn gradients_with_the_same_stops_share_an_entry_of_the_paint_table() {
    let mut layer = Layer::new();
    layer.show(Some(
        holder()
            .child(placed(10.0, 100.0, 40.0).background_image(vec![gradient::SECONDARY]))
            .child(placed(120.0, 140.0, 40.0).background_image(vec![gradient::SECONDARY]))
            .child(placed(270.0, 100.0, 40.0).background_image(vec![gradient::PRIMARY])),
    ));
    let frame = write(&mut layer);
    let entries: Vec<u16> = (frame.draws.iter())
        .filter(|draw| draw.material == Material::Gradient as u8)
        .map(|draw| draw.layer & (RADIAL - 1))
        .collect();
    // A linear gradient's stops are fractions of its line, the same on boxes of any width.
    assert_eq!(entries, [0, 0, 1]);
    assert_eq!(frame.paints.gradients, 2);
    let first = frame.paints.table.gradients[1].colors[0].to_le_bytes();
    assert_eq!(first, gradient::PRIMARY.stops()[0].color);
    // The uploaded part of the table is the masks, then the two gradients.
    assert_eq!(frame.paints.bytes().len(), 16 * 32 + 2 * 64);
}

#[test]
fn a_mask_clips_the_element_and_everything_under_it_and_names_its_gradient() {
    let mask = Gradient::linear(
        90.0,
        &[Stop::new([0; 4]), Stop::percent([0, 0, 0, 255], 30.0)],
    );
    let mut layer = Layer::new();
    layer.show(Some(
        holder().child(
            placed(20.0, 200.0, 100.0)
                .background(GOLD)
                .mask(mask)
                .child(row().width(50.0).height(50.0).background([10, 20, 30, 255])),
        ),
    ));
    let frame = write(&mut layer);
    let panels: Vec<u8> = (frame.draws.iter())
        .filter(|draw| draw.material == Material::Panel as u8)
        .map(kit_frame::frame::Draw::clip)
        .collect();
    assert_eq!(panels.len(), 2);
    assert!(panels.iter().all(|&clip| clip == panels[0] && clip > 0));
    assert!(frame.paints.masked);
    let entry = frame.paints.table.masks[usize::from(panels[0])];
    assert_eq!(entry.line[2], 1.0, "the clip is masked");
    assert!((entry.line[0] - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    let held = frame.paints.table.gradients[entry.line[1] as usize];
    assert!((held.offsets[1] - 0.3).abs() < 1e-6, "{:?}", held.offsets);
}

#[test]
fn a_scroll_fade_masks_each_end_with_more_beyond_it() {
    let rows = (0..8).map(|at| {
        row()
            .name(&format!("row {at}"))
            .width(100.0)
            .height(50.0)
            .shrink(0.0)
            .background(GOLD)
    });
    let mut list = column()
        .name("list")
        .max_height(120.0)
        .overflow(Overflow::Scroll)
        .scroll_fade(20.0)
        .children(rows);
    list.style.position = Position::Absolute;
    let mut layer = Layer::new();
    layer.show(Some(holder().child(list)));
    let angle = |frame: &Frame| {
        let clip = (frame.draws.iter())
            .find(|draw| draw.material == Material::Panel as u8)
            .map(kit_frame::frame::Draw::clip)
            .expect("a row is drawn");
        let entry = frame.paints.table.masks[usize::from(clip)];
        (entry.line[2] == 1.0).then_some(entry.line[0])
    };
    // At the top only the bottom fades, toward the top of the gradient line: `to top`.
    let frame = write(&mut layer);
    assert_eq!(angle(&frame), Some(0.0));
    layer.scroll_to_end("list");
    let frame = write(&mut layer);
    assert!((angle(&frame).expect("masked") - std::f32::consts::PI).abs() < 1e-6);
}

#[test]
fn a_filter_changes_every_color_drawn_under_it() {
    let mut layer = Layer::new();
    layer.show(Some(
        holder().child(
            placed(20.0, 200.0, 100.0)
                .filter(vec![Filter::Brightness(0.5)])
                .child(
                    row()
                        .width(50.0)
                        .height(50.0)
                        .background([200, 100, 50, 255]),
                ),
        ),
    ));
    let frame = write(&mut layer);
    let tints: Vec<[u8; 4]> = (frame.draws.iter())
        .filter(|draw| draw.material == Material::Panel as u8)
        .map(|draw| draw.tint)
        .collect();
    assert_eq!(tints, [[100, 50, 25, 255]]);
}
