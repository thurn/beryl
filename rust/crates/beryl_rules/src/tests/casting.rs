//! Casting, activation, timing permissions, the stack and resolution, through `next` and
//! `apply` on positions set up with the cards of [`super::cast`].

use super::cast::{Game, PONDER, REMOVAL, SORCERIES_AS_FLASH, cast, index_of};
use super::events::with_game;
use crate::card::{Castings, FaceIx, ManaColor, SpecIx, Takes};
use crate::continuous;
use crate::ids::Slot;
use crate::replacement::event::{self, Reason};
use crate::state::{Duration, Pending, TurnStep, Zone, ZoneKey};
use crate::zone::Place;
use crate::{Choice, DecisionKind, Opt, Profile};

/// Nothing reads code, so a Rust body says of itself what effect data would
/// (`CustomBody::SAYS`): the removal spell here is one, and so is the spell that draws and
/// puts a card back.
#[test]
fn a_rust_body_says_what_it_asks_and_what_it_takes() {
    let removal = &REMOVAL.front().abilities[0];
    assert!(removal.takes_target(SpecIx::new(0), Takes::Removal));
    assert!(!removal.takes_target(SpecIx::new(0), Takes::Counter));
    assert!(!removal.asks());
    assert!(PONDER.front().abilities[0].asks());
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
    g.s.move_object(
        &mut crate::replacement::event::Sink::off(),
        bear,
        Zone::Hand,
        Place::Top,
    )
    .unwrap();
    g.s.move_object(
        &mut crate::replacement::event::Sink::off(),
        bear,
        Zone::Battlefield,
        Place::Top,
    )
    .unwrap();
    g.pass_both();
    assert_eq!(g.s.object(bear).zone(), Zone::Battlefield, "unaffected");
    assert_eq!(g.s.object(removal).zone(), Zone::Graveyard);
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
            assert_eq!(g.s.object(mine).zone(), Zone::Graveyard);
        }
    }
    // Naming the other player asks this one nothing.
    let mut g = asking(|g| Some(g.them));
    let removal = g.give(g.me, "Test Removal", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let mine = g.give(g.me, "Test Bear", Zone::Battlefield);
    assert_ne!(answer(&mut g, cast(removal)).kind, DecisionKind::Target);
    assert_eq!(g.s.object(mine).zone(), Zone::Graveyard);
}

#[test]
fn a_lone_target_of_a_trigger_is_chosen_silently_even_for_the_player_the_profile_names() {
    let mut g = asking(|g| Some(g.me));
    let stinger = g.give(g.me, "Test Stinger", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    g.pick(cast(stinger));
    g.pass_both();
    assert_eq!(g.s.object(stinger).zone(), Zone::Battlefield);
    // CR 603.3d: the trigger goes on the stack aimed at the one opposing creature, unasked,
    // and resolves as both players can only pass.
    assert_ne!(g.decide().kind, DecisionKind::Target);
    assert_eq!(g.s.object(bear).zone(), Zone::Graveyard);
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
            assert_eq!(g.s.object(tithe).zone(), Zone::Stack);
            g.pick(Opt::Yes);
            assert_eq!(
                g.s.object(bear).zone(),
                Zone::Stack,
                "paid, so not countered"
            );
            g.pass_both();
            assert_eq!(g.s.object(bear).zone(), Zone::Battlefield);
        } else {
            // No question: they cannot pay.
            assert_eq!(g.s.object(bear).zone(), Zone::Graveyard, "countered");
        }
        assert_eq!(g.s.object(tithe).zone(), Zone::Graveyard);
    }
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
    assert_eq!(g.s.object(stupor).zone(), Zone::Battlefield);
    assert_eq!(g.s.object(stupor).face(), FaceIx::BACK);
    // Its land face is a source of blue: with the two Islands, {U}{U}{U}.
    let uuu = crate::card::ManaCost::new(0, [0, 3, 0, 0, 0, 0]);
    assert!(
        crate::mana::payable(
            &g.s,
            &g.t,
            g.me,
            uuu,
            crate::Bitboard::EMPTY,
            crate::mana::Paying::NOTHING
        )
        .unwrap()
    );
    assert_eq!(g.s.object(bear).zone(), Zone::Battlefield);
}

/// "Each opponent" is read for the permanent's controller after continuous effects
/// ([CR 109.5](https://yawgatog.com/resources/magic-rules/#R1095)): "For a static ability,
/// this is the current controller of the object it's on." The limit is found with the
/// layers, so it moves as control does and ends as the permanent leaves.
#[test]
fn a_static_rule_change_follows_its_permanents_controller_and_ends_with_it() {
    let mut g = Game::new(Profile::EXACT);
    let flasher = g.give(g.me, "Test Flasher", Zone::Hand);
    // An instant-speed action keeps my priority a decision.
    g.give(g.me, "Test HQ", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    let raveler = g.give(g.me, "Test Raveler", Zone::Battlefield);
    let thief = g.give(g.them, "Test Bear", Zone::Battlefield);
    g.at(g.me, TurnStep::Upkeep);
    assert!(g.offers_cast(flasher), "mine limits my opponents, not me");
    // A control effect and no change to the object: layer 2 says whose opponents.
    let stolen = g.s.object_ref(raveler);
    with_game(&g.t, &mut g.s, &mut g.x, thief, |game, resolution| {
        continuous::gain_control(game, resolution, stolen, Duration::EndOfTurn).unwrap();
    });
    assert_eq!(g.s.base_controller(raveler), g.me);
    assert!(!g.offers_cast(flasher), "under their control it limits me");
    // CR 708.2: face down it has no abilities, whoever controls it.
    g.s.board().set_face(raveler, FaceIx::DOWN);
    assert!(
        g.offers_cast(flasher),
        "a face-down permanent limits nobody"
    );
    g.s.board().set_face(raveler, FaceIx::FRONT);
    assert!(!g.offers_cast(flasher));
    // The control effect ends, and the limit is my opponents' again.
    g.s.expire(Duration::EndOfTurn);
    assert!(g.offers_cast(flasher), "mine again");
    g.s.board().set_controller(raveler, g.them);
    assert!(!g.offers_cast(flasher));
    let graveyard = Zone::Graveyard;
    event::move_one(
        &mut g.s,
        &g.t,
        &mut g.x.events,
        raveler,
        graveyard,
        Reason::Effect,
    )
    .unwrap();
    g.at(g.me, TurnStep::Upkeep);
    assert!(g.offers_cast(flasher), "the limit left with it");
}

/// A player who may cast sorceries as though they had flash and whom another effect limits
/// to sorcery speed casts them at sorcery speed
/// ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)): "When a rule or effect
/// allows or directs something to happen, and another effect states that it can't happen,
/// the "can't" effect takes precedence."
#[test]
fn a_limit_to_sorcery_speed_beats_a_permission_to_cast_as_though_with_flash() {
    let mut g = Game::new(Profile::EXACT);
    let ponder = g.give(g.me, "Test Ponder", Zone::Hand);
    let island = g.give(g.me, "Test Island", Zone::Battlefield);
    let me = g.me;
    g.current();
    with_game(&g.t, &mut g.s, &mut g.x, island, |game, resolution| {
        continuous::change_rule(
            game,
            resolution,
            &SORCERIES_AS_FLASH,
            Duration::UntilYourNextTurn(me),
        )
        .unwrap();
    });
    g.at(g.me, TurnStep::Upkeep);
    assert!(g.offers_cast(ponder), "as though it had flash");
    g.give(g.them, "Test Raveler", Zone::Battlefield);
    assert!(!g.offers_cast(ponder), "can't beats can");
    g.main(g.me);
    assert!(g.offers_cast(ponder), "at sorcery speed still");
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
    assert_eq!(view.object(candidate).zone(), Zone::Hand);
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
    assert_eq!(g.s.player(g.me).mana().amounts(), [0, 1, 0, 0, 0, 0]);
    assert!(g.s.object(island).is_tapped());
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
            .filter(|&slot| g.s.definition_name(&g.t, slot) == "Test Land")
            .collect();
    lands.sort_unstable();
    let [seen, mine, drawn] = [lands[0], lands[1], lands[2]];
    for slot in [seen, mine] {
        g.s.move_object(
            &mut crate::replacement::event::Sink::off(),
            slot,
            Zone::Hand,
            Place::Top,
        )
        .unwrap();
    }
    g.s.move_object(
        &mut crate::replacement::event::Sink::off(),
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
