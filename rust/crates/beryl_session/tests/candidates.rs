//! What each candidate tells the UI beyond its answer (`CandidateView`), as the cards' own
//! scenarios reach it: the label it shows, and the paragraph of the card an activation's
//! ability is.

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::state::Pending;
use beryl_engine::{Observation, Scratch, view};
use beryl_session::protocol::{Candidate, CandidateView, Moment, player_view};
use beryl_session::{DecisionId, SessionId, Status, StopsSetting};

/// Every candidate offered at a decision of `card`'s scenarios, with the name of the object
/// it is about, if any.
fn candidates(card: &str) -> Vec<(CandidateView, Option<String>)> {
    let (_, scenarios) = SCENARIOS
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
