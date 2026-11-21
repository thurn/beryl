//! What each player sees of a hidden pick while the other player still has to pick
//! ([CR 101.4a]): "If an effect has each player choose a card in a hidden zone, such as their
//! hand or library, those cards may remain face down as they're chosen. However, each player
//! must clearly indicate which face-down card they are choosing." Both players see which card
//! in the picker's hand was picked; only the picker sees what it is.
//!
//! [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a

use crate::common::fixtures::SHOW_AND_TELL;
use crate::play::Play;

use beryl_cards::{CARDS, names};
use beryl_engine::{Observation, Seat};
use beryl_session::protocol::{Candidate, Moment, ObjectView, Side, ZoneName, player_view};
use beryl_session::{CandidateIndex, Status, StopsSetting};

fn picked(view: impl IntoIterator<Item = ObjectView>) -> Vec<ObjectView> {
    view.into_iter().filter(|o| o.hidden_pick).collect()
}

#[test]
fn a_hidden_pick_is_marked_for_both_players_and_named_only_for_its_picker() {
    let mut seen = None;
    beryl_scenario::visit(&CARDS, SHOW_AND_TELL, |t, s, d| {
        if d.decider != Seat::P1 || seen.is_some() {
            return;
        }
        let view = |viewer| {
            let moment = Moment::Thinking {
                resolving: None,
                decision: None,
            };
            let obs = Observation::new(s, viewer);
            picked(player_view(t, obs, moment, StopsSetting::SMART, &[]).objects)
        };
        seen = Some([view(Seat::P0), view(Seat::P1)]);
    })
    .unwrap_or_else(|e| panic!("{e}"));
    let [mine, theirs] = seen.expect("player 1's pick");
    assert_eq!(mine.len(), 1, "the picker's view marks their pick");
    assert_eq!(theirs.len(), 1, "the other player's view marks it too");
    assert_eq!(mine[0].slot, theirs[0].slot);
    assert_eq!(mine[0].zone, ZoneName::Hand);
    assert_eq!(mine[0].name.as_deref(), Some(names::GRISELBRAND));
    assert_eq!(theirs[0].name, None);
}

/// A session's view while the other player picks is drawn from the position the paused
/// resolution shows, whose own state holds no record of the picks: it must still mark the one
/// made.
#[test]
fn a_session_marks_the_humans_hidden_pick_while_the_opponent_picks() {
    let mut play = Play::scenario(SHOW_AND_TELL);
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    while play.decision().kind == beryl_session::protocol::Kind::Priority {
        play.choose(|c| *c == Candidate::Pass);
    }
    let griselbrand = play.named(names::GRISELBRAND, ZoneName::Hand, Side::You)[0];
    let decision = play.decision();
    let index = (decision.candidates.iter())
        .position(|c| c.option == Candidate::Object { slot: griselbrand })
        .expect("Griselbrand among the candidates");
    let answer = (play.game)
        .answer(decision.id, CandidateIndex(index as u16))
        .unwrap();
    play.game.choose(answer);
    let thinking: Vec<_> = std::iter::from_fn(|| play.game.next_view())
        .filter(|v| matches!(v.status, Status::Thinking))
        .map(|v| picked(v.objects))
        .collect();
    assert!(
        thinking.iter().any(|picks| picks.len() == 1
            && picks[0].slot == griselbrand
            && picks[0].zone == ZoneName::Hand),
        "a view while the opponent picks marks Griselbrand: {thinking:?}"
    );
}
