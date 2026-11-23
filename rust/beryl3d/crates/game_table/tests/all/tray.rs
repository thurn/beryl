//! A new prompt tray's movement is limited to the cards in that tray.

use beryl_protocol::protocol::{CardType, ObjectId, Side, ZoneName};
use beryl_protocol::testing::ViewBuilder;
use game_table::table::{Held, Table, TrayKind, Window};
use kit_base::pose::Curve;

fn put_back(table: &mut Table, top: Vec<ObjectId>, hand: Vec<ObjectId>, now: u64) {
    let held = Held {
        piles: vec![top, hand],
        rooms: vec![Some(2), None],
        kind: TrayKind::PutBack,
    };
    let poses = table.tray_poses(&held);
    table.lift(held, poses, (now, now + 200), Curve::EASE_OUT);
}

#[test]
fn a_new_tray_cannot_replay_the_movement_of_a_card_from_an_old_tray() {
    let mut hand = ViewBuilder::new();
    let land = hand.card(ZoneName::Hand, Side::You, "Island").slot;
    let first = hand.card(ZoneName::Hand, Side::You, "Counterspell").slot;
    let second = hand.card(ZoneName::Hand, Side::You, "Mana Drain").slot;
    let hand = hand.build();
    let mut table = Table::new(crate::test_data::data(), Window::default());
    table.apply(&hand);
    put_back(&mut table, vec![], vec![land, first, second], 0);
    let rests = table.zone_rests(&hand);
    table.let_go(&rests, (200, Some(1)), Curve::EASE_OUT);
    table.start_let_go(300);
    table.handed_over_tray(&[]);

    // The Island is played to the battlefield before the player opens another tray.
    // Its slot still belongs to the same card, but its earlier tray pose is obsolete.
    let mut battlefield = hand;
    let played = battlefield
        .objects
        .iter_mut()
        .find(|card| card.slot == land)
        .unwrap();
    played.zone = ZoneName::Battlefield;
    played.types = vec![CardType::Land];
    battlefield.battlefield.push(land);
    table.apply(&battlefield);
    let slot = usize::from(land.0);
    let rest = table.rest(slot).unwrap();
    assert_eq!(table.drawn_pose(slot, rest, 600), rest);

    put_back(&mut table, vec![], vec![first, second], 600);
    put_back(&mut table, vec![second], vec![first], 900);
    assert_eq!(table.drawn_pose(slot, rest, 900), rest);

    let rests = table.zone_rests(&battlefield);
    table.let_go(&rests, (200, Some(2)), Curve::EASE_OUT);
    assert!(table.letting_go(usize::from(first.0)).is_some());
    assert!(table.letting_go(slot).is_none());
    assert_eq!(table.drawn_pose(slot, rest, 1200), rest);
}
