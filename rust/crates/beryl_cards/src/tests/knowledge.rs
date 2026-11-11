//! What a player knows of hidden cards, as the AI's determinizations see it (architecture §13):
//! a card a player saw stays known wherever it goes until it is shuffled or otherwise hidden
//! anew, and every world dealt from that player's view keeps it in its slot; a card they never
//! saw is dealt at random from the rest of its decklist.
//!
//! A hand and a library are hidden zones
//! ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)): "Library and hand are
//! hidden zones, even if all the cards in one such zone happen to be revealed."

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    CardIx, DecisionKind, GameTable, Observation, Seat, Slot, State, TurnStep, Zone, ZoneKey,
};
use beryl_scenario::{Action, P0, P1, Permanent, Pick, Scenario};

use crate::{CARDS, names};

/// Worlds dealt per check: enough that a hidden card lands differently in some of them.
const WORLDS: u64 = 32;

/// The table, and the state as each of the scenario's actions begins: where each test reads
/// what is known. The scenarios here have no assertions, so the states line up with the
/// actions.
fn at_each_action(scenario: &Scenario) -> (GameTable, Vec<State>) {
    let (t, _) = beryl_scenario::position(&CARDS, scenario).unwrap();
    let mut states = Vec::new();
    beryl_scenario::visit(&CARDS, scenario, |_, s, _| states.push(*s)).unwrap();
    assert_eq!(states.len(), scenario.actions.len());
    (t, states)
}

/// The table, and the state as the scenario's last action begins.
fn at_last_action(scenario: &Scenario) -> (GameTable, State) {
    let (t, mut states) = at_each_action(scenario);
    (t, states.pop().expect("the scenario has actions"))
}

/// The slots `owner` has in `zone`, top of a library first.
fn slots(s: &State, owner: Seat, zone: Zone) -> Vec<Slot> {
    if zone == Zone::Library {
        s.library(owner).iter().rev().copied().collect()
    } else {
        s.zone(ZoneKey::Of(owner, zone)).iter().collect()
    }
}

/// The slot holding the card `name` among `owner`'s cards in `zone`.
fn slot_of(t: &GameTable, s: &State, owner: Seat, zone: Zone, name: &str) -> Slot {
    slots(s, owner, zone)
        .into_iter()
        .find(|&slot| s.definition_name(t, slot) == name)
        .unwrap_or_else(|| panic!("no {name} in {owner:?}'s {zone:?}"))
}

/// The worlds the search would deal from `viewer`'s view of `s`.
fn worlds(t: &GameTable, s: &State, viewer: Seat) -> Vec<State> {
    let obs = Observation::new(s, viewer);
    (0..WORLDS)
        .map(|seed| obs.determinize(t, &mut SplitMix64::new(seed)))
        .collect()
}

/// `viewer` knows the card in `slot`, and every world dealt from their view keeps it there.
fn kept(t: &GameTable, s: &State, viewer: Seat, slot: Slot) {
    let name = s.definition_name(t, slot);
    assert!(s.knows(viewer, slot), "{viewer:?} doesn't know {name}");
    for (seed, world) in worlds(t, s, viewer).iter().enumerate() {
        assert_eq!(
            world.card_behind(slot),
            s.card_behind(slot),
            "world {seed} for {viewer:?} lost {name}"
        );
    }
}

/// `viewer` knows none of `hidden`, and the worlds dealt from their view do not all agree on
/// what those slots hold.
fn varies(t: &GameTable, s: &State, viewer: Seat, hidden: &[Slot]) {
    for &slot in hidden {
        assert!(
            !s.knows(viewer, slot),
            "{viewer:?} knows the {} in slot {slot}",
            s.definition_name(t, slot)
        );
    }
    let deal = |w: &State| -> Vec<Option<CardIx>> {
        hidden.iter().map(|&slot| w.card_behind(slot)).collect()
    };
    let worlds = worlds(t, s, viewer);
    assert!(
        worlds.iter().any(|w| deal(w) != deal(&worlds[0])),
        "every world for {viewer:?} deals the same cards to {hidden:?}"
    );
}

/// A permanent returned to its owner's hand was public on the battlefield, so its new place is
/// known: Teferi, Time Raveler's −3 shows its controller the Mana Crypt in the opponent's hand,
/// but not the cards beside it.
#[test]
fn a_bounced_permanent_stays_known_in_hand() {
    const BOUNCE: Scenario = Scenario::new("Teferi returns Mana Crypt")
        .battlefield(P0, &[Permanent::new(names::TEFERI_TIME_RAVELER)])
        .battlefield(P1, &[Permanent::new(names::MANA_CRYPT)])
        .hand(P1, &[names::COUNTERSPELL, names::MANA_TITHE, names::OUST])
        .library(P0, &[names::ISLAND, names::PLAINS])
        .library(
            P1,
            &[
                names::PLAINS,
                names::SNAPCASTER_MAGE,
                names::MANA_DRAIN,
                names::ISLAND,
            ],
        )
        .actions(&[
            Action::activate(P0, names::TEFERI_TIME_RAVELER, 2),
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            Action::pass(P0),
        ]);
    let (t, s) = at_last_action(&BOUNCE);
    let crypt = slot_of(&t, &s, Seat::P1, Zone::Hand, names::MANA_CRYPT);
    kept(&t, &s, Seat::P0, crypt);
    let others: Vec<Slot> = slots(&s, Seat::P1, Zone::Hand)
        .into_iter()
        .filter(|&slot| slot != crypt)
        .collect();
    assert_eq!(others.len(), 3);
    varies(&t, &s, Seat::P0, &others);
    varies(&t, &s, Seat::P0, &slots(&s, Seat::P1, Zone::Library));
}

/// A spell returned from the stack to its owner's hand was revealed as it was cast
/// ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a)): Reprieve's caster knows
/// the Thieving Skydiver in the opponent's hand, not the rest of that hand.
#[test]
fn a_spell_returned_to_hand_stays_known() {
    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 4];
    const LANDS: &[Permanent] = &[Permanent::new(names::PLAINS), Permanent::new(names::ISLAND)];
    const RETURN: Scenario = Scenario::new("Reprieve returns Thieving Skydiver")
        .battlefield(P0, ISLANDS)
        .hand(
            P0,
            &[
                names::THIEVING_SKYDIVER,
                names::PLAINS,
                names::OUST,
                names::MANA_TITHE,
            ],
        )
        .library(
            P0,
            &[names::ISLAND, names::MANA_DRAIN, names::SNAPCASTER_MAGE],
        )
        .battlefield(P1, LANDS)
        .hand(P1, &[names::REPRIEVE])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::cast(P0, names::THIEVING_SKYDIVER),
            Action::cast(P1, names::REPRIEVE),
            Action::pass(P0),
        ]);
    let (t, s) = at_last_action(&RETURN);
    let skydiver = slot_of(&t, &s, Seat::P0, Zone::Hand, names::THIEVING_SKYDIVER);
    kept(&t, &s, Seat::P1, skydiver);
    let others: Vec<Slot> = slots(&s, Seat::P0, Zone::Hand)
        .into_iter()
        .filter(|&slot| slot != skydiver)
        .collect();
    assert_eq!(others.len(), 3);
    varies(&t, &s, Seat::P1, &others);
}

/// A card a tutor reveals ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a):
/// "To reveal a card, show that card to all players for a brief time") is known to the
/// opponent in its owner's hand from then on. The shuffle after the search leaves the library
/// unknown to both players, the searcher included
/// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)).
#[test]
fn a_tutored_card_stays_known_in_hand() {
    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];
    const TUTOR: Scenario = Scenario::new("Trinket Mage reveals Mana Crypt")
        .hand(P0, &[names::TRINKET_MAGE])
        .battlefield(P0, ISLANDS)
        .library(
            P0,
            &[
                names::PLAINS,
                names::MANA_CRYPT,
                names::OUST,
                names::COUNTERSPELL,
                names::SNAPCASTER_MAGE,
            ],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::cast(P0, names::TRINKET_MAGE),
            Action::Resolve,
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::choose(P0, DecisionKind::ChooseUpTo, names::MANA_CRYPT),
            Action::pass(P0),
        ]);
    let (t, s) = at_last_action(&TUTOR);
    let crypt = slot_of(&t, &s, Seat::P0, Zone::Hand, names::MANA_CRYPT);
    kept(&t, &s, Seat::P1, crypt);
    let library = slots(&s, Seat::P0, Zone::Library);
    varies(&t, &s, Seat::P1, &library);
    varies(&t, &s, Seat::P0, &library);
}

/// A permanent put into a library at a known depth was public as it went, so both players know
/// it there: Oust's creature, second from the top, while the cards around it stay hidden.
#[test]
fn a_permanent_put_into_a_library_stays_known_there() {
    const OUST: Scenario = Scenario::new("Oust tucks Elite Spellbinder")
        .battlefield(P0, &[Permanent::new(names::PLAINS)])
        .hand(P0, &[names::OUST, names::PLAINS])
        .battlefield(P1, &[Permanent::new(names::ELITE_SPELLBINDER)])
        .library(
            P1,
            &[
                names::ISLAND,
                names::COUNTERSPELL,
                names::MANA_TITHE,
                names::SNAPCASTER_MAGE,
                names::PLAINS,
            ],
        )
        .actions(&[Action::cast(P0, names::OUST), Action::pass(P0)]);
    let (t, s) = at_last_action(&OUST);
    let library = slots(&s, Seat::P1, Zone::Library);
    let spellbinder = library[1];
    assert_eq!(s.definition_name(&t, spellbinder), names::ELITE_SPELLBINDER);
    for viewer in Seat::BOTH {
        kept(&t, &s, viewer, spellbinder);
        let rest: Vec<Slot> = library
            .iter()
            .copied()
            .filter(|&slot| slot != spellbinder)
            .collect();
        varies(&t, &s, viewer, &rest);
    }
}

/// Brainstorm's put-backs (Jace, the Mind Sculptor's 0): their owner knows the top of their
/// library in the order they chose, and the opponent, who never saw those cards, does not.
#[test]
fn brainstorm_put_backs_are_known_to_their_owner_alone() {
    const BRAINSTORM: Scenario = Scenario::new("Jace brainstorms")
        .battlefield(P0, &[Permanent::new(names::JACE_THE_MIND_SCULPTOR)])
        .hand(P0, &[names::STAFF_OF_THE_STORYTELLER])
        .library(
            P0,
            &[
                names::ISLAND,
                names::PLAINS,
                names::JACE_VRYNS_PRODIGY,
                names::MANA_CRYPT,
                names::COUNTERSPELL,
                names::OUST,
            ],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::activate(P0, names::JACE_THE_MIND_SCULPTOR, 1),
            Action::Resolve,
            Action::choose(
                P0,
                DecisionKind::ChooseObject,
                names::STAFF_OF_THE_STORYTELLER,
            ),
            Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
            Action::pass(P0),
        ]);
    let (t, s) = at_last_action(&BRAINSTORM);
    let library = slots(&s, Seat::P0, Zone::Library);
    let names_on_top: Vec<&str> = library[..2]
        .iter()
        .map(|&slot| s.definition_name(&t, slot))
        .collect();
    assert_eq!(
        names_on_top,
        [names::ISLAND, names::STAFF_OF_THE_STORYTELLER]
    );
    for &slot in &library[..2] {
        kept(&t, &s, Seat::P0, slot);
    }
    varies(&t, &s, Seat::P0, &library[2..]);
    varies(&t, &s, Seat::P1, &library[..2]);
}

/// A look at the top of an opponent's library (Jace, the Mind Sculptor's +2, keeping the card
/// there) is known to the one who looked, not to the library's owner; once its owner draws it,
/// the one who looked still knows it, now in their hand.
#[test]
fn a_fatesealed_card_stays_known_once_drawn() {
    const FATESEAL: Scenario = Scenario::new("Jace fateseals and keeps the top")
        .battlefield(P0, &[Permanent::new(names::JACE_THE_MIND_SCULPTOR)])
        // A land to play keeps player 0 in their main phase after Jace's ability.
        .hand(P0, &[names::PLAINS])
        .library(P0, &[names::ISLAND, names::PLAINS])
        .library(
            P1,
            &[
                names::COUNTERSPELL,
                names::PLAINS,
                names::MANA_CRYPT,
                names::OUST,
                names::ISLAND,
            ],
        )
        .hand(P1, &[names::MANA_TITHE, names::SNAPCASTER_MAGE])
        .actions(&[
            Action::activate(P0, names::JACE_THE_MIND_SCULPTOR, 0),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            Action::pass(P0),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::PrecombatMain,
            },
            Action::pass(P1),
        ]);
    let (t, states) = at_each_action(&FATESEAL);
    let s = &states[4];
    let library = slots(s, Seat::P1, Zone::Library);
    assert_eq!(s.definition_name(&t, library[0]), names::COUNTERSPELL);
    kept(&t, s, Seat::P0, library[0]);
    varies(&t, s, Seat::P0, &library[1..]);
    varies(&t, s, Seat::P1, &library);
    let s = &states[6];
    let counterspell = slot_of(&t, s, Seat::P1, Zone::Hand, names::COUNTERSPELL);
    kept(&t, s, Seat::P0, counterspell);
    let others: Vec<Slot> = slots(s, Seat::P1, Zone::Hand)
        .into_iter()
        .filter(|&slot| slot != counterspell)
        .collect();
    assert_eq!(others.len(), 2);
    varies(&t, s, Seat::P0, &others);
}

/// A card the opponent knows in a hand (seen on top of the library by Jace, the Mind
/// Sculptor's +2, then drawn) is lost to them once its owner brainstorms: they can't tell which
/// cards went back, so they know neither what stays in the hand nor what went on top.
#[test]
fn brainstorming_hides_a_known_hand_card() {
    const JACE: &[Permanent] = &[Permanent::new(names::JACE_THE_MIND_SCULPTOR)];
    const HIDDEN: Scenario = Scenario::new("Jace fateseals, then the opponent brainstorms")
        .battlefield(P0, JACE)
        .hand(P0, &[names::PLAINS])
        .library(P0, &[names::ISLAND, names::PLAINS])
        .battlefield(P1, JACE)
        .library(
            P1,
            &[
                names::COUNTERSPELL,
                names::PLAINS,
                names::MANA_CRYPT,
                names::OUST,
                names::ISLAND,
            ],
        )
        .hand(P1, &[names::MANA_TITHE, names::SNAPCASTER_MAGE])
        .actions(&[
            Action::activate(P0, names::JACE_THE_MIND_SCULPTOR, 0),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            Action::pass(P0),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::PrecombatMain,
            },
            Action::activate(P1, names::JACE_THE_MIND_SCULPTOR, 1),
            Action::Resolve,
            Action::choose(P1, DecisionKind::ChooseObject, names::MANA_TITHE),
            Action::choose(P1, DecisionKind::ChooseObject, names::SNAPCASTER_MAGE),
            Action::pass(P1),
        ]);
    let (t, states) = at_each_action(&HIDDEN);
    // Drawn, the fatesealed Counterspell is still known in the hand.
    let s = &states[6];
    let counterspell = slot_of(&t, s, Seat::P1, Zone::Hand, names::COUNTERSPELL);
    kept(&t, s, Seat::P0, counterspell);
    let s = &states[10];
    let hand = slots(s, Seat::P1, Zone::Hand);
    assert!(hand.contains(&counterspell));
    let library = slots(s, Seat::P1, Zone::Library);
    for &slot in &library[..2] {
        kept(&t, s, Seat::P1, slot);
    }
    let hidden: Vec<Slot> = hand.iter().chain(&library[..2]).copied().collect();
    varies(&t, s, Seat::P0, &hidden);
}

/// Scry ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): the scrying player
/// knows the cards they put on the bottom and those they kept on top, each in its place.
#[test]
fn scried_cards_are_known_where_they_went() {
    const SCRY: Scenario = Scenario::new("Aang's Iceberg scries an Island to the bottom")
        .battlefield(
            P0,
            &[
                Permanent::new(names::AANGS_ICEBERG),
                Permanent::new(names::PLAINS),
                Permanent::new(names::JACE_VRYNS_PRODIGY),
                Permanent::new(names::STAFF_OF_THE_STORYTELLER),
            ],
        )
        .library(
            P0,
            &[
                names::ISLAND,
                names::PLAINS,
                names::MANA_CRYPT,
                names::COUNTERSPELL,
                names::OUST,
            ],
        )
        .hand(P0, &[names::PLAINS])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::activate(P0, names::AANGS_ICEBERG, 1),
            Action::answer(
                P0,
                DecisionKind::Waterbend,
                Pick::Card {
                    card: names::STAFF_OF_THE_STORYTELLER,
                },
            ),
            Action::choose(P0, DecisionKind::ChooseUpTo, names::ISLAND),
            Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
            Action::pass(P0),
        ]);
    let (t, s) = at_last_action(&SCRY);
    let library = slots(&s, Seat::P0, Zone::Library);
    let (top, bottom) = (library[0], library[library.len() - 1]);
    assert_eq!(s.definition_name(&t, top), names::PLAINS);
    assert_eq!(s.definition_name(&t, bottom), names::ISLAND);
    kept(&t, &s, Seat::P0, top);
    kept(&t, &s, Seat::P0, bottom);
    varies(&t, &s, Seat::P0, &library[1..library.len() - 1]);
    varies(&t, &s, Seat::P1, &[top, bottom]);
}
