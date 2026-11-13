//! The order of a view's steps and what stays put: what the old board's checks
//! held by watching flights, which on the stage is the session's script. Each test here holds
//! one of those checks (`ui/scripts/interactions.mjs`, deleted at the cut-over, by-w8gv.21.65)
//! to a script written by hand:
//!
//! - "a cast stays on the stack while its graveyard exile cost plays" and "mana payment
//!   precedes graveyard exile departures": [`a_costs_mana_is_paid_before_its_cards_leave`].
//! - "resolution payment mana reaches its spell before it leaves":
//!   [`mana_paid_as_a_spell_resolves_streaks_into_it_before_it_leaves`].
//! - "Staff's Spirit and story counter triggers play in sequence":
//!   [`two_abilities_of_one_source_each_emerge_as_their_own`].
//! - "pending Counterspell preserves only Mana Tithe's committed link":
//!   [`a_spell_waiting_on_its_target_keeps_the_arrow_under_it`].
//! - "self-target is green": [`an_ability_aimed_at_its_own_controller_draws_a_friendly_line`].
//!
//! The rest of that bead's checks are held elsewhere: "a beneficial own permanent target
//! submits directly" by `tests/clicks.rs`
//! (`a_target_of_ones_own_that_is_helped_is_picked_unasked`); "opponent hand stays parked
//! during a private choice" and "an opponent searching keeps their library parked" by the
//! verb suite's `lift` case (`ui/scripts/verb-cases.mjs`); "simultaneous exile departures
//! span three hundred milliseconds" and "a nameless two-two token shows the Double Masters
//! Copy art" with no browser (`ui/scripts/stage-zones.test.mjs`,
//! `ui/scripts/card-records.test.mjs`).

mod common;
#[path = "common/play.rs"]
mod play;

use beryl_cards::names::{
    self, COUNTERSPELL, ISLAND, JACE_WIELDER_OF_MYSTERIES, MANA_CRYPT, MANA_TITHE, PLAINS,
};
use beryl_engine::TurnStep;
use beryl_engine::card::Purpose;
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::CandidateIndex;
use beryl_session::protocol::{Candidate, PlayerView, Side, StackItem, ZoneName};
use beryl_session::script::{Act, Screen};
use common::{script, text, views, views_heard};
use play::{Casts, Play};

/// A view's script as text.
fn written(view: &PlayerView) -> String {
    view.script.as_ref().expect("a view's script").to_string()
}

/// Answers the open decision with the candidate `wanted` names and plays on to the human's
/// next decision: the scripts of every view sent on the way, as text.
fn played(play: &mut Play, wanted: impl Fn(&Candidate) -> bool) -> String {
    let decision = play.decision();
    let index = (decision.candidates.iter())
        .position(|c| wanted(&c.option))
        .expect("the candidate");
    let answer = (play.game)
        .answer(decision.id, CandidateIndex(index as u16))
        .unwrap();
    play.game.choose(answer);
    let mut scripts = String::new();
    while let Some(view) = play.game.next_view() {
        scripts += &written(&view);
        play.view = view;
    }
    scripts
}

/// The steps of `script` that begin or end an arrow.
fn arrows(script: &str) -> Vec<&str> {
    script.lines().filter(|l| l.contains("point ")).collect()
}

/// A cast's costs in the order they are paid, and the spell on the stack throughout: the card
/// flies to the stack, its lands tap and their mana streaks into it, and only then do the
/// cards exiled for it leave the graveyard, together: the engine moves one cost's cards in
/// one group after the last is chosen (CR 601.2h). No step takes the spell off the stack.
/// A session sends the whole cast as one view when nothing is asked on the way, so the
/// scenario's three views of it (the cast, the payment, the cards chosen) are joined here.
#[test]
fn a_costs_mana_is_paid_before_its_cards_leave() {
    let name = "Abhorrent Oculus exiles six of seven graveyard cards as it's cast";
    let views = views_heard(names::ABHORRENT_OCULUS, name, |_, _, _| {});
    for (seat, who) in ["you", "opponent"].into_iter().enumerate() {
        let mut screen = Screen::default();
        let (first, heard) = &views[0][seat];
        screen.script(first, heard);
        let (mut cast, mut heard) = views[3][seat].clone();
        let joined = || views[1..=3].iter().map(|seats| &seats[seat]);
        cast.events = joined().flat_map(|(view, _)| view.events.clone()).collect();
        heard.causes = joined().flat_map(|(_, h)| h.causes.clone()).collect();
        heard.together = joined().flat_map(|(_, h)| h.together.clone()).collect();
        let script = screen.script(&cast, &heard).to_string();
        assert_eq!(
            script,
            text(&[
                format!("fly #1 (Abhorrent Oculus) hand>stack cast <- {who}"),
                format!("tap #0 (Island) <- {who}"),
                format!("& tap #6 (Island) <- {who}"),
                format!("& tap #9 (Island) <- {who}"),
                "& streak #0 > #1 blue 1 <- #0".to_owned(),
                "& streak #6 > #1 blue 1 <- #6".to_owned(),
                "& streak #9 > #1 blue 1 <- #9".to_owned(),
                format!("fly #5 (Counterspell) graveyard>exile exile <- {who}"),
                format!("& fly #2 (Jace, the Mind Sculptor) graveyard>exile exile <- {who}"),
                format!("& fly #3 (Oust) graveyard>exile exile <- {who}"),
                format!("& fly #11 (Portable Hole) graveyard>exile exile <- {who}"),
                format!("& fly #8 (Sink into Stupor) graveyard>exile exile <- {who}"),
                format!("& fly #7 (Timetwister) graveyard>exile exile <- {who}"),
            ]),
            "seat {seat}"
        );
        assert!(
            matches!(cast.stack.as_slice(), [StackItem::Spell { slot: 1, .. }]),
            "the spell is on the stack as its costs are paid"
        );
    }
}

/// Mana paid while a spell resolves ("unless its controller pays {1}") streaks into the spell
/// that asked for it, whose arrow then ends and which only then leaves the stack.
#[test]
fn mana_paid_as_a_spell_resolves_streaks_into_it_before_it_leaves() {
    let name = "Mana Tithe: lands left after paying for the spell pay {1}";
    let [first, second] = script(names::MANA_TITHE, name, 3);
    // The payer taps the land, and its mana is the land's own ability's.
    let paid = |who: &str| {
        text(&[
            format!("tap #6 (Plains) <- {who}"),
            "& streak #6 > #258 white 1 <- #6".to_owned(),
            "-point hostile-target #258 > #2".to_owned(),
            "vanish #258 (Mana Tithe) stack>graveyard resolve <- #258".to_owned(),
            "fly #2 (Jace, the Mind Sculptor) stack>battlefield resolve <- #2".to_owned(),
        ])
    };
    assert_eq!(first, paid("you"));
    assert_eq!(second, paid("opponent"));
}

/// Two triggered abilities of one permanent are two things on the stage, each with its own
/// number and its own stack item, which says which of the card's abilities it is: the stage
/// draws an ability's text from its item (`ui/src/stage/Faces.tsx`), so neither can show the
/// other's. The first has resolved and gone before the second emerges.
#[test]
fn two_abilities_of_one_source_each_emerge_as_their_own() {
    let name = "its own Spirit and a Samurai each add a story counter, spent to draw";
    let all = views(names::STAFF_OF_THE_STORYTELLER, name, |_, _, _| {});
    for seat in 0..2 {
        let emerged: Vec<(u32, Option<u16>, u8, u8)> = (all[..=3].iter())
            .flat_map(|seats| {
                seats[seat]
                    .script
                    .iter()
                    .flat_map(|s| s.beats.iter().flatten())
            })
            .filter_map(|step| match &step.act {
                Act::Emerge {
                    id,
                    item:
                        StackItem::Ability {
                            source,
                            ability,
                            nth,
                            ..
                        },
                } => Some((*id, *source, *ability, *nth)),
                _ => None,
            })
            .collect();
        // "When this artifact enters, create a 1/1 white Spirit creature token with flying."
        // and then "Whenever you create one or more creature tokens, put a story counter on
        // this artifact.": the card's first and second abilities, and of its triggered ones.
        assert_eq!(
            emerged,
            [(1, Some(2), 0, 0), (2, Some(2), 1, 1)],
            "seat {seat}"
        );
        let who = ["you", "opponent"][seat];
        assert_eq!(
            written(&all[3][seat]),
            text(&[
                "appear #512 (Spirit Token) nowhere>battlefield created <- a1".to_owned(),
                "-point source a1 > #2".to_owned(),
                "vanish a1 stack>gone resolve <- a1".to_owned(),
                format!("emerge a2 (Staff of the Storyteller, triggered) {who} from #2 <- {who}"),
                "& +point source a2 > #2".to_owned(),
            ])
        );
    }
}

/// P0 casts a Mana Crypt, P1 answers it with Mana Tithe, and P0 holds a Counterspell.
const TITHED: Scenario = Scenario::new("stack targets")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[MANA_CRYPT, COUNTERSPELL])
    .permanents(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
    .hand(P1, &[MANA_TITHE])
    .permanents(P1, &[Permanent::new(PLAINS)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// A spell being cast, on the stack and waiting for its target, draws no arrow yet and ends
/// none: the arrow of the spell under it, begun when that was cast, is still drawn. Its own
/// begins once its target is chosen.
#[test]
fn a_spell_waiting_on_its_target_keeps_the_arrow_under_it() {
    let mut play = Play::against(&TITHED, Some(Box::new(Casts)));
    let crypt = play.named(MANA_CRYPT, ZoneName::Hand, Side::You)[0];
    let counterspell = play.named(COUNTERSPELL, ZoneName::Hand, Side::You)[0];
    let cast =
        |slot: u16| move |c: &Candidate| matches!(c, Candidate::Cast { slot: s, .. } if *s == slot);
    let answered = played(&mut play, cast(crypt));
    let tithe = play.named(MANA_TITHE, ZoneName::Stack, Side::Opponent)[0];
    let aimed = format!("point hostile-target #{tithe} > #{crypt}");
    assert_eq!(arrows(&answered), [format!("& +{aimed}")]);
    // Counterspell goes on the stack over the two, and asks for its target.
    assert_eq!(
        played(&mut play, cast(counterspell)),
        format!("fly #{counterspell} (Counterspell) hand>stack cast <- you\n"),
    );
    assert!(play.decision().prompt.is(Purpose::Target));
    let chosen = played(&mut play, |c| *c == Candidate::Object { slot: tithe });
    // Its own arrow begins with its target, and the one under it ends only as the spells
    // resolve.
    assert_eq!(
        arrows(&chosen)[..2],
        [
            format!("& +point hostile-target #{counterspell} > #{tithe}"),
            format!("-{aimed}"),
        ]
    );
}

/// P0's Jace, Wielder of Mysteries, whose +1 targets a player.
const JACE: Scenario = Scenario::new("a player targeted")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(JACE_WIELDER_OF_MYSTERIES)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// An ability aimed at its own controller draws a friendly line to that player, and one aimed
/// at the other player a hostile one (UI Conventions, exception 4: by relationship).
#[test]
fn an_ability_aimed_at_its_own_controller_draws_a_friendly_line() {
    for (player, line) in [
        (Side::You, "& +point friendly-target a1 > you"),
        (Side::Opponent, "& +point hostile-target a1 > opponent"),
    ] {
        let mut play = Play::scenario(&JACE);
        played(&mut play, |c| {
            matches!(c, Candidate::Activate { ability: 1, .. })
        });
        let script = played(&mut play, |c| *c == Candidate::Player { player });
        assert_eq!(arrows(&script)[0], line, "{script}");
    }
}
