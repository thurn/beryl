//! Horizontal alignment of battlefield creature rows with the table's middle line and life orbs.
//!
//! This uses a focused native integration test because none of the generic interface checks
//! (overlap, off-screen, reachability, duplicates, row overlap, or motion) expresses whether a
//! battlefield row's centre lines up horizontally with the player life orbs.

use app_client::Input;
use app_client::driver::Recording;
use beryl_protocol::protocol::{CardType, Side, ZoneName};
use beryl_protocol::testing::ViewBuilder;
use game_probe::story::{Body, Story};
use kit_frame::frame::{Frame, Role};
use kit_scene::hit::bounds;
use kit_ui::input::{Input as DeviceInput, Insets, Size};

use super::flat_layer::client;

const MS: u64 = 1_000_000;

fn orb_centre_x(frame: &Frame, name: &str) -> f32 {
    let element = frame
        .resolved
        .iter()
        .find(|element| element.role == Role::Region && frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("expected orb region {name:?}"));
    let rect = bounds(element);
    rect.x + rect.width / 2.0
}

fn row_centre_x(frame: &Frame, place: &str) -> f32 {
    let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut found = false;
    for element in &frame.resolved {
        if element.role == Role::Card && element.place.label() == place {
            let rect = bounds(element);
            left = left.min(rect.x);
            right = right.max(rect.x + rect.width);
            found = true;
        }
    }
    assert!(found, "expected at least one card in {place}");
    f32::midpoint(left, right)
}

#[test]
fn lone_creature_capture_centres_on_life_orb() {
    let catalog = app_client::stories::embedded();
    let story = catalog
        .iter()
        .find(|source| source.name == "story/game_table/lone-creature-centred")
        .expect("the lone creature capture story");
    let Body::Capture { capture, .. } = Story::parse(&story.text).unwrap().body else {
        panic!("the embedded story is a capture");
    };
    let view = Recording(capture.messages).views().unwrap().pop().unwrap();

    for (width, height) in [(1728.0, 1117.0), (393.0, 759.0)] {
        let mut client = client();
        client.handle(Input::Device(DeviceInput::Surface {
            size: Size { width, height },
            scale: 2.0,
            safe_area: Insets::default(),
        }));
        client.handle(Input::Update(Box::new(view.clone())));
        client.frame(0);
        let frame = client.frame(5_000 * MS);

        let creature_x = row_centre_x(frame, "battlefield.yours.creatures");
        let orb_x = orb_centre_x(frame, "your orb");
        assert!(
            (creature_x - orb_x).abs() <= 1.0,
            "{width}x{height}: lone creature centre {creature_x} differs from orb centre {orb_x}"
        );
    }
}

#[test]
fn creature_rows_stay_centred_for_both_seats_and_growing_rows() {
    for count in [1usize, 2, 5] {
        let mut builder = ViewBuilder::new();
        for side in [Side::You, Side::Opponent] {
            for index in 0..count {
                let creature =
                    builder.card(ZoneName::Battlefield, side, &format!("Dandân {index}"));
                creature.types = vec![CardType::Creature];
            }
            for _ in 0..4 {
                let land = builder.card(ZoneName::Battlefield, side, "Island");
                land.types = vec![CardType::Land];
            }
        }
        let update = builder.update();

        for (width, height) in [(1728.0, 1117.0), (393.0, 759.0)] {
            let mut client = client();
            client.handle(Input::Device(DeviceInput::Surface {
                size: Size { width, height },
                scale: 2.0,
                safe_area: Insets::default(),
            }));
            client.handle(Input::Update(Box::new(update.clone())));
            client.frame(0);
            let frame = client.frame(5_000 * MS);

            for (place, orb_name) in [
                ("battlefield.yours.creatures", "your orb"),
                ("battlefield.theirs.creatures", "opponent's orb"),
            ] {
                let row_x = row_centre_x(frame, place);
                let orb_x = orb_centre_x(frame, orb_name);
                assert!(
                    (row_x - orb_x).abs() <= 1.0,
                    "{width}x{height} count={count} {place}: row centre {row_x} differs from {orb_name} centre {orb_x}"
                );
            }
        }
    }
}
