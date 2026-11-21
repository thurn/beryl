//! The space battlefield rows keep clear of the stack and life totals.

use beryl_protocol::protocol::{RowName, Side};
use game_table::table::{Camera, Layout, Window, row_area};
use kit_ui::input::Size;

use crate::reference;

const ROWS: [RowName; 3] = [RowName::Creatures, RowName::Lands, RowName::Others];

fn camera(layout: &Layout, width: f32, height: f32) -> Camera {
    let window = Window {
        size: Size { width, height },
        ..Window::default()
    };
    Camera::fit(layout.camera, window)
}

/// On the phone sizes the stack's reach lies left of every row's start. A cut there once
/// ended the creatures' row left of its left limit, so its cards collapsed into one pile.
#[test]
fn narrow_rows_keep_room_for_a_tapped_card() {
    let reference = reference();
    for (width, height) in [(393.0, 759.0), (375.0, 647.0), (430.0, 932.0)] {
        let camera = camera(&reference, width, height);
        for side in [Side::You, Side::Opponent] {
            for row in ROWS {
                let (area, _) = row_area((&reference, &camera), side, row);
                assert!(
                    area.right - area.left >= reference.camera.card.height,
                    "{width}x{height} {side:?} {row:?}: {} to {}",
                    area.left,
                    area.right,
                );
            }
        }
    }
}

/// On the desktop the stack stands beside the creatures' row, which still ends left of it.
#[test]
fn creatures_end_left_of_the_stack_column() {
    let reference = reference();
    for (width, height) in [(1440.0, 900.0), (1600.0, 900.0)] {
        let camera = camera(&reference, width, height);
        for side in [Side::You, Side::Opponent] {
            let (area, _) = row_area((&reference, &camera), side, RowName::Creatures);
            assert!(
                area.right < reference.creatures.right,
                "{width}x{height} {side:?}"
            );
            assert!(
                area.right - area.left >= reference.camera.card.height,
                "{width}x{height} {side:?}"
            );
        }
    }
}

/// The viewer's lands once ran under their life orb and its step stops on a 1600 by 900
/// window, with a planeswalker on the other side of the orb's middle. On every desktop size
/// each back row's part now ends, on screen, clear of the stops either side of the orb.
#[test]
fn back_rows_clear_the_life_orb() {
    use glam::Vec3;

    use kit_base::tokens::size;

    let reference = reference();

    for (width, height) in [
        (1280.0, 800.0),
        (1440.0, 900.0),
        (1600.0, 900.0),
        (1920.0, 1080.0),
    ] {
        let camera = camera(&reference, width, height);
        let life_x = f32::midpoint(reference.players[0].life.x, 1.0) * width;
        let half = (size::ORB_WIDTH * reference.play.life.orb_scale).max(size::STEPS_WIDTH) / 2.0;
        let screen = |row, x: f32| {
            let (_, y) = row_area((&reference, &camera), Side::You, row);
            [
                -reference.camera.card.height / 2.0,
                reference.camera.card.height / 2.0,
            ]
            .map(|along| camera.to_screen(Vec3::new(x, y + along, 0.0)).x)
        };
        let (lands, _) = row_area((&reference, &camera), Side::You, RowName::Lands);
        let (others, _) = row_area((&reference, &camera), Side::You, RowName::Others);
        for x in screen(RowName::Lands, lands.right) {
            assert!(
                x <= life_x - half + 0.01,
                "{width}x{height}: lands end at {x}"
            );
        }
        for x in screen(RowName::Others, others.left) {
            assert!(
                x >= life_x + half - 0.01,
                "{width}x{height}: others start at {x}"
            );
        }
    }
}
