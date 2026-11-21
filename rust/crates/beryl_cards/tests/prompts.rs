//! What every decision says of its prompt ([`beryl_engine::card::Prompt`]), as the cards'
//! scenarios reach it: a consumer folds a run of picks into one prompt from the record alone,
//! so the record must keep to the rules its shape and part set.

use beryl_cards::props::Prop;
use beryl_cards::{CARDS, names};
use beryl_engine::card::{Part, Place, Purpose, Shape};
use beryl_engine::{
    Bitboard, Candidates, Decision, DecisionKind, Opt, Scratch, State, Zone, ZoneKey, view,
};
use beryl_scenario::Scenario;
use beryl_scenario::prelude::{ME, P0, P1, Resolve, THEM, TurnStep, advance_to, stack};

/// Every decision the scenarios reach, with the state it is asked in. For a question a
/// resolving spell or ability asks, the state includes the effects resolved so far.
fn decisions(scenarios: &[Scenario], mut each: impl FnMut(&str, &State, &Decision)) {
    for scenario in scenarios {
        beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
            each(scenario.name, view(s, t, &mut Scratch::new()), d);
        })
        .unwrap_or_else(|e| panic!("{e}"));
    }
}

/// The objects a decision's candidates name, for picks among objects.
fn offered(d: &Decision) -> Bitboard {
    match &d.candidates {
        Candidates::Objects(cards) => *cards,
        Candidates::Options(opts) => opts
            .iter()
            .filter_map(|&o| match o {
                Opt::Object(slot) => Some(slot),
                _ => None,
            })
            .collect(),
    }
}

#[test]
fn every_decision_says_a_prompt_its_shape_and_part_allow() {
    for &(card, scenarios) in beryl_cards::scenarios() {
        decisions(scenarios, |scenario, _, d| {
            let at = format!("{card} / {scenario}: {d:?}");
            let (shape, part) = (d.prompt.shape(), d.prompt.part);
            // Each part belongs to the shapes whose runs have it.
            let fits = match part {
                Part::Pick => !matches!(shape, Shape::Split),
                Part::Whether | Part::Instead | Part::Count => shape == Shape::Pick,
                Part::First | Part::Second => shape == Shape::Split,
                Part::Aim => shape == Shape::Declare,
                Part::Plan => shape == Shape::Pay,
            };
            assert!(fits, "{part:?} is no part of {shape:?}: {at}");
            // A yes or no is a prompt of that shape, or opens a pick's run.
            if d.kind == DecisionKind::YesNo {
                assert!(
                    shape == Shape::YesNo || matches!(part, Part::Whether | Part::Instead),
                    "{at}"
                );
            }
            // The object set is what the shape and part say it is, and nothing elsewhere.
            match (shape, part) {
                (_, Part::Whether | Part::Instead | Part::Aim) => {
                    assert!(!d.about.is_empty(), "nothing to pick among or aim: {at}");
                }
                (Shape::YesNo | Shape::Mode, _) => {}
                // The pick after a `Whether` keeps the set its candidates stand for.
                _ => assert!(
                    (offered(d) & !d.about).is_empty() || d.about.is_empty(),
                    "{at}"
                ),
            }
            // A split names where its first cards go.
            if shape == Shape::Split {
                assert_ne!(d.prompt.places[0], Place::None, "{at}");
            }
        });
    }
}

#[test]
fn a_discard_run_counts_down_and_its_alternative_names_the_basic_lands() {
    let mut instead = false;
    let mut left = Vec::new();
    decisions(&[THIRST], |_, s, d| {
        if d.prompt.purpose != Purpose::Discard {
            return;
        }
        if d.prompt.part == Part::Instead {
            // Two cards, unless one of these: every one a card in the player's hand.
            assert_eq!(d.prompt.most, 2);
            assert!((d.about & !s.zone(ZoneKey::Of(d.player, Zone::Hand))).is_empty());
            instead = true;
        } else {
            left.push(d.prompt.most);
        }
    });
    assert!(instead, "no scenario offers the basic land");
    assert!(left.contains(&2), "no scenario discards two: {left:?}");
    assert!(left.iter().all(|&most| (1..=2).contains(&most)), "{left:?}");
}

#[test]
fn a_look_keeps_its_cards_from_the_yes_or_no_through_the_pick() {
    let mut picked = false;
    for scenario in [TRAINER, SPELLBINDER] {
        let mut whether = None;
        decisions(&[scenario], |_, s, d| {
            if d.prompt.purpose != Purpose::Look {
                return;
            }
            // The cards lie together in one player's library or hand: where any one of them
            // is, the rest are.
            let mut about = beryl_engine::Slot::all().filter(|&slot| (d.about).contains(slot));
            let first = s.object(about.next().expect("cards to look at"));
            assert!(matches!(first.zone(), Zone::Library | Zone::Hand));
            assert!((d.about & !s.zone(ZoneKey::Of(first.owner(), first.zone()))).is_empty());
            if d.prompt.part == Part::Whether {
                whether = Some(d.about);
            } else {
                assert_eq!(Some(d.about), whether, "the pick's set is the question's");
                assert!((offered(d) & !d.about).is_empty());
                picked = true;
            }
        });
    }
    assert!(picked, "no look reached its pick");
}

const THIRST: Scenario =
    Scenario::new("Thirst for Discovery: discarding two cards with a basic land in hand")
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(P0, &[names::THIRST_FOR_DISCOVERY])
        .library(
            P0,
            &[
                names::PLAINS,
                Prop::Artifact,
                names::COUNTERSPELL,
                names::ISLAND,
            ],
        )
        .actions(&[
            ME.casts(names::THIRST_FOR_DISCOVERY),
            Resolve,
            ME.no(),
            ME.picks(Prop::Artifact),
            ME.picks(names::PLAINS),
            ME.has(Zone::Graveyard, Prop::Artifact, 1),
            ME.has(Zone::Graveyard, names::PLAINS, 1),
            ME.has(Zone::Hand, names::COUNTERSPELL, 1),
        ]);

const TRAINER: Scenario = Scenario::new("Thundertrap Trainer takes a spell")
    .hand(P0, &[names::THUNDERTRAP_TRAINER])
    .battlefield(P0, &[names::ISLAND; 2])
    .library(
        P0,
        &[
            names::PLAINS,
            Prop::Instant,
            Prop::Flier,
            names::OUST,
            Prop::Artifact,
        ],
    )
    .library(P1, &[names::PLAINS])
    .actions(&[
        ME.casts(names::THUNDERTRAP_TRAINER),
        Resolve,
        Resolve,
        ME.yes(),
        ME.deciding(DecisionKind::ChooseObject, 2),
        ME.picks(names::OUST),
        ME.has(Zone::Hand, names::OUST, 1),
        ME.has_cards(Zone::Library, 4),
        advance_to(3, TurnStep::PrecombatMain),
        ME.has(Zone::Hand, Prop::Artifact, 1),
    ]);

const SPELLBINDER: Scenario = Scenario::new("Spellbinder's card costs its owner {2} more")
    .hand(P0, &[names::ELITE_SPELLBINDER])
    .battlefield(P0, &[names::PLAINS; 3])
    .hand(P1, &[Prop::Artifact, names::ISLAND])
    .battlefield(P1, &[names::PLAINS; 3])
    .library(P1, &[names::ISLAND])
    .actions(&[
        ME.casts(names::ELITE_SPELLBINDER),
        Resolve,
        stack(1),
        Resolve,
        ME.yes(),
        THEM.has(Zone::Exile, Prop::Artifact, 1),
        THEM.has(Zone::Hand, Prop::Artifact, 0),
        advance_to(2, TurnStep::PrecombatMain),
        THEM.deciding(DecisionKind::Priority, 4),
        THEM.casts(Prop::Artifact),
        THEM.tapped(names::PLAINS, 3),
        Resolve,
        THEM.controls(Prop::Artifact, 1),
    ]);
