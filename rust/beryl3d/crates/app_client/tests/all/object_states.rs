//! The object states the session sends that the table shows: a merged permanent's other cards,
//! a phased-out permanent, a copy, a card picked in a hidden pick, and a decision made for the
//! controlled opponent. One board holds all five, seen from the viewer's seat, and the interface
//! checks run on it.

use app_client::Input;
use beryl_protocol::protocol::{CardType, ObjectId, Side, Status, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_table::table::Place;
use game_table::table::labels::{COPY_NOTE, PHASED_OUT, PICKED};
use kit_frame::frame::{Frame, Resolved, Role, StateBits};
use kit_probe::agent::Context;
use kit_probe::agent::checks::{duplicates, on_screen, reachable};
use kit_probe::agent::tree::{TreeOptions, tree};
use kit_ui::input::{Insets, Size};

use super::flat_layer::{client, deciding, prompt};

/// The slots of the board's cards that each test looks at.
struct Slots {
    host: ObjectId,
    under: ObjectId,
    phased: ObjectId,
    copy: ObjectId,
    mine_picked: ObjectId,
    theirs_picked: ObjectId,
}

/// The sample table with a merged creature of the viewer's and the card under it, a phased-out
/// creature of the opponent's, a copy of the viewer's, and a card each player picked from their
/// hand, while the viewer pays a cost for the opponent, whose turn they control.
fn board() -> (Input, Slots) {
    let mut builder = ViewBuilder::sample();
    let creature = |builder: &mut ViewBuilder, side, name: &str| {
        let card = builder.card(ZoneName::Battlefield, side, name);
        card.types = vec![CardType::Creature];
        card.slot
    };
    let host = creature(&mut builder, Side::You, "Gemrazer");
    let under = {
        let card = builder.card(ZoneName::Merged, Side::You, "Blood Artist");
        card.part_of = Some(host);
        card.slot
    };
    let phased = creature(&mut builder, Side::Opponent, "Wetland Sambar");
    let copy = creature(&mut builder, Side::You, "Mother of Runes");
    let mine_picked = builder.card(ZoneName::Hand, Side::You, "Griselbrand").slot;
    let theirs_picked = builder.card(ZoneName::Hand, Side::Opponent, "Emrakul").slot;
    let mut view = builder.build();
    for object in &mut view.objects {
        if object.slot == phased {
            object.phased_out = true;
        } else if object.slot == copy {
            object.really = Some("Phantasmal Image".to_owned());
        } else if object.slot == mine_picked || object.slot == theirs_picked {
            object.hidden_pick = true;
        }
    }
    let Input::Update(mut update) = deciding(prompt("Pay {1}{U}")) else {
        unreachable!("`deciding` makes an update");
    };
    let Status::Deciding { mut decision } = update.view.status else {
        unreachable!("`deciding` makes a decision");
    };
    decision.acting_for = Some(Side::Opponent);
    view.status = Status::Deciding { decision };
    view.script = Some(ScriptBuilder::reset().build());
    update.view = view;
    let slots = Slots {
        host,
        under,
        phased,
        copy,
        mine_picked,
        theirs_picked,
    };
    (Input::Update(update), slots)
}

fn frame() -> (Frame, Slots) {
    let mut client = client();
    let (update, slots) = board();
    client.handle(update);
    (client.frame(0).clone(), slots)
}

fn card(frame: &Frame, slot: ObjectId) -> &Resolved {
    frame
        .resolved
        .iter()
        .find(|r| r.role == Role::Card && r.id.0 == u32::from(slot.0))
        .unwrap_or_else(|| panic!("slot {} is drawn", slot.0))
}

/// The middle of an element's box on screen.
fn middle(element: &Resolved) -> glam::Vec2 {
    element.corners.iter().copied().sum::<glam::Vec2>() / 4.0
}

#[test]
fn the_tree_names_each_state() {
    let (frame, _) = frame();
    let printed = tree(&frame, &TreeOptions::default()).unwrap();
    for expected in [
        "card \"Blood Artist\" #",
        "merged.yours.creatures",
        "phased-out",
        "faded(60%)",
        "copy(\"Phantasmal Image\")",
        "picked",
        "text \"For opponent: Pay {1}{U}\"",
    ] {
        assert!(printed.contains(expected), "{expected} in\n{printed}");
    }
}

#[test]
fn the_card_under_a_merged_permanent_peeks_out_to_its_left_and_away() {
    let (frame, slots) = frame();
    let (host, under) = (card(&frame, slots.host), card(&frame, slots.under));
    assert_eq!(
        under.place,
        Place::Card {
            zone: ZoneName::Merged,
            side: Side::You,
            row: Some(beryl_protocol::protocol::RowName::Creatures),
        }
        .into()
    );
    let (host_at, under_at) = (middle(host), middle(under));
    assert!(under_at.x < host_at.x, "{under_at} left of {host_at}");
    assert!(under_at.y < host_at.y, "{under_at} above {host_at}");
    assert!(under.depth > host.depth, "the permanent lies over its card");
    assert!(!under.interactive);
}

#[test]
fn a_phased_out_permanent_is_faded_and_a_copy_names_its_card() {
    let (frame, slots) = frame();
    let phased = card(&frame, slots.phased);
    assert!(phased.state.has(PHASED_OUT));
    assert!((phased.opacity - 0.4).abs() < 1e-6, "{}", phased.opacity);
    let copy = card(&frame, slots.copy);
    assert_eq!(frame.name(copy.name), "Mother of Runes");
    assert_eq!(frame.name(copy.annotations[COPY_NOTE]), "Phantasmal Image");
}

#[test]
fn a_picked_card_lies_apart_from_its_hand_face_up_only_for_its_player() {
    let (frame, slots) = frame();
    let mine = card(&frame, slots.mine_picked);
    let theirs = card(&frame, slots.theirs_picked);
    assert!(mine.state.has(PICKED) && theirs.state.has(PICKED));
    assert!(!mine.state.has(StateBits::FACE_DOWN));
    assert!(theirs.state.has(StateBits::FACE_DOWN));
    // Each lies on its player's side of the table. The opponent's pick is off the
    // centre line of the battlefield, where it could cover a creature.
    let (mine_at, theirs_at) = (middle(mine), middle(theirs));
    assert!(theirs_at.y < mine_at.y, "{theirs_at} above {mine_at}");
    assert!(theirs_at.x < mine_at.x, "{theirs_at} left of {mine_at}");
}

#[test]
fn the_board_passes_the_interface_checks() {
    let (frame, _) = frame();
    let context = Context {
        layout: crate::REGULAR,
        audit: crate::test_data::data().audit,
        checks: crate::REGULAR.checks(
            &crate::test_data::data().profiles,
            kit_base::geometry::Size::default(),
        ),
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        safe_area: Insets::default(),
        at: 0,
    };
    let mut faults = on_screen(&frame, &context);
    faults.extend(reachable(&frame, &context));
    faults.extend(duplicates(&frame, &context));
    assert!(faults.is_empty(), "{faults:#?}");
}
