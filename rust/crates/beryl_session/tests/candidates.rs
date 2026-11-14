//! What each candidate tells the UI beyond its answer (`CandidateView`), as the cards' own
//! scenarios reach it: the label it shows, and the paragraph of the card an activation's or
//! a waiting trigger's ability is.

use crate::play;

use beryl_cards::names::{ADELINE_RESPLENDENT_CATHAR, ISLAND, OVERLORD_OF_THE_MISTMOORS};
use beryl_cards::{CARDS, names};
use beryl_engine::state::Pending;
use beryl_engine::{Observation, Scratch, TurnStep, view};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::prompt::ButtonId;
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, CandidateView, Kind, Moment, StackItem, player_view};
use beryl_session::{DecisionId, SessionId, Status, StopsSetting};
use play::Play;

/// Every candidate offered at a decision of `card`'s scenarios, with the name of the object
/// it is about, if any.
fn candidates(card: &str) -> Vec<(CandidateView, Option<String>)> {
    let (_, scenarios) = beryl_cards::scenarios()
        .iter()
        .find(|&&(name, _)| name == card)
        .expect("the card's scenarios");
    let mut seen = Vec::new();
    for scenario in *scenarios {
        beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
            let mut x = Scratch::new();
            let resolving = matches!(s.pending(), Pending::Resolving(_))
                .then(|| s.stack().last())
                .flatten();
            let id = DecisionId {
                session: SessionId(1),
                seq: 1,
            };
            let v = player_view(
                t,
                Observation::new(view(s, t, &mut x), d.player),
                Moment::Deciding(id, d, resolving),
                StopsSetting::SMART,
                &[],
            );
            let Status::Deciding { decision } = v.status else {
                panic!("no decision is open");
            };
            for c in decision.candidates {
                let slot = match c.option {
                    Candidate::PlayLand { slot } | Candidate::Activate { slot, .. } => Some(slot),
                    _ => None,
                };
                let name = slot.and_then(|slot| {
                    v.objects
                        .iter()
                        .find(|o| o.slot == slot)
                        .and_then(|o| o.name.clone())
                });
                seen.push((c, name));
            }
        })
        .unwrap_or_else(|e| panic!("{e}"));
    }
    seen
}

#[test]
fn a_modal_double_faced_land_is_played_by_its_land_face() {
    let plays: Vec<_> = candidates(names::SINK_INTO_STUPOR)
        .into_iter()
        .filter(|(c, name)| {
            matches!(c.option, Candidate::PlayLand { .. })
                && name.as_deref() == Some(names::SINK_INTO_STUPOR)
        })
        .map(|(c, _)| c.label)
        .collect();
    assert!(!plays.is_empty(), "a scenario plays Sink into Stupor");
    assert!(
        plays.iter().all(|label| label == "Play Soporific Springs"),
        "{plays:?}"
    );
}

#[test]
fn a_loyalty_ability_says_which_of_the_cards_abilities_it_is() {
    let mut abilities: Vec<_> = candidates(names::JACE_THE_MIND_SCULPTOR)
        .into_iter()
        .filter(|(c, name)| {
            matches!(c.option, Candidate::Activate { .. })
                && name.as_deref() == Some(names::JACE_THE_MIND_SCULPTOR)
        })
        .map(|(c, _)| (c.nth, c.label))
        .collect();
    abilities.sort();
    abilities.dedup();
    let label = |cost: &str| format!("{}: {cost}", names::JACE_THE_MIND_SCULPTOR);
    assert_eq!(
        abilities,
        [
            (Some(0), label("+2")),
            (Some(1), label("0")),
            (Some(2), label("\u{2212}1")),
            (Some(3), label("\u{2212}12")),
        ]
    );
}

/// Overlord of the Mistmoors, cast for its full cost, and Adeline, Resplendent Cathar ready
/// to attack, each with a trigger on attacking.
const OVERLORD: Scenario = Scenario::new("overlord attacks")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new(OVERLORD_OF_THE_MISTMOORS),
            Permanent::new(ADELINE_RESPLENDENT_CATHAR),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// A waiting trigger says which paragraph of its card is its ability, the one its stack item
/// says once it is on the stack: "Whenever this permanent enters or attacks" is the second
/// paragraph that says a triggered ability (impending's reminder text is the first), though
/// the face lists the attack trigger third, after the enters trigger the paragraph also says.
/// Adeline's "Whenever you attack" is her first. Full control asks for their order.
#[test]
fn a_waiting_trigger_says_which_paragraph_of_its_card_it_is() {
    let mut play = Play::with(&OVERLORD, None, StopsSetting::EXACT);
    while play.decision().kind != Kind::Attacker {
        play.choose(|c| *c == Candidate::Pass);
    }
    assert!(play.press(ButtonId::Auto).is_some());
    assert_eq!(play.press(ButtonId::Confirm), None, "both attack");
    let order = play.decision();
    assert_eq!(order.kind, Kind::TriggerOrder);
    let source =
        |play: &Play, slot: Option<ObjectId>| play.object(slot.expect("a source")).name.clone();
    let mut waiting: Vec<_> = (order.candidates.iter())
        .map(|c| match c.option {
            Candidate::Trigger { source: slot, .. } => (source(&play, slot), c.nth),
            _ => panic!("only triggers are ordered"),
        })
        .collect();
    waiting.sort();
    let named = |name: &str, nth: u8| (Some(name.to_owned()), nth);
    let want = [
        named(ADELINE_RESPLENDENT_CATHAR, 0),
        named(OVERLORD_OF_THE_MISTMOORS, 1),
    ];
    assert_eq!(waiting, want.clone().map(|(name, nth)| (name, Some(nth))));
    play.choose(|c| matches!(c, Candidate::Trigger { .. }));
    let mut items: Vec<_> = (play.view.stack.iter())
        .map(|item| match item {
            StackItem::Ability {
                source: slot,
                triggered: true,
                nth,
                ..
            } => (source(&play, *slot), *nth),
            item => panic!("only the triggers are on the stack: {item:?}"),
        })
        .collect();
    items.sort();
    assert_eq!(items, want);
}
