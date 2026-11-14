//! What the rules note in the Trackers store, and in which games: that a permanent entered
//! the battlefield this turn, that a loyalty ability of it was activated, and how many
//! spells each player cast and cards each player drew this turn.

use beryl_card::effect::build::{each_opponent, gain_life, if_then, sequence, you};
use beryl_card::{Amount, Condition, Effect, ManaCost};

use super::LAND;
use super::cast::{EVERYTHING, Game, cast, index_of};
use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Face, ObjectAbility, Types};
use crate::event::{self, Entry, Reason};
use crate::ids::Seat;
use crate::state::{Count, Tracker, TurnStep, Zone};
use crate::table::Capability;
use crate::{CardTable, Choice, DecisionKind, Decklist, Opt, Profile};

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

/// "You gain life equal to the number of spells you cast this turn. Each opponent loses life
/// equal to the number of cards you drew this turn. If you drew a card in your draw step this
/// turn, you gain 10 life."
static TALLY: CardDef = CardDef {
    name: "Test Tally",
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::NONE,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&sequence(&[
            Effect::GainLife {
                player: you(),
                amount: Amount::SpellsCast(you()),
            },
            Effect::LoseLife {
                player: each_opponent(),
                amount: Amount::CardsDrawn(you()),
            },
            if_then(Condition::DrewInDrawStep(you()), &gain_life(you(), 10)),
        ]))],
        ..Face::EMPTY
    }],
};

static TALLY_CARDS: CardTable = CardTable::new(&[&LAND, &TALLY]);

/// Each count of `player`'s this turn, in the order of the list.
fn counts(g: &Game, player: Seat) -> [u8; 3] {
    Count::ALL.map(|count| g.s.counted(count, player))
}

/// Passes priority until it is `active`'s precombat main phase of the next turn.
fn next_main(g: &mut Game, active: Seat) {
    let turn = g.s.turn().number();
    while g.s.turn().number() == turn || g.s.turn().step() != TurnStep::PrecombatMain {
        assert_eq!(g.decide().kind, DecisionKind::Priority);
        g.choose(Choice(0));
    }
    assert_eq!(g.s.turn().active(), active);
}

/// Casts a Test Tally from `player`'s library and lets it resolve.
fn tally(g: &mut Game, player: Seat) {
    let slot = g.give(player, "Test Tally", Zone::Hand);
    g.pick(cast(slot));
    g.pass_both();
    assert_eq!(g.s.object(slot).zone(), Zone::Graveyard);
}

#[test]
fn spells_cast_and_cards_drawn_are_counted_only_in_a_game_with_a_card_that_reads_them() {
    let reading: &Decklist = &[(30, "Test Land"), (10, "Test Tally")];
    for (deck, reads) in [(reading, true), (LANDS, false)] {
        let mut g = Game::with(&TALLY_CARDS, deck, Profile::EXACT);
        let (me, them) = (g.me, g.them);
        for count in Count::ALL {
            assert_eq!(g.t.can(count.capability()), reads);
        }
        if reads {
            // The first turn: the starting player skips their draw (CR 103.8a), so the spell
            // counts itself and no card drawn.
            tally(&mut g, me);
            assert_eq!(counts(&g, me), [1, 0, 0]);
            assert_eq!((g.s.player(me).life(), g.s.player(them).life()), (21, 20));
        }

        // The next turn begins with nothing counted, and its player draws in their draw step.
        next_main(&mut g, them);
        assert_eq!(counts(&g, me), [0; 3]);
        assert_eq!(counts(&g, them), if reads { [0, 1, 1] } else { [0; 3] });
        assert_eq!(g.s.check_invariants(&g.t), Ok(()));
        if reads {
            // One spell, one card drawn, and that card in the draw step.
            tally(&mut g, them);
            assert_eq!(counts(&g, them), [1, 1, 1]);
            assert_eq!((g.s.player(me).life(), g.s.player(them).life()), (20, 31));
            // A second: two spells, and the same card drawn.
            tally(&mut g, them);
            assert_eq!(counts(&g, them), [2, 1, 1]);
            assert_eq!((g.s.player(me).life(), g.s.player(them).life()), (19, 43));
        }
    }
}
