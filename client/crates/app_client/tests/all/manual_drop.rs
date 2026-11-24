//! Dragging a permanent to a manual table region that has no cards to hit.

use beryl_protocol::protocol::{CardType, RowName, Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, table};
use game_manual::manual::drop::TARGET;
use game_table::table::DropZone;
use glam::Vec2;
use kit_frame::frame::{Role, picking::Area};
use kit_scene::hit;
use kit_ui::input::{Button, PointerPhase};
use serde_json::{Value, json};

use super::manual::{acts, client_at_table, middle, pointer};

fn drop_into_empty_zone(zone: DropZone) -> Value {
    let mut view = ViewBuilder::new();
    view.card(ZoneName::Battlefield, Side::You, "Serra Angel")
        .types = vec![CardType::Creature];
    view.script(ScriptBuilder::reset().build());
    let mut view = view.build();
    view.table = Some(table(&view));
    let (mut client, frame) = client_at_table(&view);
    let from = middle(&frame, Role::Card, "Serra Angel");
    // Find a bare point inside both the hit area and the outline of this region.
    let (to, bounds) = (1..16_u8)
        .flat_map(|x| (1..9_u8).map(move |y| Vec2::new(f32::from(x), f32::from(y)) * 100.0))
        .find_map(|at| {
            let region = frame.drop_region_at(at)?;
            (region.id == zone.id()
                && Area::Rect(region.bounds, None).contains(at)
                && hit::hit(&frame, at).is_none())
            .then_some((at, region.bounds))
        })
        .expect("a declared and uncovered drop region on the empty table");

    client.handle(pointer(PointerPhase::Down, from, Button::Primary));
    client.handle(pointer(PointerPhase::Move, to, Button::None));
    let frame = client.frame(1_000_000);
    let outline = frame
        .resolved
        .iter()
        .find(|element| frame.name(element.name) == TARGET)
        .expect("the hovered region is outlined");
    let drawn = hit::bounds(outline);
    assert!((drawn.x - bounds.x).abs() < 0.01);
    assert!((drawn.y - bounds.y).abs() < 0.01);
    assert!((drawn.width - bounds.width).abs() < 0.01);
    assert!((drawn.height - bounds.height).abs() < 0.01);

    client.handle(pointer(PointerPhase::Up, to, Button::Primary));
    let sent = acts(&mut client);
    assert_eq!(sent.len(), 1, "{sent:?}");
    serde_json::to_value(&sent[0]).unwrap()
}

#[test]
fn releasing_in_the_outlined_empty_hand_moves_the_card_to_hand() {
    let sent = drop_into_empty_zone(DropZone::Hand(Side::You));
    assert_eq!(sent["move"]["to"], json!({ "hand": 0 }));
}

#[test]
fn releasing_in_the_outlined_empty_land_row_pins_the_card_there() {
    let sent = drop_into_empty_zone(DropZone::Row(Side::You, RowName::Lands));
    assert_eq!(sent["pin"]["row"], "lands");
}
