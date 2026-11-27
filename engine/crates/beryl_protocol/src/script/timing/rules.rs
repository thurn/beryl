//! The rules that change a step's timing from its case's values in the data, and the effects
//! a step begins, each on hand-built steps. Before the session sent each step's timing, the
//! web interface had these rules too, and its unit tests checked them there.
//!
//! Times compared exactly are copied from the data or from another timing, never computed.
#![allow(clippy::float_cmp)]

use super::conventions::Conventions;
use super::tests::{card, fly, mark, object, player, point, vanish};
use super::{Edge, StepTiming, case_of, effects_of, timing_of};
use crate::protocol::{ObjectId, ObjectView, Side, Why, ZoneName};
use crate::script::{Act, Arrow, Fact, Mark, Notice, Span, Stream, Subject, Table, Tally, Verb};

/// The timing of `act` alone in its beat, with no step before it.
fn alone(act: &Act) -> StepTiming {
    timing_of(act, &Table::empty(20), None, &[act])
}

/// The timing of each step of the beat `beat`, each with the step before it.
fn beat(beat: &[Act]) -> Vec<StepTiming> {
    let table = Table::empty(20);
    let beside: Vec<&Act> = beat.iter().collect();
    beat.iter()
        .enumerate()
        .map(|(i, act)| timing_of(act, &table, i.checked_sub(1).map(|j| &beat[j]), &beside))
        .collect()
}

/// Each effect `act` begins as (name, edge, what it is on, how far into the step).
fn effects(act: &Act) -> Vec<(String, Edge, Option<Subject>, Option<f64>)> {
    effects_of(act, &Table::empty(20))
        .into_iter()
        .map(|effect| (effect.name, effect.when, effect.on, effect.into))
        .collect()
}

fn strike(source: u16, to: Subject) -> Act {
    Act::Strike {
        source: ObjectId(source),
        to,
        amount: 2,
        object: None,
    }
}

fn streak(from: Option<Subject>, to: Subject, stream: Stream) -> Act {
    Act::Streak {
        from,
        to,
        stream,
        amount: 1,
        object: None,
    }
}

fn mana() -> Stream {
    Stream::Mana {
        color: crate::protocol::Mana::Blue,
    }
}

fn opponent_life(life: i32) -> Act {
    Act::Count {
        tally: Tally::Life {
            player: Side::Opponent,
            life,
        },
    }
}

/// A finished ability is cut at once (UI Conventions, "A finished ability"), and what it left
/// room for then closes up.
#[test]
fn a_finished_ability_is_cut_and_closes_up() {
    let timing = alone(&vanish(Subject::Ability { id: 1 }, Why::Resolve, None));
    assert_eq!((timing.duration, timing.delay), (0.0, 0.0));
    assert!(timing.closes.is_some_and(|closes| closes > 0.0));
}

/// Only a permanent turns over: a card's name learned elsewhere is no motion.
#[test]
fn only_a_permanent_turns_face_up() {
    let flip = |zone| Act::Flip {
        object: card(1, zone, Side::You),
    };
    assert_eq!(alone(&flip(ZoneName::Library)).duration, 0.0);
    assert!(alone(&flip(ZoneName::Battlefield)).duration > 0.0);
}

/// A permanent tapped and sacrificed in one beat leaves only once it has turned.
#[test]
fn a_permanent_that_taps_and_leaves_in_one_beat_leaves_turned() {
    let tapped = ObjectView {
        tapped: true,
        ..card(1, ZoneName::Battlefield, Side::You)
    };
    let sacrificed = vanish(
        object(1),
        Why::Sacrifice,
        Some(card(1, ZoneName::Graveyard, Side::You)),
    );
    let timings = beat(&[Act::Tap { object: tapped }, sacrificed.clone()]);
    assert_eq!(timings[1].after, [object(1)]);
    assert!(alone(&sacrificed).after.is_empty());
}

/// An arrow goes with the step that calls for it and waits for its ends that move; its end
/// waits until it has been read and holds up what follows. The line to a source waits for
/// nothing, and goes with the step before it as it ends. A mark is a turn of its own.
#[test]
fn arrows_go_with_their_step_and_are_read_before_they_end() {
    let at_card = alone(&point(Span::Begin, Arrow::HostileTarget, object(2)));
    assert!(at_card.with);
    assert_eq!(at_card.after, [object(1), object(2)]);
    let at_player = alone(&point(Span::Begin, Arrow::Attack, player(Side::Opponent)));
    assert_eq!(at_player.after, [object(1)]);
    let source = alone(&point(Span::Begin, Arrow::Source, object(2)));
    assert!(source.with && source.after.is_empty());

    let read = alone(&point(Span::End, Arrow::HostileTarget, object(2)));
    let values = Conventions::get().timing(Verb::Point, Some("end"));
    assert_eq!(read.delay, 0.0);
    assert_eq!(read.hold, Some(values.delay));
    assert!(read.with && read.leads);
    let source_end = alone(&point(Span::End, Arrow::Source, object(2)));
    assert!(source_end.with && !source_end.leads && source_end.hold.is_none());

    for span in [Span::Begin, Span::End] {
        let marked = alone(&mark(span, Mark::Eligible));
        assert!(!marked.with && marked.after.is_empty() && marked.hold.is_none());
    }
}

/// An announcement passes over the table while play goes on under it; a coin is waited for,
/// and is thrown only once nothing else passes over the table.
#[test]
fn announcements_pass_over_the_table_but_a_coin_is_waited_for() {
    let announce = |notice| alone(&Act::Announce { notice });
    let passing = [
        Notice::Turn {
            active: Side::Opponent,
        },
        Notice::Scry {
            player: Side::You,
            top: 1,
            bottom: 1,
        },
        Notice::TopOrBottom {
            player: Side::Opponent,
            top: false,
        },
        Notice::Chosen {
            player: Side::Opponent,
            value: "Plains".into(),
        },
    ];
    for notice in passing {
        let timing = announce(notice);
        assert!(timing.passes && !timing.alone, "{timing:?}");
    }
    let coin = announce(Notice::Coin {
        player: Side::You,
        won: true,
        source: ObjectId(1),
    });
    assert!(!coin.passes && coin.alone);
}

/// A row opens for a permanent before it appears there; a hand dealt opens nothing.
#[test]
fn a_permanent_that_appears_finds_its_row_open() {
    let appear = |from, why, zone| Act::Appear {
        from,
        why,
        object: card(1, zone, Side::You),
        at: None,
        item: None,
    };
    let token = appear(ZoneName::Nowhere, Why::Created, ZoneName::Battlefield);
    assert!(alone(&token).opens);
    let dealt = appear(ZoneName::Library, Why::Draw, ZoneName::Hand);
    assert!(!alone(&dealt).opens);
}

/// The hand makes room for every card of a draw at once; a card an effect puts there that
/// the player can read is no draw.
#[test]
fn the_cards_of_a_draw_go_ahead_together() {
    let drawn = fly(
        ZoneName::Library,
        Why::Draw,
        card(1, ZoneName::Hand, Side::You),
    );
    assert!(alone(&drawn).ahead);
    let returned = fly(
        ZoneName::Library,
        Why::Return,
        card(1, ZoneName::Hand, Side::You),
    );
    assert!(!alone(&returned).ahead);
}

/// The blow a blocker deals back is one with the attacker's strike at it.
#[test]
fn the_blow_dealt_back_goes_with_the_strike() {
    let timings = beat(&[strike(1, object(2)), strike(2, object(1))]);
    assert!(!timings[0].with);
    assert!(timings[1].with);
}

/// The life a streak of damage or a strike takes from a player goes as it lands on them.
#[test]
fn a_life_total_changes_as_what_hits_it_lands() {
    let burn = streak(
        Some(Subject::Ability { id: 1 }),
        player(Side::Opponent),
        Stream::Damage,
    );
    let timings = beat(&[burn.clone(), opponent_life(17)]);
    assert!(timings[1].with);
    assert_eq!(timings[1].delay, alone(&burn).duration);

    let blow = strike(1, player(Side::Opponent));
    let timings = beat(&[blow.clone(), opponent_life(18)]);
    let hit = Conventions::get().strike().hit;
    assert!(timings[1].with);
    assert!((timings[1].delay - hit * alone(&blow).duration).abs() < 1e-9);
    // Another player's life does not wait for it.
    let yours = Act::Count {
        tally: Tally::Life {
            player: Side::You,
            life: 18,
        },
    };
    assert!(!beat(&[blow, yours])[1].with);
}

/// What follows a countered spell waits until it is gone, and the cards of a batch exile
/// leave one at a time, about 0.3 s apart (UI Conventions, "A batch exile").
#[test]
fn a_countered_spell_and_a_batch_exile_hold_up_what_follows() {
    let gone = |why, zone| vanish(object(1), why, Some(card(1, zone, Side::You)));
    let countered = alone(&gone(Why::Counter, ZoneName::Graveyard));
    assert!(countered.duration > 0.0);
    assert_eq!(countered.stagger, countered.duration);
    let exiled = alone(&gone(Why::Exile, ZoneName::Exile));
    assert!((250.0..=350.0).contains(&exiled.stagger), "{exiled:?}");
}

/// A creature's blow bursts on what it strikes and shakes the table as it lands, and a trail
/// follows it as it dashes.
#[test]
fn a_blow_bursts_on_what_it_strikes() {
    let phases = Conventions::get().strike();
    let opponent = Some(player(Side::Opponent));
    assert_eq!(
        effects(&strike(1, player(Side::Opponent))),
        [
            ("impact".to_owned(), Edge::Start, opponent, Some(phases.hit)),
            ("shake".to_owned(), Edge::Start, opponent, Some(phases.hit)),
            (
                "trail".to_owned(),
                Edge::Start,
                Some(object(1)),
                Some(phases.poise)
            ),
        ]
    );
}

/// What a streak lands on flashes as it lands, and its source glows as it leaves; a player's
/// pool is no card to glow, and mana from nowhere has no source.
#[test]
fn a_streak_glows_where_it_leaves_and_flashes_where_it_lands() {
    let names = |act: &Act| -> Vec<(String, Edge, Option<Subject>)> {
        effects(act)
            .into_iter()
            .map(|(name, when, on, _)| (name, when, on))
            .collect()
    };
    let paid = streak(Some(object(1)), object(2), mana());
    assert_eq!(
        names(&paid),
        [
            ("glow".to_owned(), Edge::Start, Some(object(1))),
            ("flash".to_owned(), Edge::End, Some(object(2))),
        ]
    );
    let burn = streak(
        Some(Subject::Ability { id: 1 }),
        player(Side::Opponent),
        Stream::Damage,
    );
    assert_eq!(
        names(&burn),
        [
            (
                "glow".to_owned(),
                Edge::Start,
                Some(Subject::Ability { id: 1 })
            ),
            ("impact".to_owned(), Edge::End, Some(player(Side::Opponent))),
        ]
    );
    for spent in [
        streak(Some(player(Side::You)), object(2), mana()),
        streak(None, object(2), mana()),
    ] {
        let names: Vec<_> = effects(&spent).into_iter().map(|e| e.0).collect();
        assert_eq!(names, ["flash"]);
    }
}

/// A land played is quicker than a spell, as Arena's (data/arena-motion.json, fly), and lands
/// with no flash, as a permanent that resolves does.
#[test]
fn a_land_played_is_quick_and_lands_with_no_flash() {
    let land = |controller| {
        fly(
            ZoneName::Hand,
            Why::Play,
            card(1, ZoneName::Battlefield, controller),
        )
    };
    let resolved = fly(
        ZoneName::Stack,
        Why::Resolve,
        card(1, ZoneName::Battlefield, Side::You),
    );
    assert_eq!(alone(&land(Side::You)).duration, 333.0);
    assert_eq!(alone(&land(Side::Opponent)).duration, 833.0);
    assert!(effects(&land(Side::You)).is_empty());
    assert!(effects(&land(Side::Opponent)).is_empty());
    let names: Vec<_> = effects(&resolved).into_iter().map(|e| e.0).collect();
    assert_eq!(names, ["settle"]);
    assert_eq!(alone(&resolved).duration, 571.0);
    assert_eq!(
        alone(&fly(
            ZoneName::Hand,
            Why::Cast,
            card(1, ZoneName::Stack, Side::You)
        ))
        .duration,
        500.0
    );
    assert_eq!(
        alone(&fly(
            ZoneName::Hand,
            Why::Cast,
            card(1, ZoneName::Stack, Side::Opponent)
        ))
        .duration,
        625.0
    );
}

/// A spell or ability is left a moment to be read, and the table longer as the game ends; a
/// card's record changing is no case.
#[test]
fn the_end_of_a_game_is_left_longer_than_a_spell_to_be_read() {
    let read = alone(&Act::Dwell { subject: object(1) });
    let ended = alone(&Act::Dwell {
        subject: player(Side::You),
    });
    assert!(read.duration > 0.0);
    assert!(ended.duration > read.duration);
    let changed = Act::Set {
        fact: Fact::Object {
            object: card(1, ZoneName::Battlefield, Side::You),
        },
    };
    assert_eq!(case_of(&changed, &Table::empty(20), &[&changed]), None);
}

/// Beginning-phase draws play at Arena's slower 0.7x speed (1429 ms), non-beginning draws play at
/// 1.0x (1000 ms), opponent draws play at 1.2x (833 ms), and batches over 6 cards compress their
/// stagger so total stagger time stays within 1500 ms.
#[test]
fn draw_timing_splits_by_phase_and_compresses_large_batch_stagger() {
    use crate::protocol::Step as TurnStep;
    let mut beginning = Table::empty(20);
    beginning.turn.step = TurnStep::Draw;
    let main = Table::empty(20);

    let drawn = fly(
        ZoneName::Library,
        Why::Draw,
        card(1, ZoneName::Hand, Side::You),
    );
    let opp_drawn = fly(
        ZoneName::Library,
        Why::Draw,
        ObjectView::hidden(ObjectId(1), ZoneName::Hand, Side::Opponent, Side::Opponent),
    );

    assert_eq!(
        timing_of(&drawn, &beginning, None, &[&drawn]).duration,
        1429.0
    );
    assert_eq!(timing_of(&drawn, &main, None, &[&drawn]).duration, 1000.0);
    assert_eq!(
        timing_of(&opp_drawn, &beginning, None, &[&opp_drawn]).duration,
        833.0
    );

    let batch_three: Vec<Act> = (1..=3)
        .map(|slot| {
            fly(
                ZoneName::Library,
                Why::Draw,
                card(slot, ZoneName::Hand, Side::You),
            )
        })
        .collect();
    let refs_three: Vec<&Act> = batch_three.iter().collect();
    assert_eq!(
        timing_of(&batch_three[0], &main, None, &refs_three).stagger,
        250.0
    );

    let batch_ten: Vec<Act> = (1..=10)
        .map(|slot| {
            fly(
                ZoneName::Library,
                Why::Draw,
                card(slot, ZoneName::Hand, Side::You),
            )
        })
        .collect();
    let refs_ten: Vec<&Act> = batch_ten.iter().collect();
    assert_eq!(
        timing_of(&batch_ten[0], &main, None, &refs_ten).stagger,
        150.0
    );
}

/// An opponent's discard takes longer than the player's own quick hop to their adjacent
/// graveyard, including in a shared-deck game where the graveyard card carries a fixed side.
#[test]
fn opponent_discard_uses_its_own_duration_including_in_shared_games() {
    let own = fly(
        ZoneName::Hand,
        Why::Discard,
        card(1, ZoneName::Graveyard, Side::You),
    );
    let opp = fly(
        ZoneName::Hand,
        Why::Discard,
        card(2, ZoneName::Graveyard, Side::Opponent),
    );
    assert_eq!(alone(&own).case.as_deref(), Some("discard"));
    assert_eq!(alone(&own).duration, 165.0);
    assert_eq!(alone(&opp).case.as_deref(), Some("opponent discard"));
    assert_eq!(alone(&opp).duration, 475.0);

    // In a shared-deck game, a card in the opponent's hand loses its owner on entering the
    // shared graveyard (`owner == Side::You`), so timing reads the hand it left from `found`.
    let mut shared = Table::empty(20);
    shared.objects.insert(
        ObjectId(3),
        ObjectView::hidden(ObjectId(3), ZoneName::Hand, Side::Opponent, Side::Opponent),
    );
    let shared_discard = fly(
        ZoneName::Hand,
        Why::Discard,
        card(3, ZoneName::Graveyard, Side::You),
    );
    let timing = timing_of(&shared_discard, &shared, None, &[&shared_discard]);
    assert_eq!(timing.case.as_deref(), Some("opponent discard"));
    assert_eq!(timing.duration, 475.0);
}
