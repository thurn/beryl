//! Casting, activation, timing permissions, the stack and resolution, through `next` and
//! `apply` on positions set up with the cards of [`super::cast`].

use super::cast::{Game, cast, index_of};
use crate::card::{Castings, FaceIx, ManaColor, ObjectAbility};
use crate::ids::Slot;
use crate::state::{CounterKind, ManaPool, ObjectFlags, Pending, TurnStep, Zone, ZoneKey};
use crate::zone::Place;
use crate::{Choice, DecisionKind, Opt, Profile};

#[test]
fn a_spell_its_caster_cannot_pay_for_is_never_offered() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    assert!(!g.offers_cast(bear), "{{1}}{{W}} with one land");
    g.give(g.me, "Test Island", Zone::Battlefield);
    assert!(g.offers_cast(bear));
}

#[test]
fn a_spell_with_no_legal_target_is_never_offered() {
    let mut g = Game::new(Profile::EXACT);
    let removal = g.give(g.me, "Test Removal", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    assert!(!g.offers_cast(removal), "no creature to target");
    g.give(g.them, "Test Bear", Zone::Battlefield);
    assert!(g.offers_cast(removal));
}

#[test]
fn a_creature_spell_goes_on_the_stack_then_resolves_onto_the_battlefield() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let lands = [
        g.give(g.me, "Test Land", Zone::Battlefield),
        g.give(g.me, "Test Island", Zone::Battlefield),
    ];
    g.pick(cast(bear));
    // CR 601.2a: on the stack under its caster's control; CR 601.2i: they get priority.
    assert_eq!(g.s.object(bear).zone, Zone::Stack);
    assert_eq!(g.s.stack().len(), 1);
    for land in lands {
        assert!(g.s.object(land).flags.has(ObjectFlags::TAPPED));
    }
    assert_eq!(g.s.player(g.me).mana, ManaPool::default());
    assert_eq!(g.s.turn().priority, g.me);
    g.pass_both();
    assert_eq!(g.s.object(bear).zone, Zone::Battlefield);
    assert_eq!(g.s.controller(bear), g.me);
    assert!(g.s.stack().is_empty());
    // CR 117.3b: the active player receives priority after a resolution.
    assert_eq!(g.s.turn().priority, g.me);
}

#[test]
fn sorcery_speed_spells_wait_for_an_empty_stack_in_their_casters_main_phase() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let flasher = g.give(g.me, "Test Flasher", Zone::Hand);
    for _ in 0..4 {
        g.give(g.me, "Test Island", Zone::Battlefield);
    }
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.at(g.me, TurnStep::Upkeep);
    assert!(!g.offers_cast(bear));
    // CR 702.8a: flash lets it be cast any time its caster could cast an instant.
    assert!(g.offers_cast(flasher));
    g.main(g.me);
    assert!(g.offers_cast(bear));
    g.pick(cast(flasher));
    assert!(!g.offers_cast(bear), "the stack is not empty");
}

#[test]
fn a_spell_whose_targets_all_became_illegal_does_not_resolve() {
    let mut g = Game::new(Profile::EXACT);
    let removal = g.give(g.me, "Test Removal", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    g.pick(cast(removal));
    // The only creature is chosen silently.
    assert_eq!(
        g.s.stack()[0].targets.as_slice()[0]
            .as_object()
            .unwrap()
            .slot,
        bear
    );
    // The bear leaves and comes back: a new object (CR 400.7), so an illegal target.
    g.s.move_object(&mut crate::event::Sink::off(), bear, Zone::Hand, Place::Top)
        .unwrap();
    g.s.move_object(
        &mut crate::event::Sink::off(),
        bear,
        Zone::Battlefield,
        Place::Top,
    )
    .unwrap();
    g.pass_both();
    assert_eq!(g.s.object(bear).zone, Zone::Battlefield, "unaffected");
    assert_eq!(g.s.object(removal).zone, Zone::Graveyard);
}

#[test]
fn a_target_is_chosen_among_the_legal_ones() {
    let mut g = Game::new(Profile::EXACT);
    let removal = g.give(g.me, "Test Removal", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let mine = g.give(g.me, "Test Bear", Zone::Battlefield);
    let theirs = g.give(g.them, "Test Bear", Zone::Battlefield);
    g.pick(cast(removal));
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::Target);
    // Two bears with different controllers are different targets.
    assert_eq!(d.len(), 2);
    g.pick_object(theirs);
    g.pass_both();
    assert_eq!(g.s.object(theirs).zone, Zone::Graveyard);
    assert_eq!(g.s.object(mine).zone, Zone::Battlefield);
}

/// A game whose profile asks `asks` for a lone target of their own cast
/// ([`Profile::asks_targets`]).
fn asking(asks: impl FnOnce(&Game) -> Option<crate::ids::Seat>) -> Game {
    let mut g = Game::new(Profile::EXACT);
    let asks_targets = asks(&g);
    g.s.set_profile(Profile {
        asks_targets,
        ..Profile::EXACT
    });
    g
}

/// Answers the open decision with `opt` and plays on, under the profile alone, to the next.
fn answer(g: &mut Game, opt: Opt) -> crate::Decision {
    let d = g.decide();
    let c = index_of(&d, opt).unwrap_or_else(|| panic!("{opt:?} not offered in {d:?}"));
    crate::apply(&mut g.s, &g.t, &mut g.x, &mut crate::Replay, c);
    g.decide()
}

#[test]
fn a_lone_target_of_a_cast_is_asked_only_of_the_player_the_profile_names() {
    for named in [false, true] {
        let mut g = asking(|g| named.then_some(g.me));
        let removal = g.give(g.me, "Test Removal", Zone::Hand);
        g.give(g.me, "Test Land", Zone::Battlefield);
        g.give(g.me, "Test Land", Zone::Battlefield);
        let mine = g.give(g.me, "Test Bear", Zone::Battlefield);
        let d = answer(&mut g, cast(removal));
        if named {
            // CR 601.2c: the player announces the target, here the only one, their own bear.
            assert_eq!((d.kind, d.player, d.len()), (DecisionKind::Target, g.me, 1));
            assert_eq!(super::cast::object_index(&d, mine), Some(Choice(0)));
        } else {
            // Announced silently, and resolved as both players can only pass.
            assert_ne!(d.kind, DecisionKind::Target);
            assert_eq!(g.s.object(mine).zone, Zone::Graveyard);
        }
    }
    // Naming the other player asks this one nothing.
    let mut g = asking(|g| Some(g.them));
    let removal = g.give(g.me, "Test Removal", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let mine = g.give(g.me, "Test Bear", Zone::Battlefield);
    assert_ne!(answer(&mut g, cast(removal)).kind, DecisionKind::Target);
    assert_eq!(g.s.object(mine).zone, Zone::Graveyard);
}

#[test]
fn a_lone_target_of_a_trigger_is_chosen_silently_even_for_the_player_the_profile_names() {
    let mut g = asking(|g| Some(g.me));
    let stinger = g.give(g.me, "Test Stinger", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    g.pick(cast(stinger));
    g.pass_both();
    assert_eq!(g.s.object(stinger).zone, Zone::Battlefield);
    // CR 603.3d: the trigger goes on the stack aimed at the one opposing creature, unasked,
    // and resolves as both players can only pass.
    assert_ne!(g.decide().kind, DecisionKind::Target);
    assert_eq!(g.s.object(bear).zone, Zone::Graveyard);
}

#[test]
fn counter_unless_pays_asks_the_spells_controller_only_when_they_can_pay() {
    for can_pay in [false, true] {
        let mut g = Game::new(Profile::EXACT);
        let bear = g.give(g.them, "Test Bear", Zone::Hand);
        let tithe = g.give(g.me, "Test Tithe", Zone::Hand);
        g.give(g.me, "Test Land", Zone::Battlefield);
        g.give(g.them, "Test Land", Zone::Battlefield);
        g.give(g.them, "Test Island", Zone::Battlefield);
        if can_pay {
            g.give(g.them, "Test Island", Zone::Battlefield);
        }
        g.main(g.them);
        g.pick(cast(bear));
        // Their priority after casting; they pass, then the Tithe's caster responds.
        g.choose(Choice(0));
        g.pick(cast(tithe));
        g.pass_both();
        if can_pay {
            let d = g.decide();
            assert_eq!((d.kind, d.player), (DecisionKind::YesNo, g.them));
            assert!(g.is_paused());
            // The committed state is still the one before the Tithe resolved.
            assert_eq!(g.s.object(tithe).zone, Zone::Stack);
            g.pick(Opt::Yes);
            assert_eq!(g.s.object(bear).zone, Zone::Stack, "paid, so not countered");
            g.pass_both();
            assert_eq!(g.s.object(bear).zone, Zone::Battlefield);
        } else {
            // No question: they cannot pay.
            assert_eq!(g.s.object(bear).zone, Zone::Graveyard, "countered");
        }
        assert_eq!(g.s.object(tithe).zone, Zone::Graveyard);
    }
}

#[test]
fn a_spell_countered_by_an_exiling_counter_is_exiled() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.them, "Test Bear", Zone::Hand);
    let lies = g.give(g.me, "Test Lies", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.them, "Test Land", Zone::Battlefield);
    g.give(g.them, "Test Land", Zone::Battlefield);
    g.main(g.them);
    g.pick(cast(bear));
    g.choose(Choice(0));
    g.pick(cast(lies));
    g.pass_both();
    assert_eq!(g.s.object(bear).zone, Zone::Exile);
    assert_eq!(g.s.object(lies).zone, Zone::Graveyard);
}

#[test]
fn a_spell_is_an_illegal_target_for_itself() {
    let mut g = Game::new(Profile::EXACT);
    let lies = g.give(g.me, "Test Lies", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    assert!(!g.offers_cast(lies), "no other spell to target");
}

#[test]
fn kicker_x_announces_an_x_of_at_least_one_and_marks_the_permanent() {
    let mut g = Game::new(Profile::EXACT);
    let diver = g.give(g.me, "Test Diver", Zone::Hand);
    for _ in 0..4 {
        g.give(g.me, "Test Island", Zone::Battlefield);
    }
    let kicked = Opt::Cast {
        slot: diver,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };
    let d = g.current();
    assert!(index_of(&d, cast(diver)).is_some() && index_of(&d, kicked).is_some());
    g.pick(kicked);
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::X);
    // {1}{U} plus X from four Islands: X is 1 or 2, never 0.
    let xs: Vec<_> = (0..d.len())
        .map(|i| d.candidates.option(Choice(i as u16)).unwrap())
        .collect();
    assert_eq!(xs, [Opt::Number(1), Opt::Number(2)]);
    g.pick(Opt::Number(2));
    g.pass_both();
    let o = g.s.object(diver);
    assert_eq!(o.zone, Zone::Battlefield);
    assert!(o.flags.has(ObjectFlags::KICKED));
    assert_eq!(g.s.x_of(diver), 2);
}

#[test]
fn offspring_is_an_additional_cost_offered_only_when_payable() {
    let mut g = Game::new(Profile::EXACT);
    let trainer = g.give(g.me, "Test Trainer", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    let with_offspring = Opt::Cast {
        slot: trainer,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };
    let d = g.current();
    assert!(index_of(&d, cast(trainer)).is_some());
    assert!(index_of(&d, with_offspring).is_none());
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.pick(with_offspring);
    g.pass_both();
    assert!(g.s.object(trainer).flags.has(ObjectFlags::OFFSPRING));
}

#[test]
fn impending_is_an_alternative_cost_that_enters_with_time_counters() {
    let mut g = Game::new(Profile::EXACT);
    let overlord = g.give(g.me, "Test Overlord", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let impending = Opt::Cast {
        slot: overlord,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };
    let d = g.current();
    assert!(
        index_of(&d, cast(overlord)).is_none(),
        "{{3}}{{W}} is too much"
    );
    g.pick(impending);
    g.pass_both();
    let o = g.s.object(overlord);
    assert_eq!(o.zone, Zone::Battlefield);
    assert!(o.flags.has(ObjectFlags::IMPENDING));
    assert_eq!(o.counters.long_tail(), Some((CounterKind::Time, 4)));
}

#[test]
fn a_modal_double_faced_card_is_cast_by_its_spell_face_or_played_by_its_land_face() {
    let mut g = Game::new(Profile::EXACT);
    let stupor = g.give(g.me, "Test Stupor", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    let d = g.current();
    assert!(index_of(&d, cast(stupor)).is_some());
    assert!(index_of(&d, Opt::PlayLand(stupor)).is_some());
    assert!(
        index_of(
            &d,
            Opt::Cast {
                slot: stupor,
                face: FaceIx::BACK,
                costs: Castings::NONE
            }
        )
        .is_none(),
        "CR 305.9: a land face can't be cast"
    );
    g.pick(Opt::PlayLand(stupor));
    assert_eq!(g.s.object(stupor).zone, Zone::Battlefield);
    assert_eq!(g.s.object(stupor).face, FaceIx::BACK);
    // Its land face is a source of blue: with the two Islands, {U}{U}{U}.
    let uuu = crate::card::ManaCost::new(0, [0, 3, 0, 0, 0, 0]);
    assert!(crate::mana::payable(&g.s, &g.t, g.me, uuu, crate::Bitboard::EMPTY).unwrap());
    assert_eq!(g.s.object(bear).zone, Zone::Battlefield);
}

#[test]
fn loyalty_abilities_are_sorcery_speed_and_once_a_turn() {
    let mut g = Game::new(Profile::EXACT);
    let raveler = g.give(g.me, "Test Raveler", Zone::Battlefield);
    g.s.board().set_counters(raveler, CounterKind::Loyalty, 2);
    let plus = Opt::Activate {
        slot: raveler,
        ability: ObjectAbility::own(1),
    };
    let minus = Opt::Activate {
        slot: raveler,
        ability: ObjectAbility::own(2),
    };
    let d = g.current();
    assert!(index_of(&d, plus).is_some());
    // CR 606.6: -3 needs three loyalty counters.
    assert!(index_of(&d, minus).is_none());
    g.at(g.me, TurnStep::Upkeep);
    let d = g.current();
    assert!(index_of(&d, plus).is_none(), "not at instant speed");
    g.main(g.me);
    g.pick(plus);
    // CR 606.4: the counter is the cost, paid on activation.
    assert_eq!(g.s.object(raveler).counters.loyalty, 3);
    g.pass_both();
    let d = g.current();
    assert!(index_of(&d, plus).is_none(), "once a turn");
    assert!(index_of(&d, minus).is_none(), "once a turn");
}

#[test]
fn a_permission_lets_loyalty_abilities_be_activated_at_instant_speed_the_turn_it_entered() {
    let mut g = Game::new(Profile::EXACT);
    let emperor = g.give(g.me, "Test Emperor", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    // Flashed in on the opponent's end step.
    g.at(g.them, TurnStep::End);
    g.choose(Choice(0));
    g.pick(cast(emperor));
    g.pass_both();
    assert_eq!(g.s.object(emperor).zone, Zone::Battlefield);
    assert_eq!(g.s.object(emperor).counters.loyalty, 3, "CR 306.5b");
    let plus = Opt::Activate {
        slot: emperor,
        ability: ObjectAbility::own(1),
    };
    // The active player gets priority after the resolution and passes.
    g.choose(Choice(0));
    g.pick(plus);
    assert_eq!(g.s.object(emperor).counters.loyalty, 4);
    g.pass_both();
    // The turn it did not enter, it is sorcery speed again.
    g.s.board().set_entered_this_turn(emperor, false);
    g.s.board().set_loyalty_activated(emperor, false);
    g.at(g.them, TurnStep::End);
    let d = g.current();
    assert!(index_of(&d, plus).is_none());
}

#[test]
fn a_static_ability_limits_an_opponent_to_sorcery_speed() {
    let mut g = Game::new(Profile::EXACT);
    let flasher = g.give(g.them, "Test Flasher", Zone::Hand);
    // An instant-speed action keeps their priority a decision.
    g.give(g.them, "Test HQ", Zone::Hand);
    g.give(g.them, "Test Island", Zone::Battlefield);
    g.give(g.them, "Test Island", Zone::Battlefield);
    g.at(g.me, TurnStep::Upkeep);
    g.choose(Choice(0));
    assert!(g.offers_cast(flasher));
    g.give(g.me, "Test Raveler", Zone::Battlefield);
    g.at(g.me, TurnStep::Upkeep);
    g.choose(Choice(0));
    assert!(!g.offers_cast(flasher), "flash, but only at sorcery speed");
    g.main(g.them);
    assert!(g.offers_cast(flasher));
}

#[test]
fn sorceries_can_be_cast_as_though_they_had_flash_until_your_next_turn() {
    let mut g = Game::new(Profile::EXACT);
    let raveler = g.give(g.me, "Test Raveler", Zone::Battlefield);
    let ponder = g.give(g.me, "Test Ponder", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.pick(Opt::Activate {
        slot: raveler,
        ability: ObjectAbility::own(1),
    });
    g.pass_both();
    g.at(g.them, TurnStep::End);
    g.choose(Choice(0));
    assert!(g.offers_cast(ponder));
    crate::turn::begin_turn(&mut g.s, &g.t, &mut g.x.events).unwrap();
    if g.s.turn().active != g.me {
        crate::turn::begin_turn(&mut g.s, &g.t, &mut g.x.events).unwrap();
    }
    g.at(g.me, TurnStep::Upkeep);
    assert!(
        !g.offers_cast(ponder),
        "the permission ended as my turn began"
    );
}

#[test]
fn cycling_discards_from_hand_as_a_cost_and_draws_on_resolution() {
    let mut g = Game::new(Profile::EXACT);
    let hq = g.give(g.me, "Test HQ", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.at(g.me, TurnStep::Upkeep);
    g.pick(Opt::Activate {
        slot: hq,
        ability: ObjectAbility::own(1),
    });
    assert_eq!(g.s.object(hq).zone, Zone::Graveyard);
    assert_eq!(g.s.zone(ZoneKey::Of(g.me, Zone::Hand)).count(), 0);
    g.pass_both();
    assert_eq!(g.s.zone(ZoneKey::Of(g.me, Zone::Hand)).count(), 1);
}

#[test]
fn an_ability_whose_source_was_sacrificed_still_resolves() {
    let mut g = Game::new(Profile::EXACT);
    let strip = g.give(g.me, "Test Strip", Zone::Battlefield);
    let land = g.give(g.them, "Test Land", Zone::Battlefield);
    g.pick(Opt::Activate {
        slot: strip,
        ability: ObjectAbility::own(1),
    });
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::Target, "its own land is a target too");
    g.pick_object(land);
    assert_eq!(
        g.s.object(strip).zone,
        Zone::Graveyard,
        "sacrificed as a cost"
    );
    g.pass_both();
    assert_eq!(g.s.object(land).zone, Zone::Graveyard);
}

#[test]
fn a_card_can_be_cast_from_a_graveyard_this_turn_and_is_then_exiled() {
    let mut g = Game::new(Profile::EXACT);
    let flashback = g.give(g.me, "Test Flashback", Zone::Hand);
    let ponder = g.give(g.me, "Test Ponder", Zone::Graveyard);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    assert!(!g.offers_cast(ponder));
    g.pick(cast(flashback));
    g.pass_both();
    g.on_top(g.me, "Test Bear");
    g.on_top(g.me, "Test Removal");
    g.pick(cast(ponder));
    g.pass_both();
    // Ponder's put-back.
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::ChooseObject);
    g.choose(Choice(0));
    assert_eq!(g.s.object(ponder).zone, Zone::Exile);
}

/// [CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a): "If the flashback cost
/// was paid, exile this card instead of putting it anywhere else any time it would leave the
/// stack." Returned to its owner's hand, it is exiled instead.
#[test]
fn a_spell_cast_with_flashback_is_exiled_wherever_else_it_would_go() {
    let mut g = Game::new(Profile::EXACT);
    let snapback = g.give(g.me, "Test Snapback", Zone::Hand);
    let ponder = g.give(g.me, "Test Ponder", Zone::Graveyard);
    let bounce = g.give(g.them, "Test Bounce", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.them, "Test Island", Zone::Battlefield);
    assert!(!g.offers_cast(ponder));
    g.pick(cast(snapback));
    g.pass_both();
    g.pick(cast(ponder));
    assert_eq!(g.s.object(ponder).zone, Zone::Stack);
    // P0 can only pass, which `next` does silently: P1 holds priority.
    g.pick(cast(bounce));
    g.pass_both();
    assert_eq!(g.s.object(ponder).zone, Zone::Exile);
    assert_eq!(g.s.object(bounce).zone, Zone::Graveyard);
}

#[test]
fn a_permission_from_exile_costs_its_increase_more() {
    let mut g = Game::new(Profile::EXACT);
    let binder = g.give(g.me, "Test Binder", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let bear = g.give(g.them, "Test Bear", Zone::Hand);
    g.pick(cast(binder));
    g.pass_both();
    g.pick(Opt::Yes);
    assert_eq!(g.s.object(bear).zone, Zone::Exile);
    g.main(g.them);
    g.give(g.them, "Test Land", Zone::Battlefield);
    g.give(g.them, "Test Land", Zone::Battlefield);
    g.give(g.them, "Test Land", Zone::Battlefield);
    assert!(!g.offers_cast(bear), "{{1}}{{W}} plus {{2}}");
    g.give(g.them, "Test Land", Zone::Battlefield);
    g.pick(cast(bear));
    g.pass_both();
    assert_eq!(g.s.object(bear).zone, Zone::Battlefield);
}

#[test]
fn a_resolution_may_offer_a_free_cast_with_no_priority_in_between() {
    let mut g = Game::new(Profile::EXACT);
    let looter = g.give(g.me, "Test Looter", Zone::Hand);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.at(g.me, TurnStep::Upkeep);
    g.pick(cast(looter));
    g.pass_both();
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::CastOffer);
    g.pick(cast(bear));
    assert_eq!(
        g.s.object(bear).zone,
        Zone::Stack,
        "cast for free, at upkeep"
    );
    assert_eq!(g.s.object(looter).zone, Zone::Graveyard);
    g.pass_both();
    assert_eq!(g.s.object(bear).zone, Zone::Battlefield);
}

#[test]
fn a_paused_resolution_leaves_the_committed_state_unchanged_and_view_replays_it() {
    let mut g = Game::new(Profile::EXACT);
    let ponder = g.give(g.me, "Test Ponder", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.on_top(g.me, "Test Bear");
    g.on_top(g.me, "Test Removal");
    g.pick(cast(ponder));
    g.pass_both();
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::ChooseObject);
    assert!(g.is_paused());
    assert_eq!(
        g.s.zone(ZoneKey::Of(g.me, Zone::Hand)).count(),
        0,
        "nothing drawn yet"
    );
    let view = crate::view(&g.s, &g.t, &mut g.x);
    assert_eq!(
        view.zone(ZoneKey::Of(g.me, Zone::Hand)).count(),
        2,
        "the two cards drawn"
    );
    let candidate = d.candidates.object(Choice(0)).unwrap();
    assert_eq!(view.object(candidate).zone, Zone::Hand);
    g.choose(Choice(0));
    assert!(matches!(g.s.pending(), Pending::None));
    assert_eq!(g.s.zone(ZoneKey::Of(g.me, Zone::Hand)).count(), 1);
    assert_eq!(g.s.library(g.me).last(), Some(&candidate));
}

#[test]
fn a_mana_ability_taps_for_the_color_chosen() {
    let mut g = Game::new(Profile {
        payment: crate::Payment::Manual,
        ..Profile::EXACT
    });
    // A priority with only mana to make passes silently but under full control.
    g.s.set_profile(Profile {
        full_control: Some(g.me),
        ..g.s.profile()
    });
    let island = g.give(g.me, "Test Island", Zone::Battlefield);
    g.pick(Opt::Mana {
        slot: island,
        color: ManaColor::Blue,
    });
    assert_eq!(g.s.player(g.me).mana.amounts(), [0, 1, 0, 0, 0, 0]);
    assert!(g.s.object(island).flags.has(ObjectFlags::TAPPED));
}

#[test]
fn a_triggered_ability_chooses_its_targets_as_it_goes_on_the_stack() {
    for creatures in [0, 2] {
        let mut g = Game::new(Profile::EXACT);
        let stinger = g.give(g.me, "Test Stinger", Zone::Hand);
        g.give(g.me, "Test Land", Zone::Battlefield);
        let bears: Vec<Slot> = (0..creatures)
            .map(|_| g.give(g.them, "Test Bear", Zone::Battlefield))
            .collect();
        // Two bears with different tapped states are two targets.
        if let Some(&first) = bears.first() {
            g.s.board().set_tapped(first, true);
        }
        g.pick(cast(stinger));
        g.pass_both();
        assert_eq!(g.s.object(stinger).zone, Zone::Battlefield);
        if creatures == 0 {
            // CR 603.3d: no legal target, so it is removed from the stack.
            g.decide();
            assert!(g.s.stack().is_empty());
            continue;
        }
        let d = g.decide();
        assert_eq!((d.kind, d.player, d.len()), (DecisionKind::Target, g.me, 2));
        g.pick_object(bears[1]);
        assert_eq!(g.s.stack().len(), 1);
        g.pass_both();
        assert_eq!(g.s.object(bears[1]).zone, Zone::Graveyard);
        assert_eq!(g.s.object(bears[0]).zone, Zone::Battlefield);
    }
}

#[test]
fn a_paused_body_answers_each_question_with_the_card_first_chosen() {
    // A hand of copies some known to the opponent and some not: what a run shows after one
    // question (a discard, to both players) must not change that question's candidates when
    // the body runs again, or its recorded index names another card.
    const DECK: &crate::Decklist = &[(20, "Test Land"), (2, "Test Bear"), (1, "Test Rummage")];
    let mut g = Game::of(DECK, Profile::EXACT);
    let mut lands: Vec<Slot> =
        g.s.library(g.me)
            .iter()
            .copied()
            .filter(|&slot| g.t.def(g.s.object(slot).card).name == "Test Land")
            .collect();
    lands.sort_unstable();
    let [seen, mine, drawn] = [lands[0], lands[1], lands[2]];
    for slot in [seen, mine] {
        g.s.move_object(&mut crate::event::Sink::off(), slot, Zone::Hand, Place::Top)
            .unwrap();
    }
    g.s.move_object(
        &mut crate::event::Sink::off(),
        drawn,
        Zone::Library,
        Place::Top,
    )
    .unwrap();
    g.s.board().set_known(g.them, seen, true);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let rummage = g.give(g.me, "Test Rummage", Zone::Hand);
    assert!(!g.s.knows(g.them, mine) && !g.s.knows(g.them, drawn));
    g.pick(cast(rummage));
    g.pass_both();
    assert!(g.is_paused());
    // The land only I know and the one I drew are one candidate, offered as the first.
    g.pick_object(mine);
    assert!(g.is_paused());
    g.pick_object(bear);
    assert!(!g.is_paused());
    let graveyard = g.s.zone(ZoneKey::Of(g.me, Zone::Graveyard));
    assert!(graveyard.contains(mine) && graveyard.contains(bear));
    let hand = g.s.zone(ZoneKey::Of(g.me, Zone::Hand));
    assert!(hand.contains(seen) && hand.contains(drawn));
}
