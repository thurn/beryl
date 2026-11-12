//! Effect data: what the interpreter makes of the vocabulary's instructions and expressions,
//! resolved as abilities of test permanents.

use beryl_card::effect::build::{
    EACH_OPPONENT, EACH_PLAYER, IF_YOU_DO, ITS_CONTROLLER, TARGET, THAT_MANY, THAT_PLAYER, THIS,
    THOSE, YOU, add_counters, controller_of, create_copy, deal_damage_to, destroy, discard, draw,
    exile, exile_until_this_leaves, for_each_player, gain_life, if_else, if_then, lose_life, may,
    mill, modify, put_into_library, remove_counters, return_to_hand, sacrifice, scry, sequence,
    target, target_player,
};
use beryl_card::{
    Amount, Comparison, Condition, Duration, Effect, LibraryPosition, Objects, Recipients,
};

use crate::card::{
    Ability, FaceAbility, Modification, Part, Place, Players, Prompt, Purpose, SpecIx, Takes,
    TargetKind, TargetSpec, Types,
};
use crate::event::{self, Event, Reason};
use crate::filter::Filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::{Seat, Slot};
use crate::resolution::Resolution;
use crate::state::{
    CounterKind, ObjectFlags, ObjectRef, StackEntry, Target, TurnStep, Zone, ZoneKey,
};
use crate::{Decision, DecisionKind, GameTable, Scratch, State};
use beryl_kernel::two_player::TwoPlayerSeat as _;

use super::events::{INSECT, advance_to, game, put, with_game};
use crate::continuous;

const CREATURES: Filter = Filter::Types(Types::CREATURE);
const A_CREATURE: TargetSpec = TargetSpec::one(TargetKind::Permanent, CREATURES);
const A_PLAYER: TargetSpec = TargetSpec::one(TargetKind::Player, Filter::Any);

/// An ability that does `effect`.
const fn does(effect: &'static Effect) -> Ability {
    Ability::activated(beryl_card::Cost::NONE).effect(effect)
}

/// An ability that does `effect` to `targets`.
const fn does_to(targets: &'static [TargetSpec], effect: &'static Effect) -> Ability {
    does(effect).targeting(targets)
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
    let (question, ended) = run(t, s, x, source, ability, targets, answers);
    assert_eq!(ended, Ok(question.is_some()));
    question
}

/// [`resolve`], with how the resolution ended: whether it paused, or that it ran out of room.
fn run(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    source: ObjectRef,
    ability: &'static Ability,
    targets: &[Target],
    answers: &[u8],
) -> (Option<Decision>, Result<bool, Full>) {
    let mut entry = StackEntry {
        object: Some(source),
        controller: s.object(source.slot).owner(),
        card: s.copiable(source.slot).record(t).0,
        ability: Some(FaceAbility::own(0)),
        ..StackEntry::default()
    };
    for &target in targets {
        entry.targets.push(target).unwrap();
    }
    let legal = crate::resolve::legal_targets(s, t, &entry, ability);
    let mut resolution = Resolution::new(entry, ability, legal, None, answers);
    let mut game = Game::new(s, t, &mut x.events);
    let flow = crate::interpret::resolve(&mut game, &mut resolution);
    let question = resolution.question.take();
    (question, crate::interpret::finish(&mut game, flow))
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
    s.player(player).life()
}

fn hand(s: &State, player: Seat) -> u32 {
    s.zone(ZoneKey::Of(player, Zone::Hand)).count()
}

fn zone_of(s: &State, slot: Slot) -> Zone {
    s.object(slot).zone()
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
                filter: Filter::Whose(&Players::You),
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
fn attached_to_is_the_permanent_the_source_is_attached_to() {
    // "Destroy enchanted creature": the permanent the ability's source is attached to, and
    // nothing while it is attached to nothing (CR 303.4b).
    static DESTROY_ENCHANTED: Ability = does(&destroy(Objects::AttachedTo));
    let (t, mut s, mut x, p, source) = game_with_source();
    let bear = put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    assert!(resolve(&t, &mut s, &mut x, source, &DESTROY_ENCHANTED, &[], &[]).is_none());
    assert_eq!(zone_of(&s, bear), Zone::Battlefield);
    s.board().attach(source.slot, bear);
    assert!(resolve(&t, &mut s, &mut x, source, &DESTROY_ENCHANTED, &[], &[]).is_none());
    assert_eq!(zone_of(&s, bear), Zone::Graveyard);
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
    assert_eq!(lost, [s.turn().active(), s.turn().active().opponent()]);
}

#[test]
fn a_choice_in_the_data_pauses_until_it_is_answered() {
    static MAY: Ability = does(&sequence(&[
        may(YOU, &draw(YOU, 1)),
        if_then(Condition::Not(&IF_YOU_DO), &lose_life(YOU, 2)),
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
        (question.player, question.kind, question.prompt.purpose),
        (them, DecisionKind::ChooseObject, Purpose::Discard)
    );
    let mut discarded = s;
    assert!(resolve(&t, &mut discarded, &mut x, source, &DISCARD, &[], &[0]).is_none());
    assert_eq!(hand(&discarded, them), hand(&s, them) - 1);
    assert_eq!(
        discarded.zone(ZoneKey::Of(them, Zone::Graveyard)).count(),
        1
    );

    let question = resolve(&t, &mut { s }, &mut x, source, &SCRY, &[], &[]).unwrap();
    // The first pick of a scry: a card for the bottom, of the two looked at.
    assert_eq!(
        question.prompt,
        Prompt::new(Purpose::Scry)
            .count(2)
            .most(2)
            .places(Place::Bottom, Place::Top)
            .part(Part::First)
    );
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
    with_game(&t, &mut s, &mut x, source.slot, |game, resolution| {
        continuous::gain_control(game, resolution, stolen, crate::state::Duration::Indefinite)
            .unwrap();
    });
    let targets = [Target::object(SpecIx::new(0), s.object_ref(bear))];
    assert!(resolve(&t, &mut s, &mut x, source, &SWORDS, &targets, &[]).is_none());
    assert_eq!(zone_of(&s, bear), Zone::Exile);
    assert_eq!((life(&s, p), life(&s, p.opponent())), (17, 20));
    assert!(SWORDS.takes_target(SpecIx::new(0), Takes::Removal));
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
    let targets = [Target::object(SpecIx::new(0), s.object_ref(bear))];
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
        Target::object(SpecIx::new(0), chosen[0]),
        Target::object(SpecIx::new(1), chosen[1]),
        Target::player(SpecIx::new(2), them),
    ];
    assert!(resolve(&t, &mut s, &mut x, source, &BOTH, &targets, &[]).is_none());
    assert_eq!(zone_of(&s, bears[0]), Zone::Battlefield);
    assert_eq!(zone_of(&s, bears[1]), Zone::Graveyard);
    assert_eq!((life(&s, p), life(&s, them)), (20, 23));
    assert!(
        BOTH.takes_target(SpecIx::new(0), Takes::Removal)
            && !BOTH.takes_target(SpecIx::new(2), Takes::Removal)
    );
}

#[test]
fn counters_amounts_and_conditions_are_read_as_each_instruction_is_reached() {
    const STORY: Amount = Amount::Counters(THIS, CounterKind::Story);
    static COUNT_UP: Ability = does(&sequence(&[
        add_counters(THIS, CounterKind::Story, 2),
        if_then(
            Condition::Compare(STORY, Comparison::AtLeast, Amount::Fixed(2)),
            &Effect::CreateTokens {
                player: YOU,
                token: &INSECT,
                count: STORY,
            },
        ),
        // The tokens just created are creatures.
        if_then(Condition::Is(THOSE, CREATURES), &gain_life(YOU, 1)),
        // As many as it has: two, not five (CR 609.3).
        remove_counters(THIS, CounterKind::Story, 5),
        if_then(
            Condition::Compare(THAT_MANY, Comparison::Equal, Amount::Fixed(2)),
            &Effect::LoseLife {
                player: EACH_OPPONENT,
                amount: Amount::Times(
                    2,
                    &Amount::Count(&Objects::All {
                        zone: Zone::Battlefield,
                        filter: Filter::All(&[CREATURES, Filter::Whose(&Players::You)]),
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
    assert_eq!(s.counters_of(source.slot, CounterKind::Story), 0);
    assert_eq!((life(&s, p), life(&s, p.opponent())), (21, 16));
    assert!(!COUNT_UP.asks());
}

/// "An opponent has more than you": the most any opponent has of an amount, each of them
/// read in the controller's place, against the controller's own. A life total is an amount
/// ([CR 119.1](https://yawgatog.com/resources/magic-rules/#R1191)), and a tie is not more.
#[test]
fn the_most_any_player_has_is_read_with_each_in_your_place() {
    const LIFE: Amount = Amount::Life(YOU);
    const YOUR_CREATURES: Amount = Amount::Count(&Objects::All {
        zone: Zone::Battlefield,
        filter: Filter::All(&[CREATURES, Filter::Whose(&Players::You)]),
    });
    const fn an_opponent_has_more(amount: &'static Amount) -> Condition {
        Condition::Compare(
            Amount::Most(EACH_OPPONENT, amount),
            Comparison::MoreThan,
            *amount,
        )
    }
    static CATCH_UP: Ability = does(&sequence(&[
        // Life 20 each: no opponent has more.
        if_then(an_opponent_has_more(&LIFE), &draw(YOU, 1)),
        deal_damage_to(YOU, 3),
        // 20 against 17.
        if_then(an_opponent_has_more(&LIFE), &gain_life(YOU, 1)),
        // Their one creature against none.
        if_then(
            an_opponent_has_more(&YOUR_CREATURES),
            &Effect::CreateTokens {
                player: YOU,
                token: &INSECT,
                count: Amount::Fixed(1),
            },
        ),
        // One each now.
        if_then(an_opponent_has_more(&YOUR_CREATURES), &draw(YOU, 1)),
    ]));
    let (t, mut s, mut x, p, source) = game_with_source();
    put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    let cards = hand(&s, p);
    assert!(resolve(&t, &mut s, &mut x, source, &CATCH_UP, &[], &[]).is_none());
    assert_eq!((life(&s, p), life(&s, p.opponent())), (18, 20));
    assert_eq!(hand(&s, p), cards);
    let insects = (s.battlefield().iter())
        .filter(|&&slot| s.name(&t, slot) == "Test Insect")
        .count();
    assert_eq!(insects, 1);
}

/// Damage an effect deals to permanents is marked on each
/// ([CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e)): "Damage dealt to a
/// creature by a source with neither wither nor infect causes that much damage to be marked
/// on that creature." What was dealt damage is what the next instruction reads, and a card
/// is no token ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)).
#[test]
fn damage_to_permanents_is_marked_on_each_and_a_token_is_told_from_a_card() {
    static COPY: Ability = does_to(&[A_CREATURE], &create_copy(YOU, TARGET));
    static SINGE: Ability = does(&sequence(&[
        Effect::DealDamage {
            to: Recipients::Permanents(Objects::All {
                zone: Zone::Battlefield,
                filter: CREATURES,
            }),
            amount: Amount::Fixed(1),
        },
        // One of the two dealt damage is a card, so they are not all tokens: each is copied.
        if_else(
            Condition::Token(THOSE),
            &lose_life(YOU, 5),
            &create_copy(YOU, THOSE),
        ),
        // The copies are.
        if_then(Condition::Token(THOSE), &gain_life(YOU, 1)),
    ]));
    let (t, mut s, mut x, p, source) = game_with_source();
    let bear = put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    let targets = [Target::object(SpecIx::new(0), s.object_ref(bear))];
    assert!(resolve(&t, &mut s, &mut x, source, &COPY, &targets, &[]).is_none());
    assert!(resolve(&t, &mut s, &mut x, source, &SINGE, &[], &[]).is_none());
    let bears = || (s.battlefield().iter()).filter(|&&slot| s.name(&t, slot) == "Test Bear");
    // The bear, its first copy, and the two copies made after the damage.
    assert_eq!(bears().count(), 4);
    let damaged = bears().filter(|&&slot| s.object(slot).damage() == 1);
    assert_eq!(damaged.count(), 2);
    assert_eq!(life(&s, p), 21);
}

/// "Gain control of" with no duration lasts
/// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a): "If no duration is
/// stated, it lasts until the end of the game"), and the permanent comes under its new
/// controller's control anew
/// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
#[test]
fn gaining_control_as_data_changes_the_controller_and_says_so() {
    static STEAL: Ability = does_to(
        &[A_CREATURE],
        &modify(TARGET, Modification::Control, Duration::Indefinite),
    );
    let (t, mut s, mut x, p, source) = game_with_source();
    let bear = put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    s.set_summoning_sick(bear, false);
    let targets = [Target::object(SpecIx::new(0), s.object_ref(bear))];
    assert!(resolve(&t, &mut s, &mut x, source, &STEAL, &targets, &[]).is_none());
    assert_eq!(s.controller(bear), p);
    assert!(s.object(bear).flags().has(ObjectFlags::SUMMONING_SICK));
    let said = x.events.drain().into_iter().any(|event| {
        matches!(event, Event::GainControl { object, player } if object.slot == bear && player == p)
    });
    assert!(said);
}

#[test]
fn an_instruction_out_of_room_stops_the_resolution_at_once() {
    // `Small` has 135 slots for tokens, so 200 do not fit. The library's function says so,
    // the instruction passes it on, and nothing after it is done: the game stalls.
    static SWARM: Ability = does(&sequence(&[
        Effect::CreateTokens {
            player: YOU,
            token: &INSECT,
            count: Amount::Fixed(200),
        },
        gain_life(YOU, 1),
    ]));
    let (t, mut s, mut x, p, source) = game_with_source();
    let (question, ended) = run(&t, &mut s, &mut x, source, &SWARM, &[], &[]);
    assert_eq!((question, ended), (None, Err(Full)));
    assert_eq!(life(&s, p), 20);
}

#[test]
fn a_copy_of_a_target_is_a_token_with_its_copiable_values() {
    static COPY: Ability = does_to(&[A_CREATURE], &create_copy(YOU, TARGET));
    let (t, mut s, mut x, p, source) = game_with_source();
    let bear = put(&t, &mut s, &mut x, p.opponent(), "Test Bear");
    let targets = [Target::object(SpecIx::new(0), s.object_ref(bear))];
    assert!(resolve(&t, &mut s, &mut x, source, &COPY, &targets, &[]).is_none());
    let copies: Vec<Slot> = (s.battlefield().iter().copied())
        .filter(|&slot| s.object(slot).flags().has(ObjectFlags::TOKEN))
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(s.copiable(copies[0]), s.copiable(bear));
    assert_eq!(s.card_behind(copies[0]), None);
    assert_eq!(s.controller(copies[0]), p);
    assert!(!COPY.takes_target(SpecIx::new(0), Takes::Removal));
}

/// The permanent's power and toughness after continuous effects, as they are now.
fn size(t: &GameTable, s: &mut State, x: &mut Scratch, slot: Slot) -> (i8, i8) {
    crate::layers::refresh(s, t, &mut x.events).unwrap();
    (s.derived(slot).power, s.derived(slot).toughness)
}

/// "Creatures get +1/+1": one effect on every creature there as it begins.
const fn creatures_get_bigger(duration: Duration) -> Ability {
    const ALL_CREATURES: Objects = Objects::All {
        zone: Zone::Battlefield,
        filter: CREATURES,
    };
    static UNTIL_END_OF_TURN: Effect = modify(
        ALL_CREATURES,
        Modification::ModifyPt(1, 1),
        Duration::EndOfTurn,
    );
    static INDEFINITELY: Effect = modify(
        ALL_CREATURES,
        Modification::ModifyPt(1, 1),
        Duration::Indefinite,
    );
    does(match duration {
        Duration::EndOfTurn => &UNTIL_END_OF_TURN,
        _ => &INDEFINITELY,
    })
}

/// [CR 611.2c](https://yawgatog.com/resources/magic-rules/#R6112c): "the set of objects it
/// affects is determined when that continuous effect begins. After that point, the set won't
/// change." [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that
/// moves from one zone to another becomes a new object with no memory of, or relation to, its
/// previous existence."
#[test]
fn an_effect_on_several_permanents_is_one_effect_on_those_that_were_there() {
    static PUMP: Ability = creatures_get_bigger(Duration::EndOfTurn);
    let (t, mut s, mut x, p, source) = game_with_source();
    let q = p.opponent();
    let [stays, leaves] = [p, q].map(|owner| put(&t, &mut s, &mut x, owner, "Test Bear"));
    let before = s.effects().len();
    assert!(resolve(&t, &mut s, &mut x, source, &PUMP, &[], &[]).is_none());

    // One effect for both, whoever controls them, and none for what is no creature.
    assert_eq!(s.effects().len(), before + 1);
    assert_eq!(size(&t, &mut s, &mut x, stays), (3, 3));
    assert_eq!(size(&t, &mut s, &mut x, leaves), (3, 3));

    // A creature that arrives afterwards is no member of the set.
    let late = put(&t, &mut s, &mut x, p, "Test Bear");
    assert_eq!(size(&t, &mut s, &mut x, late), (2, 2));

    // One that leaves and comes back is a new object: the effect is still one effect, on
    // the other alone.
    for zone in [Zone::Hand, Zone::Battlefield] {
        event::move_one(&mut s, &t, &mut x.events, leaves, zone, Reason::Effect).unwrap();
    }
    assert_eq!(size(&t, &mut s, &mut x, leaves), (2, 2));
    assert_eq!(size(&t, &mut s, &mut x, stays), (3, 3));
    assert_eq!(s.effects().len(), before + 1);
    assert_eq!(s.check_invariants(&t), Ok(()));

    // It ends with the turn.
    advance_to(&t, &mut s, &mut x, q, TurnStep::PrecombatMain);
    assert!(s.effects().is_empty());
    assert_eq!(size(&t, &mut s, &mut x, stays), (2, 2));
}

#[test]
fn an_effect_on_several_permanents_ends_when_the_last_of_them_has_left() {
    static PUMP: Ability = creatures_get_bigger(Duration::Indefinite);
    let (t, mut s, mut x, p, source) = game_with_source();
    let q = p.opponent();
    let bears = [p, q, q].map(|owner| put(&t, &mut s, &mut x, owner, "Test Bear"));
    assert!(resolve(&t, &mut s, &mut x, source, &PUMP, &[], &[]).is_none());
    // It outlasts the turn, and each member but the last.
    advance_to(&t, &mut s, &mut x, q, TurnStep::PrecombatMain);
    for &bear in &bears[..2] {
        event::move_one(
            &mut s,
            &t,
            &mut x.events,
            bear,
            Zone::Graveyard,
            Reason::Effect,
        )
        .unwrap();
        assert_eq!(s.effects().len(), 1);
        assert_eq!(size(&t, &mut s, &mut x, bears[2]), (3, 3));
    }
    event::move_one(
        &mut s,
        &t,
        &mut x.events,
        bears[2],
        Zone::Exile,
        Reason::Effect,
    )
    .unwrap();
    assert!(s.effects().is_empty());
    assert_eq!(s.check_invariants(&t), Ok(()));
}
