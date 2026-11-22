//! Turn structure through custom test cards and the engine's normal advancement.

use beryl_card::effect::build::{draw, gain_life, this, untap_up_to, you};
use beryl_card::{
    Ability, AbilityIx, CardDef, Face, Governed, Keywords, Players, Replacement, RuleChange, Says,
    Trigger, Types, When,
};
use beryl_kernel::format::Format;
use beryl_kernel::table::Capability;
use beryl_kernel::{Placement, TurnPhase};

use crate::card::Flow;
use crate::game::Game;
use crate::ids::Seat;
use crate::profile::{NextStop, StepStops, next_step_stop};
use crate::resolution::Resolution;
use crate::state::{Count, TurnStep, Zone};
use crate::{
    CardTable, Choice, CustomBody, GameTable, Layout, Observation, Opt, Profile, Replay, Scratch,
    State, Step, apply, custom_body, next_in_step,
};

use super::LAND;

struct Reorder;

impl CustomBody for Reorder {
    const SAYS: Says = Says {
        changes_turn_structure: true,
        ..Says::NOTHING
    };

    fn resolve<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow {
        crate::stack::delayed_triggers::create(game, resolution, AbilityIx::new(1), 0, None)?;
        game.insert_step(TurnStep::Upkeep, Placement::After, TurnStep::Draw)?;
        game.insert_step(TurnStep::Upkeep, Placement::After, TurnStep::Upkeep)?;
        game.remove_step(TurnStep::PrecombatMain);
        game.insert_phase(TurnPhase::Combat, Placement::After, TurnPhase::Combat)?;
        Ok(())
    }
}

static REORDER: CardDef = CardDef {
    name: "Test Reorder Turn",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[
            Ability::spell().custom(custom_body::<Reorder>()),
            Ability::triggered(&Trigger::step(TurnStep::Upkeep, Players::EachPlayer).next())
                .effect(&gain_life(you(), 1)),
        ],
        ..Face::EMPTY
    }],
};

static WATCH_DRAWS: CardDef = CardDef {
    name: "Test Draw Watcher",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[
            Ability::replacing(&Replacement::draws_except_first_in_draw_step(
                Players::EachOpponent,
            ))
            .effect(&draw(you(), 1)),
        ],
        ..Face::EMPTY
    }],
};

static VIGILANT_CREATURE: CardDef = CardDef {
    name: "Test Vigilant Creature",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::CREATURE,
        keywords: Keywords::VIGILANCE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
};

fn table() -> GameTable {
    static CARDS: CardTable = CardTable::new(&[&LAND, &REORDER, &WATCH_DRAWS, &VIGILANT_CREATURE]);
    let deck = [
        (35, LAND.name),
        (3, REORDER.name),
        (1, WATCH_DRAWS.name),
        (1, VIGILANT_CREATURE.name),
    ];
    GameTable::new(Format::CONSTRUCTED, &CARDS, [&deck, &deck]).unwrap()
}

#[test]
fn two_upkeeps_stop_and_the_last_insertion_plays_first() {
    let t = table();
    assert!(t.can(Capability::TurnStructure));
    let mut profile = Profile::SMART;
    let stops = StepStops::NONE.with(TurnStep::Upkeep, true, true);
    profile.step_stops[Seat::P0.index()] = stops;
    let mut s = State::new(&t, 1, profile).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::Upkeep);
    s.plan_from_current_step(t.format()).unwrap();
    let copies: Vec<_> = s
        .library(Seat::P0)
        .iter()
        .copied()
        .filter(|&slot| s.definition_name(&t, slot) == REORDER.name)
        .take(2)
        .collect();
    for &slot in &copies {
        s.board().place(slot, Zone::Hand).unwrap();
    }
    let watcher = s
        .library(Seat::P1)
        .iter()
        .copied()
        .find(|&slot| s.definition_name(&t, slot) == WATCH_DRAWS.name)
        .unwrap();
    s.board().place(watcher, Zone::Battlefield).unwrap();
    let cards_in_library = s.library(Seat::P0).len();
    let mut x = Scratch::recording();
    let first = s.step_position();
    assert!(s.current_step_is_first());
    let Some(Step::Decide(d)) = next_in_step(&mut s, &t, &mut x, &mut Replay, first) else {
        panic!("the active player stops in their first upkeep");
    };
    let cast = (0..d.len()).map(|i| Choice(u16::try_from(i).unwrap())).find(|&i| {
        matches!(d.candidates.option(i), Some(Opt::Cast { slot, .. }) if slot == copies[0])
    }).unwrap();
    apply(&mut s, &t, &mut x, &mut Replay, cast);
    while !s.stack().is_empty() {
        assert!(matches!(
            next_in_step(&mut s, &t, &mut x, &mut Replay, first),
            Some(Step::Decide(_))
        ));
        apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
    }

    assert_eq!(s.delayed().len(), 1);
    let NextStop::Step(TurnStep::Upkeep, target) =
        next_step_stop(&t, Observation::new(&s, Seat::P0), stops, None)
    else {
        panic!("the predicted stop is the additional upkeep");
    };
    assert!(s.before_step(target));
    assert!(next_in_step(&mut s, &t, &mut x, &mut Replay, first).is_none());
    assert_eq!(s.turn().step(), TurnStep::Upkeep);
    assert!(s.delayed().is_empty());
    assert!(!s.before_step(target));
    let second = s.step_position();
    assert!(!s.current_step_is_first());
    assert_eq!(second.ordinal, first.ordinal + 1);
    assert!(matches!(
        next_in_step(&mut s, &t, &mut x, &mut Replay, second),
        Some(Step::Decide(_))
    ));

    let mut sequence = vec![s.turn().step()];
    while s.turn().number() == first.turn {
        let seen = s.step_position();
        if let Some(Step::Decide(_)) = next_in_step(&mut s, &t, &mut x, &mut Replay, seen) {
            apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
        }
        if s.step_position() != seen && s.turn().number() == first.turn {
            sequence.push(s.turn().step());
            if s.turn().step() == TurnStep::Draw {
                assert_eq!(s.counted(Count::DrewInDrawStep, Seat::P0), 1);
            }
        }
    }
    assert_eq!(s.player(Seat::P0).life(), 21);
    assert_eq!(s.library(Seat::P0).len(), cards_in_library - 2);
    assert_eq!(
        sequence,
        [
            TurnStep::Upkeep,
            TurnStep::Draw,
            TurnStep::Draw,
            TurnStep::BeginningOfCombat,
            TurnStep::DeclareAttackers,
            TurnStep::EndOfCombat,
            TurnStep::BeginningOfCombat,
            TurnStep::DeclareAttackers,
            TurnStep::EndOfCombat,
            TurnStep::PrecombatMain,
            TurnStep::End,
            TurnStep::Cleanup,
        ]
    );
}

#[test]
fn a_vigilant_attacker_makes_each_combat_a_distinct_smart_stop() {
    let t = table();
    let mut s = State::new(&t, 1, Profile::SMART).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::PrecombatMain);
    s.plan_from_current_step(t.format()).unwrap();
    assert!(
        s.insert_phases_after_current(TurnPhase::Main, &[TurnPhase::Combat, TurnPhase::Main])
            .unwrap()
    );
    let slot = s
        .library(Seat::P0)
        .iter()
        .copied()
        .find(|&slot| s.definition_name(&t, slot) == VIGILANT_CREATURE.name)
        .unwrap();
    s.board().place(slot, Zone::Battlefield).unwrap();
    let creature = s.permanent_at(slot).unwrap();
    let mut x = Scratch::recording();
    crate::continuous::layers::refresh(&mut s, &t, &mut x.events).unwrap();

    let next_combat = |s: &State| {
        let NextStop::Step(TurnStep::BeginningOfCombat, target) =
            next_step_stop(&t, Observation::new(s, Seat::P0), StepStops::NONE, None)
        else {
            panic!("the next stop should be the beginning of combat");
        };
        target
    };
    let first = next_combat(&s);
    while s.before_step(first) {
        let step = s.take_planned_step().unwrap();
        let _began = s.begin_step(step);
    }
    s.declare_attacker(creature, Seat::P1);
    assert!(s.object(slot).is_attacking());
    let second = next_combat(&s);
    assert_ne!(first, second);
    assert!(s.before_step(second));
}

#[test]
fn a_full_phase_insertion_can_retry_after_widening() {
    let t = table();
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::PrecombatMain);
    s.plan_from_current_step(t.format()).unwrap();
    let (_, end) = s
        .later_steps()
        .find(|(step, _)| *step == TurnStep::End)
        .unwrap();
    assert!(
        s.insert_step(TurnStep::End, Placement::Before, TurnStep::End)
            .unwrap()
            .is_some()
    );
    let ends: Vec<_> = s
        .later_steps()
        .filter_map(|(step, target)| (step == TurnStep::End).then_some(target))
        .collect();
    assert_eq!(ends.len(), 2);
    assert_ne!(ends[0], end);
    assert_eq!(ends[1], end);
    assert!(s.remove_step(TurnStep::End).is_some());
    assert!(s.before_step(end));
    // There is room for two extra combats, with less than another combat's steps left.
    for _ in 0..2 {
        assert!(
            s.insert_phase(TurnPhase::Main, Placement::After, TurnPhase::Combat)
                .unwrap()
                .is_some()
        );
    }
    let before: Vec<_> = s.later_steps().collect();
    let phases = &[TurnPhase::Main, TurnPhase::Combat];
    assert!(
        s.insert_phases_after_current(TurnPhase::Main, phases)
            .is_err()
    );
    assert_eq!(s.later_steps().collect::<Vec<_>>(), before);
    let mut wide = s.widen();
    assert_eq!(wide.later_steps().collect::<Vec<_>>(), before);
    assert!(
        wide.insert_phases_after_current(TurnPhase::Main, phases)
            .unwrap()
    );
    let after: Vec<_> = wide.later_steps().collect();
    assert_eq!(after[0].0, TurnStep::PostcombatMain);
    assert_eq!(after[1].0, TurnStep::BeginningOfCombat);
    assert_eq!(after[7..], before);
    assert!(wide.remove_phase(TurnPhase::Combat).is_some());
    let _cut = wide.cut_to_cleanup().unwrap();
    assert_eq!(
        wide.later_steps().map(|(step, _)| step).collect::<Vec<_>>(),
        [TurnStep::Cleanup]
    );
}

#[test]
fn an_ordinary_turn_distinguishes_repeated_cleanup_steps() {
    let t = super::table([super::LANDS; 2]);
    assert!(!t.can(Capability::TurnStructure));
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::End);
    let mut x = Scratch::recording();
    crate::turn::end_step_before_actions(&mut s, &t, &mut x.events).unwrap();
    let first = s.step_position();
    crate::turn::end_step_before_actions(&mut s, &t, &mut x.events).unwrap();
    assert_eq!(s.turn().step(), TurnStep::Cleanup);
    assert_eq!(s.step_position().ordinal, first.ordinal + 1);
}

#[test]
fn skipping_the_ending_phase_advances_to_the_next_turn() {
    let t = table();
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::PostcombatMain);
    s.plan_from_current_step(t.format()).unwrap();
    assert!(s.remove_phase(TurnPhase::Ending).is_some());
    let mut x = Scratch::recording();
    crate::turn::end_step(&mut s, &t, &mut x.events).unwrap();
    assert_eq!(s.turn().number(), 4);
    assert_eq!(s.turn().step(), TurnStep::Upkeep);
    assert!(x.events.drain().all(|event| !matches!(
        event,
        crate::replacement::event::Event::StepBegins { turn: 3, .. }
    )));
}

static SKIP_UNTAP: CardDef = CardDef {
    name: "Test Skip Untap",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::rule_change(&RuleChange {
            players: Players::EachPlayer,
            what: Governed::TakeStep(TurnStep::Untap),
            when: When::Never,
        })],
        ..Face::EMPTY
    }],
};

static SKIP_UPKEEP: CardDef = CardDef {
    name: "Test Skip Upkeep",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::rule_change(&RuleChange {
            players: Players::EachPlayer,
            what: Governed::TakeStep(TurnStep::Upkeep),
            when: When::Never,
        })],
        ..Face::EMPTY
    }],
    ..SKIP_UNTAP
};

static UNTAP_ON_UPKEEP: CardDef = CardDef {
    name: "Test Untap Itself",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::triggered(&Trigger::first_step(TurnStep::Upkeep, Players::You))
                .effect(&untap_up_to(you(), 1, this())),
        ],
        ..Face::EMPTY
    }],
    ..SKIP_UNTAP
};

#[test]
fn skipping_untap_allows_an_upkeep_ability_to_untap_itself() {
    static CARDS: CardTable = CardTable::new(&[&SKIP_UNTAP, &SKIP_UPKEEP, &UNTAP_ON_UPKEEP, &LAND]);
    for (skip, next) in [
        (SKIP_UPKEEP.name, TurnStep::Draw),
        (SKIP_UNTAP.name, TurnStep::Upkeep),
    ] {
        let t = GameTable::new(
            Format::CONSTRUCTED,
            &CARDS,
            [
                &[(1, skip), (1, UNTAP_ON_UPKEEP.name), (5, LAND.name)],
                &[(7, LAND.name)],
            ],
        )
        .unwrap();
        assert!(t.can(Capability::TurnStructure));
        let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
        s.board().begin_at(2, Seat::P1, TurnStep::End);
        let find = |name| {
            s.library(Seat::P0)
                .iter()
                .copied()
                .find(|&slot| s.definition_name(&t, slot) == name)
                .unwrap()
        };
        let rule = find(skip);
        let artifact = find(UNTAP_ON_UPKEEP.name);
        s.board().place(rule, Zone::Battlefield).unwrap();
        s.board().place(artifact, Zone::Battlefield).unwrap();
        s.board().set_tapped(artifact, true);
        let mut x = Scratch::recording();

        crate::turn::begin_turn(&mut s, &t, &mut x.events).unwrap();

        assert_eq!(s.turn().step(), next);
        assert!(s.current_step_is_first());
        if next == TurnStep::Draw {
            assert!(s.triggers().is_empty());
            assert!(!s.object(artifact).is_tapped());
            continue;
        }
        assert!(s.object(artifact).is_tapped());
        assert_eq!(s.triggers().len(), 1);
        let upkeep = s.step_position();
        while s.object(artifact).is_tapped() {
            let Some(Step::Decide(d)) = next_in_step(&mut s, &t, &mut x, &mut Replay, upkeep)
            else {
                panic!("the first upkeep's trigger can untap the artifact");
            };
            let untap = (0..d.len())
                .map(|index| Choice(u16::try_from(index).unwrap()))
                .find(|&choice| d.candidates.option(choice) == Some(Opt::Object(artifact)));
            apply(&mut s, &t, &mut x, &mut Replay, untap.unwrap_or(Choice(0)));
        }
        assert_eq!(s.step_position(), upkeep);
    }
}
