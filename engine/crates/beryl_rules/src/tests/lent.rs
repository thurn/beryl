//! Abilities lent from other cards ([`Modification::Lend`], [`ObjectAbility::Lent`]):
//! Agatha's Soul Cauldron ("all activated abilities of all creature cards exiled with this
//! artifact") and Necrotic Ooze ("all activated abilities of all creature cards in all
//! graveyards").

use beryl_card::effect::build::{add_counters, draw, exile_linked, gain_life, target, this, you};
use beryl_card::filter::Filter;
use beryl_card::phrases::{creature_you_control, when_enters};
use beryl_card::target::{TargetKind, TargetSpec};
use beryl_card::{
    Ability, CardDef, Cost, CounterKind, Face, FaceIx, LayerSpec, Lends, Lent, LentFrom,
    Modification, ObjectAbility, Objects, Types, Zone,
};

use super::LAND;
use super::cast::Game;
use crate::decision::{Choice, Decision};
use crate::fixed::Full;
use crate::ids::{Slot, SlotId};
use crate::replacement::event::Sink;
use crate::state::Zone as StateZone;
use crate::zone::Place;
use crate::{CardTable, Decklist, Large, Layout, Opt, Profile, continuous::layers, reference};

/// "All activated abilities of all creature cards exiled with this artifact."
static EXILED_CREATURES: Lent = Lent {
    from: LentFrom::ExiledWithThis(Filter::Types(Types::CREATURE)),
    which: Lends::Activated,
};

/// "All activated abilities of all creature cards in all graveyards."
static GRAVEYARD_CREATURES: Lent = Lent {
    from: LentFrom::Graveyards(Filter::Types(Types::CREATURE)),
    which: Lends::Activated,
};

/// An artifact that exiles a card from a graveyard linked to itself and lends all activated
/// abilities of creature cards exiled with it to creatures you control.
static CAULDRON: CardDef = CardDef {
    name: "Test Cauldron",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::static_ability(&LayerSpec {
                affected: Objects::All {
                    zone: Zone::Battlefield,
                    filter: creature_you_control(),
                },
                condition: None,
                modifications: &[Modification::Lend(&EXILED_CREATURES)],
            }),
            Ability::activated(Cost::TAP)
                .target(&TargetSpec::one(TargetKind::GraveyardCard, Filter::Any))
                .effect(&exile_linked(target())),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature that has all activated abilities of all creature cards in all graveyards.
static OOZE: CardDef = CardDef {
    name: "Test Ooze",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(4),
        toughness: Some(3),
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: None,
            modifications: &[Modification::Lend(&GRAVEYARD_CREATURES)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature with one activated ability ("{T}: Put a +1/+1 counter on this creature"),
/// one static ability ("gets +2/+2"), and one triggered ability (when enters, draw a card).
static DONOR_A: CardDef = CardDef {
    name: "Test Donor A",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[
            Ability::activated(Cost::TAP).effect(&add_counters(this(), CounterKind::PlusOne, 1)),
            Ability::static_ability(&LayerSpec {
                affected: Objects::This,
                condition: None,
                modifications: &[Modification::ModifyPt(2, 2)],
            }),
            when_enters(&draw(you(), 1)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature with a zero-cost activated ability ("{0}: You gain 2 life.") that does not tap.
static DONOR_B: CardDef = CardDef {
    name: "Test Donor B",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::activated(Cost::NONE).effect(&gain_life(you(), 2))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A vanilla 1/1 creature.
static BORROWER: CardDef = CardDef {
    name: "Test Borrower",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Creatures lose all abilities."
static PLAIN: CardDef = CardDef {
    name: "Test Plain",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
            condition: None,
            modifications: &[Modification::LoseAllAbilities],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND, &CAULDRON, &OOZE, &DONOR_A, &DONOR_B, &BORROWER, &PLAIN,
]);

const DECK: &Decklist = &[
    (18, "Test Cauldron"),
    (2, "Test Ooze"),
    (2, "Test Donor A"),
    (2, "Test Donor B"),
    (4, "Test Borrower"),
    (2, "Test Plain"),
    (10, "Test Land"),
];

fn refresh(g: &mut Game) {
    layers::refresh(&mut g.s, &g.t, &mut Sink::off()).unwrap();
    reference::check_references(&g.s, &g.t).unwrap();
}

fn lent(g: &Game, slot: Slot) -> Vec<ObjectAbility> {
    g.s.abilities(&g.t, slot)
        .map(|(ability, _)| ability)
        .filter(|a| matches!(a, ObjectAbility::Lent(..)))
        .collect()
}

fn offered(d: &Decision, slot: Slot) -> Vec<ObjectAbility> {
    (0..d.len() as u16)
        .filter_map(|i| match d.candidates.option(Choice(i)) {
            Some(Opt::Activate { slot: s, ability }) if s == slot => Some(ability),
            _ => None,
        })
        .collect()
}

/// Exiles `card` from a graveyard using `cauldron`'s activated ability, then resolves it.
fn exile_with_cauldron(g: &mut Game, cauldron: Slot, card: Slot) {
    g.pick(Opt::Activate {
        slot: cauldron,
        ability: ObjectAbility::own(1),
    });
    if g.current().kind == crate::DecisionKind::Target {
        g.pick_object(card);
    }
    g.pass_both();
}

/// A creature in the affected set has each activated ability of each lending card and none of
/// their static or triggered abilities; a creature outside the affected set has none.
#[test]
fn affected_creature_gets_only_activated_abilities_and_unaffected_gets_none() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let cauldron = g.give(g.me, "Test Cauldron", StateZone::Battlefield);
    let mine = g.give(g.me, "Test Borrower", StateZone::Battlefield);
    let theirs = g.give(g.them, "Test Borrower", StateZone::Battlefield);
    let donor_a = g.give(g.me, "Test Donor A", StateZone::Graveyard);
    let land_in_gy = g.give(g.me, "Test Land", StateZone::Graveyard);
    refresh(&mut g);

    // Before exiling anything, no lent abilities exist.
    assert!(lent(&g, mine).is_empty());
    assert!(lent(&g, theirs).is_empty());

    // Exile Donor A with the cauldron.
    exile_with_cauldron(&mut g, cauldron, donor_a);
    refresh(&mut g);

    // Mine gets ONLY Donor A's activated ability (index 0), not its static (+2/+2) or trigger.
    assert_eq!(lent(&g, mine), [ObjectAbility::Lent(0, 0)]);
    assert_eq!(g.s.derived(mine).power, 1);
    assert_eq!(g.s.derived(mine).toughness, 1);
    // Opponent's creature is outside "creatures you control".
    assert!(lent(&g, theirs).is_empty());

    // Exile a noncreature card (Land) with a second cauldron: noncreatures are filtered out.
    let cauldron2 = g.give(g.me, "Test Cauldron", StateZone::Battlefield);
    refresh(&mut g);
    exile_with_cauldron(&mut g, cauldron2, land_in_gy);
    refresh(&mut g);
    assert_eq!(lent(&g, mine), [ObjectAbility::Lent(0, 0)]);
}

/// The borrower's {T} and "this creature" are the borrower's: activating a lent "{T}: Put a
/// +1/+1 counter on this creature" taps the borrower and puts the counter on the borrower.
#[test]
fn borrower_tap_and_this_creature_are_the_borrowers() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let cauldron = g.give(g.me, "Test Cauldron", StateZone::Battlefield);
    let borrower = g.give(g.me, "Test Borrower", StateZone::Battlefield);
    let donor_a = g.give(g.me, "Test Donor A", StateZone::Graveyard);
    refresh(&mut g);

    exile_with_cauldron(&mut g, cauldron, donor_a);
    refresh(&mut g);

    let d = g.decide();
    assert_eq!(offered(&d, borrower), [ObjectAbility::Lent(0, 0)]);

    g.pick(Opt::Activate {
        slot: borrower,
        ability: ObjectAbility::Lent(0, 0),
    });
    // Borrower is tapped as part of the activation cost.
    assert!(g.s.object(borrower).is_tapped());
    g.pass_both();

    // Counter was placed on the borrower, making it 2/2.
    assert_eq!(g.s.object(borrower).counters().plus_one, 1);
    assert_eq!(g.s.derived(borrower).power, 2);
    assert_eq!(g.s.derived(borrower).toughness, 2);
}

/// An activated lent ability on the stack resolves independently after its lending card leaves
/// exile and after the borrower loses all abilities (CR 113.7a).
#[test]
fn ability_on_stack_resolves_after_card_leaves_exile_and_borrower_loses_it() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let cauldron = g.give(g.me, "Test Cauldron", StateZone::Battlefield);
    let borrower = g.give(g.me, "Test Borrower", StateZone::Battlefield);
    let donor_b = g.give(g.me, "Test Donor B", StateZone::Graveyard);
    refresh(&mut g);

    exile_with_cauldron(&mut g, cauldron, donor_b);
    refresh(&mut g);

    let life_before = g.s.player(g.me).life();
    g.pick(Opt::Activate {
        slot: borrower,
        ability: ObjectAbility::Lent(0, 0),
    });
    assert_eq!(g.s.stack().len(), 1);

    // Move the lending card out of exile (to graveyard) and make borrower lose all abilities.
    g.s.move_object(&mut Sink::off(), donor_b, StateZone::Graveyard, Place::Top)
        .unwrap();
    assert!(g.s.is_derived_stale());
    g.give(g.me, "Test Plain", StateZone::Battlefield);
    refresh(&mut g);
    assert!(lent(&g, borrower).is_empty());

    // The ability on the stack still resolves and gains 2 life.
    g.pass_both();
    assert_eq!(g.s.player(g.me).life(), life_before + 2);
}

/// Two lending permanents lend their own exiled cards, tracked by each row's source.
#[test]
fn two_lending_permanents_lend_their_own_cards() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let cauldron1 = g.give(g.me, "Test Cauldron", StateZone::Battlefield);
    let borrower = g.give(g.me, "Test Borrower", StateZone::Battlefield);
    let donor_a = g.give(g.me, "Test Donor A", StateZone::Graveyard);
    let donor_b = g.give(g.me, "Test Donor B", StateZone::Graveyard);
    refresh(&mut g);

    exile_with_cauldron(&mut g, cauldron1, donor_a);
    let cauldron2 = g.give(g.me, "Test Cauldron", StateZone::Battlefield);
    refresh(&mut g);
    exile_with_cauldron(&mut g, cauldron2, donor_b);
    refresh(&mut g);

    assert_eq!(g.s.grant_rows().len(), 2);
    assert_eq!(
        lent(&g, borrower),
        [ObjectAbility::Lent(0, 0), ObjectAbility::Lent(1, 0)]
    );
    let (card0, ..) =
        g.s.lent_ability(&g.t, borrower, ObjectAbility::Lent(0, 0))
            .unwrap();
    let (card1, ..) =
        g.s.lent_ability(&g.t, borrower, ObjectAbility::Lent(1, 0))
            .unwrap();
    assert_eq!(g.t.def(card0).name, "Test Donor A");
    assert_eq!(g.t.def(card1).name, "Test Donor B");

    // Removing cauldron1 removes only Donor A's ability; cauldron2 still lends Donor B's.
    g.s.move_object(
        &mut Sink::off(),
        cauldron1,
        StateZone::Graveyard,
        Place::Top,
    )
    .unwrap();
    refresh(&mut g);
    assert_eq!(lent(&g, borrower), [ObjectAbility::Lent(0, 0)]);
    let (remaining, ..) =
        g.s.lent_ability(&g.t, borrower, ObjectAbility::Lent(0, 0))
            .unwrap();
    assert_eq!(g.t.def(remaining).name, "Test Donor B");
}

/// A permanent that lends from all graveyards (Necrotic Ooze) gets activated abilities of
/// creature cards in both graveyards and refreshes when graveyards change.
#[test]
fn graveyard_lender_lends_from_all_graveyards() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let ooze = g.give(g.me, "Test Ooze", StateZone::Battlefield);
    refresh(&mut g);
    assert!(lent(&g, ooze).is_empty());

    // Put Donor B into opponent's graveyard.
    let foe_donor = g.give(g.them, "Test Donor B", StateZone::Graveyard);
    assert!(g.s.is_derived_stale());
    refresh(&mut g);
    assert_eq!(lent(&g, ooze), [ObjectAbility::Lent(0, 0)]);

    let life_before = g.s.player(g.me).life();
    g.pick(Opt::Activate {
        slot: ooze,
        ability: ObjectAbility::Lent(0, 0),
    });
    g.pass_both();
    assert_eq!(g.s.player(g.me).life(), life_before + 2);

    // Removing Donor B from the graveyard removes the lent ability.
    g.s.move_object(&mut Sink::off(), foe_donor, StateZone::Exile, Place::Top)
        .unwrap();
    assert!(g.s.is_derived_stale());
    refresh(&mut g);
    assert!(lent(&g, ooze).is_empty());
}

/// 17 lent rows are `Full` in `Small`, and a widened `Small` state equals the `Large` one.
#[test]
fn seventeen_lent_rows_are_full_in_small_and_widened_equals_large() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let borrower = g.give(g.me, "Test Borrower", StateZone::Battlefield);
    for _ in 0..17 {
        g.give(g.me, "Test Cauldron", StateZone::Battlefield);
    }
    let mut large = g.s.widen();
    assert_eq!(layers::refresh(&mut g.s, &g.t, &mut Sink::off()), Err(Full));
    layers::refresh(&mut large, &g.t, &mut Sink::off()).unwrap();
    reference::check_references(&large, &g.t).unwrap();
    assert_eq!(large.grant_rows().len(), 17);
    let borrower_large: <Large as Layout>::Slot = SlotId::to(borrower).unwrap();
    assert!(large.has(borrower_large, ObjectAbility::Lent(16, 0)));

    // Also verify widening a Small state with active lent rows equals Large after refresh.
    let mut g2 = Game::with(&CARDS, DECK, Profile::EXACT);
    let c = g2.give(g2.me, "Test Cauldron", StateZone::Battlefield);
    let b = g2.give(g2.me, "Test Borrower", StateZone::Battlefield);
    let d = g2.give(g2.me, "Test Donor B", StateZone::Graveyard);
    refresh(&mut g2);
    exile_with_cauldron(&mut g2, c, d);
    refresh(&mut g2);

    let widened = g2.s.widen();
    let mut from_large = widened.clone();
    layers::refresh(&mut from_large, &g2.t, &mut Sink::off()).unwrap();
    assert_eq!(widened, from_large);
    let b_large: <Large as Layout>::Slot = SlotId::to(b).unwrap();
    assert_eq!(
        widened
            .lent_ability(&g2.t, b_large, ObjectAbility::Lent(0, 0))
            .map(|(card, ..)| g2.t.def(card).name),
        Some("Test Donor B")
    );
}

/// Rendering of `Modification::Lend` produces the expected Oracle phrasing.
#[test]
fn renders_lent_static_abilities() {
    let cauldron_text = beryl_card::render::render_face(&CAULDRON, FaceIx::FRONT);
    assert!(
        cauldron_text.contains(
            "have all activated abilities of all creature cards exiled with this artifact"
        ),
        "unexpected cauldron text: {cauldron_text}"
    );
    let ooze_text = beryl_card::render::render_face(&OOZE, FaceIx::FRONT);
    assert!(
        ooze_text.contains("has all activated abilities of all creature cards in all graveyards"),
        "unexpected ooze text: {ooze_text}"
    );
}
