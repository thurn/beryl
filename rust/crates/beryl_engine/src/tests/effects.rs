//! Effect data: what the interpreter makes of the vocabulary's instructions and expressions,
//! resolved as abilities of test permanents.

use crate::state::ZoneKey;
use beryl_card::effect::build::{
    EACH_OPPONENT, EACH_PLAYER, IF_YOU_DO, ITS_CONTROLLER, TARGET, THAT_MANY, THAT_PLAYER, THIS,
    THOSE, YOU, add_counters, controller_of, create_copy, destroy, discard, draw, exile,
    exile_until_this_leaves, for_each_player, gain_life, if_else, lose_life, may, mill, modify,
    put_into_library, remove_counters, return_to_hand, sacrifice, scry, sequence, target,
    target_player, when,
};
use beryl_card::{Amount, Comparison, Condition, Duration, Effect, LibraryPosition, Objects};

use crate::card::{
    Ability, AbilityKind, Behavior, FaceAbility, Modification, TargetKind, TargetSpec, Types,
};
use crate::decision::Ask;
use crate::effect::EffectCtx;
use crate::event::{self, Event, Reason};
use crate::filter::{Filter, Whose};
use crate::ids::{Seat, Slot};
use crate::state::{CounterKind, ObjectFlags, ObjectRef, StackEntry, Target, Zone};
use crate::{Decision, DecisionKind, GameTable, Scratch, State};

use super::events::{INSECT, game, put, with_ctx};

const CREATURES: Filter = Filter::Types(Types::CREATURE);
const A_CREATURE: TargetSpec = TargetSpec::one(TargetKind::Permanent, CREATURES);
const A_PLAYER: TargetSpec = TargetSpec::one(TargetKind::Player, Filter::Any);

/// An ability that does `effect`.
const fn does(effect: &'static Effect) -> Ability {
    Ability {
        kind: AbilityKind::Activated,
        body: Behavior::Data(effect),
        ..Ability::NONE
    }
}

/// An ability that does `effect` to `targets`.
const fn does_to(targets: &'static [TargetSpec], effect: &'static Effect) -> Ability {
    Ability {
        targets,
        ..does(effect)
    }
}

/// Resolves `ability` as one of the object `source` names, with `targets`, answering its
/// questions from `answers` in turn. Returns the question it stopped at, if it did.
fn resolve(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    source: ObjectRef,
    ability: &'static Ability,
    targets: &[Target],
    answers: &[u8],
) -> Option<Decision> {
    let mut entry = StackEntry {
        object: Some(source),
        controller: s.object(source.slot).owner,
        card: s.object(source.slot).card,
        ability: Some(FaceAbility::Own(0)),
        ..StackEntry::default()
    };
    for &target in targets {
        entry.targets.push(target).unwrap();
    }
    let legal = crate::resolve::legal_targets(s, t, &entry, ability);
    let mut ctx = EffectCtx::resolving(s, t, &mut x.events, None, answers, entry, ability);
    let flow = crate::interpret::resolve(&mut ctx, legal);
    let question = ctx.question.take();
    ctx.finish().unwrap();
    assert_eq!(flow.is_err(), question.is_some());
    question
}

/// A game with a land of `p`'s on the battlefield to be the source of abilities.
fn game_with_source() -> (GameTable, State, Scratch, Seat, ObjectRef) {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let land = put(&t, &mut s, &mut x, p, "Test Land");
    let source = s.object_ref(land);
    x.events.drain();
    (t, s, x, p, source)
}

fn life(s: &State, player: Seat) -> i16 {
    s.player(player).life
}

fn hand(s: &State, player: Seat) -> u32 {
    s.zone(ZoneKey::Of(player, Zone::Hand)).count()
}

fn zone_of(s: &State, slot: Slot) -> Zone {
    s.object(slot).zone
}

#[test]
fn that_many_is_what_the_instruction_before_did_to_every_object_at_once() {
    static SWEEP: Ability = does(&sequence(&[
        destroy(Objects::All {
            zone: Zone::Battlefield,
            filter: CREATURES,
        }),
        Effect::GainLife {
            player: YOU,
            amount: THAT_MANY,
        },
    ]));
    let (t, mut s, mut x, p, source) = game_with_source();
    let bears = [p, p.opponent()].map(|owner| put(&t, &mut s, &mut x, owner, "Test Bear"));
    x.events.drain();
    assert!(resolve(&t, &mut s, &mut x, source, &SWEEP, &[], &[]).is_none());
    assert!(
        bears
            .iter()
            .all(|&bear| zone_of(&s, bear) == Zone::Graveyard)
    );
    assert_eq!(zone_of(&s, source.slot), Zone::Battlefield);
    assert_eq!(life(&s, p), 22);
    // One batch: both creatures are gone before anything else happens.
    let events: Vec<Event> = x.events.drain().collect();
    assert!(matches!(
        events[..],
        [
            Event::ZoneChange {
                reason: Reason::Destroy,
                ..
            },
            Event::ZoneChange {
                reason: Reason::Destroy,
                ..
            },
            Event::LifeGain { amount: 2, .. }
        ]
    ));
}

#[test]
fn those_are_the_objects_the_instruction_before_moved_as_they_are_now() {
    static MILL_BACK: Ability = does(&sequence(&[mill(YOU, 2), return_to_hand(THOSE)]));
    static MILL_UNDER: Ability = does(&sequence(&[
        mill(YOU, 2),
        put_into_library(
            Objects::All {
                zone: Zone::Graveyard,
                filter: Filter::Whose(Whose::You),
            },
            LibraryPosition::Bottom,
        ),
    ]));
    let (t, s, mut x, p, source) = game_with_source();
    let top: Vec<Slot> = s.library(p).iter().rev().take(2).copied().collect();

    let mut back = s;
    assert!(resolve(&t, &mut back, &mut x, source, &MILL_BACK, &[], &[]).is_none());
    assert!(top.iter().all(|&card| zone_of(&back, card) == Zone::Hand));
    assert_eq!(hand(&back, p), hand(&s, p) + 2);

    let mut under = s;
    assert!(resolve(&t, &mut under, &mut x, source, &MILL_UNDER, &[], &[]).is_none());
    assert!(under.zone(ZoneKey::Of(p, Zone::Graveyard)).is_empty());
    assert_eq!(under.library(p).len(), s.library(p).len());
    assert!(under.library(p)[..2].iter().all(|card| top.contains(card)));
}

#[test]
fn if_you_do_asks_whether_the_instruction_before_did_anything() {
    static PAY_OFF: Ability = does(&sequence(&[
        sacrifice(THIS),
        if_else(IF_YOU_DO, &draw(YOU, 1), &lose_life(YOU, 1)),
    ]));
    let (t, s, mut x, p, source) = game_with_source();

    let mut done = s;
    assert!(resolve(&t, &mut done, &mut x, source, &PAY_OFF, &[], &[]).is_none());
    assert_eq!(zone_of(&done, source.slot), Zone::Graveyard);
    assert_eq!((hand(&done, p), life(&done, p)), (hand(&s, p) + 1, 20));

    // The source has left already, so there is nothing to sacrifice (CR 400.7).
    let mut gone = s;
    let (sink, land) = (&mut x.events, source.slot);
    event::move_one(&mut gone, &t, sink, land, Zone::Hand, Reason::Effect).unwrap();
    let before = hand(&gone, p);
    assert!(resolve(&t, &mut gone, &mut x, source, &PAY_OFF, &[], &[]).is_none());
    assert_eq!((hand(&gone, p), life(&gone, p)), (before, 19));
}

#[test]
fn each_player_takes_their_turn_at_an_effect_from_the_active_player() {
    static EACH: Ability = does(&sequence(&[
        for_each_player(
            EACH_PLAYER,
            &sequence(&[
                draw(THAT_PLAYER, 1),
                Effect::LoseLife {
                    player: THAT_PLAYER,
                    amount: THAT_MANY,
                },
            ]),
        ),
        // Every player's together: the life both lost.
        Effect::GainLife {
            player: YOU,
            amount: THAT_MANY,
        },
    ]));
    let (t, mut s, mut x, p, source) = game_with_source();
    let hands = Seat::BOTH.map(|player| hand(&s, player));
    assert!(resolve(&t, &mut s, &mut x, source, &EACH, &[], &[]).is_none());
    assert_eq!(
        Seat::BOTH.map(|player| hand(&s, player)),
        hands.map(|n| n + 1)
    );
    assert_eq!((life(&s, p), life(&s, p.opponent())), (21, 19));
    let lost: Vec<Seat> = (x.events.drain())
        .filter_map(|e| match e {
            Event::LifeLoss { player, .. } => Some(player),
            _ => None,
        })
        .collect();
    assert_eq!(lost, [s.turn().active, s.turn().active.opponent()]);
}

#[test]
fn a_choice_in_the_data_pauses_until_it_is_answered() {
    static MAY: Ability = does(&sequence(&[
        may(YOU, &draw(YOU, 1)),
        when(Condition::Not(&IF_YOU_DO), &lose_life(YOU, 2)),
    ]));
    static DISCARD: Ability = does(&discard(EACH_OPPONENT, 1));
    static SCRY: Ability = does(&scry(YOU, 2));
    let (t, s, mut x, p, source) = game_with_source();

    let question = resolve(&t, &mut { s }, &mut x, source, &MAY, &[], &[]).unwrap();
    assert_eq!((question.player, question.kind), (p, DecisionKind::YesNo));
    let mut yes = s;
    assert!(resolve(&t, &mut yes, &mut x, source, &MAY, &[], &[0]).is_none());
    assert_eq!((hand(&yes, p), life(&yes, p)), (hand(&s, p) + 1, 20));
    let mut no = s;
    assert!(resolve(&t, &mut no, &mut x, source, &MAY, &[], &[1]).is_none());
    assert_eq!((hand(&no, p), life(&no, p)), (hand(&s, p), 18));

    let them = p.opponent();
    let question = resolve(&t, &mut { s }, &mut x, source, &DISCARD, &[], &[]).unwrap();
    assert_eq!(
        (question.player, question.kind),
        (them, DecisionKind::Discard)
    );
    let mut discarded = s;
    assert!(resolve(&t, &mut discarded, &mut x, source, &DISCARD, &[], &[0]).is_none());
    assert_eq!(hand(&discarded, them), hand(&s, them) - 1);
    assert_eq!(
        discarded.zone(ZoneKey::Of(them, Zone::Graveyard)).count(),
        1
    );

    let question = resolve(&t, &mut { s }, &mut x, source, &SCRY, &[], &[]).unwrap();
    assert_eq!(question.ask, Ask::Scry { count: 2 });
    for ability in [&MAY, &DISCARD, &SCRY] {
        assert!(ability.asks());
    }
}

#[test]
fn a_target_the_effect_moved_is_known_as_it_last_was() {
    // The creature is 3/3 as it is exiled; in exile its card says 2/2 and nobody controls it.
    static SWORDS: Ability = does_to(
        &[A_CREATURE],
        &sequence(&[
            modify(TARGET, Modification::ModifyPt(1, 1), Duration::EndOfTurn),
            exile(TARGET),
            Effect::LoseLife {
                player: ITS_CONTROLLER,
                amount: Amount::Power(TARGET),
            },
        ]),
    );
    let (t, mut s, mut x, p, source) = game_with_source();
    let bear = put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    // Its owner's opponent controls it: the controller, not the owner, loses the life.
    let stolen = s.object_ref(bear);
    with_ctx(&t, &mut s, &mut x, source.slot, |ctx| {
        ctx.gain_control(stolen, crate::state::Duration::Indefinite);
    });
    let targets = [Target::object(0, s.object_ref(bear))];
    assert!(resolve(&t, &mut s, &mut x, source, &SWORDS, &targets, &[]).is_none());
    assert_eq!(zone_of(&s, bear), Zone::Exile);
    assert_eq!((life(&s, p), life(&s, p.opponent())), (17, 20));
    assert!(SWORDS.removes_target(0));
}

#[test]
fn a_target_an_instruction_could_not_move_is_read_where_it_is() {
    // The source has left, so nothing is exiled (CR 610.3b): the creature is still a 2/2 on
    // the battlefield, counted once.
    static HOLE: Ability = does_to(
        &[A_CREATURE],
        &sequence(&[
            exile_until_this_leaves(TARGET),
            Effect::GainLife {
                player: YOU,
                amount: Amount::Power(TARGET),
            },
        ]),
    );
    let (t, mut s, mut x, p, source) = game_with_source();
    let bear = put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    let targets = [Target::object(0, s.object_ref(bear))];
    let to = Zone::Graveyard;
    event::move_one(&mut s, &t, &mut x.events, source.slot, to, Reason::Effect).unwrap();
    assert!(resolve(&t, &mut s, &mut x, source, &HOLE, &targets, &[]).is_none());
    assert_eq!(zone_of(&s, bear), Zone::Battlefield);
    assert_eq!(life(&s, p), 22);
}

#[test]
fn an_illegal_target_is_not_acted_on_and_names_nobody() {
    static BOTH: Ability = does_to(
        &[A_CREATURE, A_CREATURE, A_PLAYER],
        &sequence(&[
            destroy(target(0)),
            destroy(target(1)),
            gain_life(controller_of(target(0)), 5),
            gain_life(target_player(2), 3),
        ]),
    );
    let (t, mut s, mut x, p, source) = game_with_source();
    let them = p.opponent();
    let bears = [p, them].map(|owner| put(&t, &mut s, &mut x, owner, "Test Bear"));
    let chosen = bears.map(|bear| s.object_ref(bear));
    // The first left and came back: a new object (CR 400.7), so an illegal target (CR 608.2b).
    for to in [Zone::Hand, Zone::Battlefield] {
        event::move_one(&mut s, &t, &mut x.events, bears[0], to, Reason::Effect).unwrap();
    }
    let targets = [
        Target::object(0, chosen[0]),
        Target::object(1, chosen[1]),
        Target::player(2, them),
    ];
    assert!(resolve(&t, &mut s, &mut x, source, &BOTH, &targets, &[]).is_none());
    assert_eq!(zone_of(&s, bears[0]), Zone::Battlefield);
    assert_eq!(zone_of(&s, bears[1]), Zone::Graveyard);
    assert_eq!((life(&s, p), life(&s, them)), (20, 23));
    assert!(BOTH.removes_target(0) && !BOTH.removes_target(2));
}

#[test]
fn counters_amounts_and_conditions_are_read_as_each_instruction_is_reached() {
    const STORY: Amount = Amount::Counters(THIS, CounterKind::Story);
    static COUNT_UP: Ability = does(&sequence(&[
        add_counters(THIS, CounterKind::Story, 2),
        when(
            Condition::Compare(STORY, Comparison::AtLeast, Amount::Fixed(2)),
            &Effect::CreateTokens {
                player: YOU,
                token: &INSECT,
                count: STORY,
            },
        ),
        // The tokens just created are creatures.
        when(Condition::Is(THOSE, CREATURES), &gain_life(YOU, 1)),
        // As many as it has: two, not five (CR 609.3).
        remove_counters(THIS, CounterKind::Story, 5),
        when(
            Condition::Compare(THAT_MANY, Comparison::Equal, Amount::Fixed(2)),
            &Effect::LoseLife {
                player: EACH_OPPONENT,
                amount: Amount::Times(
                    2,
                    &Amount::Count(&Objects::All {
                        zone: Zone::Battlefield,
                        filter: Filter::All(&[CREATURES, Filter::Whose(Whose::You)]),
                    }),
                ),
            },
        ),
    ]));
    let (t, mut s, mut x, p, source) = game_with_source();
    put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    assert!(resolve(&t, &mut s, &mut x, source, &COUNT_UP, &[], &[]).is_none());
    let insects = (s.battlefield().iter())
        .filter(|&&slot| s.name(&t, slot) == "Test Insect" && s.controller(slot) == p)
        .count();
    assert_eq!(insects, 2);
    assert_eq!(s.object(source.slot).counters.get(CounterKind::Story), 0);
    assert_eq!((life(&s, p), life(&s, p.opponent())), (21, 16));
    assert!(!COUNT_UP.asks());
}

#[test]
fn a_copy_of_a_target_is_a_token_with_its_copiable_values() {
    static COPY: Ability = does_to(&[A_CREATURE], &create_copy(YOU, TARGET));
    let (t, mut s, mut x, p, source) = game_with_source();
    let bear = put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    let targets = [Target::object(0, s.object_ref(bear))];
    assert!(resolve(&t, &mut s, &mut x, source, &COPY, &targets, &[]).is_none());
    let copies: Vec<Slot> = (s.battlefield().iter().copied())
        .filter(|&slot| s.object(slot).flags.has(ObjectFlags::TOKEN))
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(s.card(copies[0]), s.card(bear));
    assert_eq!(s.controller(copies[0]), p);
    assert!(!COPY.removes_target(0));
}
