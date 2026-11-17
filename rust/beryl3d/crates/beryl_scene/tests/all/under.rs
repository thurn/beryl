//! Cards that lie under a permanent in its row, as the web interface tucks them: a card the
//! permanent holds in exile peeks out to its left, and an Aura or Equipment attached to it, of
//! either player's, peeks out to its right. And the rows that are laid from their left end.

use beryl_protocol::protocol::{CardType, ObjectId, PlayerView, RowName, Side, ZoneName};
use beryl_scene::frame::Place;
use beryl_scene::input::{Insets, Size};
use beryl_scene::table::{Camera, DESKTOP, Placement, Scroll, Window, place};
use beryl_scene::testing::ViewBuilder;

fn placed(view: &PlayerView) -> Placement {
    let window = Window {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    };
    let camera = Camera::fit(DESKTOP.camera, window);
    place(view, &[], &DESKTOP, &camera, &Scroll::default())
}

fn creature(builder: &mut ViewBuilder, side: Side, name: &str) -> ObjectId {
    let card = builder.card(ZoneName::Battlefield, side, name);
    card.types = vec![CardType::Creature];
    card.slot
}

#[test]
fn held_and_attached_cards_lie_under_their_permanent_in_its_row() {
    let mut builder = ViewBuilder::sample();
    let host = creature(&mut builder, Side::You, "Fiend Hunter");
    let held = {
        let card = builder.card(ZoneName::Exile, Side::Opponent, "Geist of the Moors");
        card.exiled_with = Some(host);
        card.slot
    };
    // The opponent's Aura on the viewer's creature lies on the viewer's side, with the
    // creature.
    let aura = {
        let card = builder.card(ZoneName::Battlefield, Side::Opponent, "Pacifism");
        card.types = vec![CardType::Enchantment];
        card.attached_to = Some(host);
        card.slot
    };
    let view = builder.build();
    let placement = placed(&view);
    let rest = |slot| placement.get(slot).expect("every card has a place");
    let row = Place::Card {
        zone: ZoneName::Battlefield,
        side: Side::You,
        row: Some(RowName::Creatures),
    };
    assert_eq!(rest(host).place, row);
    assert_eq!(rest(aura).place, row);
    assert_eq!(
        rest(held).place,
        Place::Card {
            zone: ZoneName::Exile,
            side: Side::You,
            row: Some(RowName::Creatures),
        }
    );
    let x = |slot| rest(slot).pose.position.x;
    assert!(x(held) < x(host), "the held card peeks out to the left");
    assert!(x(aura) > x(host), "the Aura peeks out to the right");
    let z = |slot| rest(slot).pose.position.z;
    assert!(z(held) < z(host) && z(aura) < z(host), "both lie under it");
}

#[test]
fn a_card_whose_permanent_left_is_in_its_pile_again() {
    let mut builder = ViewBuilder::sample();
    let host = {
        let card = builder.card(ZoneName::Graveyard, Side::You, "Fiend Hunter");
        card.types = vec![CardType::Creature];
        card.slot
    };
    let held = {
        let card = builder.card(ZoneName::Exile, Side::Opponent, "Geist of the Moors");
        card.exiled_by = Some(host);
        card.slot
    };
    let view = builder.build();
    let placement = placed(&view);
    assert_eq!(
        placement.get(held).map(|rest| rest.place),
        Some(Place::Card {
            zone: ZoneName::Exile,
            side: Side::Opponent,
            row: None,
        })
    );
}

#[test]
fn a_lands_row_stands_from_its_left_end() {
    let lands = |count: usize| {
        let mut builder = ViewBuilder::new();
        let slots: Vec<ObjectId> = (0..count)
            .map(|_| {
                let card = builder.card(ZoneName::Battlefield, Side::You, "Wasteland");
                card.types = vec![CardType::Land];
                card.slot
            })
            .collect();
        let view = builder.build();
        let placement = placed(&view);
        slots
            .iter()
            .map(|&slot| placement.get(slot).expect("placed").pose.position.x)
            .collect::<Vec<f32>>()
    };
    // A land that leaves from the row's right end moves none of the others.
    let (two, one) = (lands(2), lands(1));
    assert!((two[0] - one[0]).abs() < 1e-3, "{two:?} {one:?}");
}

#[test]
fn an_artifact_with_a_mana_ability_stands_among_the_lands_after_them() {
    let mut builder = ViewBuilder::sample();
    let mut permanent = |name: &str, types: Vec<CardType>, mana_ability: bool| {
        let card = builder.card(ZoneName::Battlefield, Side::Opponent, name);
        card.types = types;
        card.mana_ability = mana_ability;
        card.slot
    };
    let rock = permanent("Mana Crypt", vec![CardType::Artifact], true);
    // A land stands before the rock, as on the web, though it entered after.
    let land = permanent("Island", vec![CardType::Land], true);
    let equipment = permanent("Leonin Scimitar", vec![CardType::Artifact], false);
    // A creature stands with the creatures, whatever it taps for.
    let dork = permanent(
        "Steel Overseer",
        vec![CardType::Artifact, CardType::Creature],
        true,
    );
    let view = builder.build();
    let placement = placed(&view);
    let row = |slot| match placement.get(slot).expect("every card has a place").place {
        Place::Card { row, .. } => row,
        place @ (Place::Layer | Place::Revealed) => {
            panic!("a permanent rests in a row, not {place:?}")
        }
    };
    assert_eq!(row(rock), Some(RowName::Lands));
    assert_eq!(row(land), Some(RowName::Lands));
    let x = |slot| placement.get(slot).expect("placed").pose.position.x;
    assert!(x(land) < x(rock), "the land stands left of the rock");
    assert_eq!(row(equipment), Some(RowName::Others));
    assert_eq!(row(dork), Some(RowName::Creatures));
}
