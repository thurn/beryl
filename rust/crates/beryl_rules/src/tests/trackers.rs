//! What the rules note in the Trackers store, and in which games: that a permanent entered
//! the battlefield this turn, and that a loyalty ability of it was activated.

use super::cast::{EVERYTHING, Game, index_of};
use crate::bitboard::Bitboard;
use crate::card::ObjectAbility;
use crate::event::{self, Entry, Reason};
use crate::state::{Tracker, TurnStep, Zone};
use crate::table::Capability;
use crate::{Choice, DecisionKind, Decklist, Opt, Profile};

/// A deck with no card that reads a tracker.
const LANDS: &Decklist = &[(40, "Test Land")];

#[test]
fn a_permanent_is_noted_as_entering_only_in_a_game_with_a_card_that_reads_it() {
    for (deck, reads) in [(EVERYTHING, true), (LANDS, false)] {
        let mut g = Game::of(deck, Profile::EXACT);
        assert_eq!(g.t.can(Capability::EnteredThisTurn), reads);
        // One on the battlefield since before the turn, and one played this turn.
        let old = g.give(g.me, "Test Land", Zone::Battlefield);
        let land = g.give(g.me, "Test Land", Zone::Hand);
        g.pick(Opt::PlayLand(land));
        assert_eq!(g.s.object(land).zone(), Zone::Battlefield);
        assert_eq!(g.s.tracked(Tracker::EnteredThisTurn, land), reads);
        assert!(!g.s.tracked(Tracker::EnteredThisTurn, old));

        // The next turn begins with nothing noted of it.
        let turn = g.s.turn().number();
        while g.s.turn().number() == turn {
            assert_eq!(g.decide().kind, DecisionKind::Priority);
            g.choose(Choice(0));
        }
        assert!(!g.s.tracked(Tracker::EnteredThisTurn, land));
    }
}

#[test]
fn a_permanent_that_leaves_and_returns_entered_this_turn_with_no_loyalty_ability_activated() {
    let mut g = Game::new(Profile::EXACT);
    // On the battlefield since before the turn began.
    let emperor = g.give(g.me, "Test Emperor", Zone::Battlefield);
    let plus = Opt::Activate {
        slot: emperor,
        ability: ObjectAbility::own(1),
    };
    assert!(!g.s.tracked(Tracker::EnteredThisTurn, emperor));
    g.pick(plus);
    g.pass_both();
    assert!(g.s.tracked(Tracker::LoyaltyActivated, emperor));
    assert!(
        index_of(&g.current(), plus).is_none(),
        "CR 606.3: once a turn"
    );

    // The object it returns as is new (CR 400.7): it entered this turn, and no loyalty
    // ability of it has been activated.
    for zone in [Zone::Hand, Zone::Battlefield] {
        event::move_batch(
            &mut g.s,
            &g.t,
            &mut g.x.events,
            Bitboard::single(emperor),
            zone,
            Reason::Return,
            Entry::default(),
        )
        .unwrap();
    }
    assert!(g.s.tracked(Tracker::EnteredThisTurn, emperor));
    assert!(!g.s.tracked(Tracker::LoyaltyActivated, emperor));
    assert!(index_of(&g.current(), plus).is_some());
    // And at instant speed, the turn it entered.
    g.at(g.me, TurnStep::End);
    assert!(index_of(&g.current(), plus).is_some());
}
