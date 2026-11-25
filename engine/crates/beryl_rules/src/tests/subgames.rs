//! Subgames through a spell that changes life before and after its suspended instruction.

use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_card::effect::build::wish;
use beryl_card::{Ability, CardDef, Face, Filter, Says, Types};
use beryl_kernel::{Misfit, format::Format, relayout::Relayout, rng::SplitMix64};

use crate::card::Flow;
use crate::decision::{
    Answers, Choice, Decision, DecisionKind, Opt, Outcome, Replay, StallReason, Step,
};
use crate::game::{Game, life};
use crate::ids::{LargeSlot, Seat, SlotId};
use crate::layout::{Large, Layout, Small};
use crate::resolution::Resolution;
use crate::state::{State, TurnStep, Zone};
use crate::{CardTable, CustomBody, GameTable, Profile, Scratch, apply_decided, custom_body, next};

use super::{DUAL, LAND};

struct PlaySubgame;

impl CustomBody for PlaySubgame {
    const SAYS: Says = Says {
        starts_subgame: true,
        ..Says::NOTHING
    };

    fn resolve<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow {
        life::gain(game, resolution.controller(), 1)?;
        let result = resolution.play_subgame(game)?;
        for player in game.state().players().in_seat_order() {
            if result != Outcome::Win(player) {
                life::lose(game, player, 4)?;
            }
        }
        Ok(())
    }
}

static SUBGAME: CardDef = CardDef {
    name: "Test Subgame Sorcery",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::SORCERY,
        abilities: &[Ability::spell().custom(custom_body::<PlaySubgame>())],
        ..Face::EMPTY
    }],
};

fn table(spells: u8, own_lands: u8, opponent_lands: u8) -> GameTable {
    static CARDS: CardTable = CardTable::new(&[&SUBGAME, &LAND, &DUAL]);
    let own = [(spells, SUBGAME.name), (own_lands, LAND.name)];
    let opponent = [(opponent_lands, LAND.name), (2, DUAL.name)];
    GameTable::new(Format::CONSTRUCTED, &CARDS, [&own, &opponent]).unwrap()
}

fn position(table: &GameTable, profile: Profile) -> State {
    let mut state = State::new(table, 3, profile).unwrap();
    state.board().begin_at(3, Seat::P0, TurnStep::PrecombatMain);
    let spell = state
        .library(Seat::P0)
        .iter()
        .copied()
        .find(|&slot| state.definition_name(table, slot) == SUBGAME.name)
        .unwrap();
    state.board().place(spell, Zone::Hand).unwrap();
    state
}

fn cast<L: Layout>(
    state: &mut State<L>,
    table: &GameTable,
    scratch: &mut Scratch<L>,
    answers: &mut dyn Answers<L>,
    decision: &Decision<L>,
) {
    let choice = (0..decision.len() as u16)
        .map(Choice)
        .find(|&choice| {
            matches!(decision.candidates.option(choice), Some(Opt::Cast { slot, .. })
            if state.definition_name(table, slot) == SUBGAME.name)
        })
        .expect("the spell is in hand");
    apply_decided(state, table, scratch, answers, decision, choice);
}

fn cast_from_position<L: Layout>(
    state: &mut State<L>,
    table: &GameTable,
    scratch: &mut Scratch<L>,
) {
    let Step::Decide(decision) = next(state, table, scratch, &mut Replay) else {
        panic!("priority before the spell");
    };
    cast(state, table, scratch, &mut Replay, &decision);
}

fn play_until_depth(
    state: &mut State<Large>,
    table: &GameTable,
    scratch: &mut Scratch<Large>,
    depth: usize,
) {
    for _ in 0..120 {
        let Step::Decide(decision) = next(state, table, scratch, &mut Replay) else {
            panic!("the main game should still be playing");
        };
        if state.subgame_depth() == depth {
            return;
        }
        apply_decided(state, table, scratch, &mut Replay, &decision, Choice(0));
    }
    panic!("never reached depth {depth}");
}

#[test]
fn the_small_game_requests_widening() {
    let table = table(1, 14, 3);
    let mut state = position(&table, Profile::EXACT);
    let mut scratch = Scratch::new();
    cast_from_position(&mut state, &table, &mut scratch);
    assert_eq!(
        next(&mut state, &table, &mut scratch, &mut Replay),
        Step::Over(Outcome::Stalled(StallReason::Capacity)),
    );
}

#[test]
fn the_subgame_restores_the_main_game_with_a_win_or_draw() {
    for (own_lands, final_life) in [(14, 21), (4, 17)] {
        let table = table(1, own_lands, 3);
        let mut state = position(&table, Profile::EXACT).widen();
        let original_deck = state.library(Seat::P0).to_vec();
        let mut scratch = Scratch::new();
        cast_from_position(&mut state, &table, &mut scratch);
        play_until_depth(&mut state, &table, &mut scratch, 1);
        assert_eq!(state.player(Seat::P0).life(), 20);
        assert_eq!(state.suspended_game().unwrap().player(Seat::P0).life(), 20);
        assert!(state.in_subgame());
        assert!(matches!(
            state.try_relayout::<Small>(&table),
            Err(Misfit::Subgame)
        ));
        let cloned = state.clone();
        assert_eq!(state, cloned);
        assert_eq!(state, state.relayout::<Large>());
        let hash = |state: &State<Large>| {
            let mut hasher = DefaultHasher::new();
            state.hash(&mut hasher);
            hasher.finish()
        };
        let mut rng = SplitMix64::new(19);
        let sampled =
            crate::observe(&state, &table, &mut scratch, Seat::P0).determinize(&table, &mut rng);
        assert_ne!(hash(&state), hash(&sampled));
        sampled.check_invariants(&table).unwrap();
        for &slot in &original_deck {
            assert_ne!(state.object(slot).zone(), Zone::Nowhere);
        }
        let main_spell = state.suspended_game().unwrap().stack()[0].spell().unwrap();
        assert_eq!(state.object(main_spell.slot).zone(), Zone::Nowhere);
        play_until_depth(&mut state, &table, &mut scratch, 0);
        assert_eq!(state.player(Seat::P0).life(), final_life);
        assert_eq!(state.player(Seat::P1).life(), 16);
        assert!(state.graveyard(Seat::P0).contains(&main_spell.slot));
        assert_eq!(state.library(Seat::P0).len(), original_deck.len());
    }
}

#[test]
fn the_turn_budget_counts_the_suspended_game() {
    let table = table(1, 14, 14);
    let mut profile = Profile::EXACT;
    profile.limits.turns = 3;
    let mut state = position(&table, profile).widen();
    let mut scratch = Scratch::new();
    cast_from_position(&mut state, &table, &mut scratch);
    for _ in 0..6 {
        match next(&mut state, &table, &mut scratch, &mut Replay) {
            Step::Over(outcome) => {
                assert_eq!(outcome, Outcome::Stalled(StallReason::Turns));
                assert!(!state.in_subgame());
                assert_eq!(state.player(Seat::P0).life(), 20);
                return;
            }
            Step::Decide(decision) => {
                apply_decided(
                    &mut state,
                    &table,
                    &mut scratch,
                    &mut Replay,
                    &decision,
                    Choice(0),
                );
            }
        }
    }
    panic!("the turn budget did not stop both games");
}

#[test]
fn opening_decisions_share_the_parent_turn_budget() {
    let table = table(1, 14, 14);
    let mut profile = Profile::EXACT;
    profile.limits.decisions_per_turn = 3;
    let mut state = position(&table, profile).widen();
    let mut scratch = Scratch::new();
    cast_from_position(&mut state, &table, &mut scratch);
    let Step::Decide(keep) = next(&mut state, &table, &mut scratch, &mut Replay) else {
        panic!("the budget has room for one opening decision");
    };
    assert_eq!(keep.kind, DecisionKind::KeepOrMulligan);
    assert!(state.in_subgame());
    apply_decided(
        &mut state,
        &table,
        &mut scratch,
        &mut Replay,
        &keep,
        Choice(0),
    );
    assert_eq!(
        next(&mut state, &table, &mut scratch, &mut Replay),
        Step::Over(Outcome::Stalled(StallReason::DecisionsPerTurn)),
    );
    assert!(!state.in_subgame());
}

#[test]
fn inline_answers_still_play_the_subgame() {
    struct First;
    impl Answers<Large> for First {
        fn inline(&self) -> bool {
            true
        }

        fn answer(
            &mut self,
            _: crate::Observation<'_, Large>,
            _: &Decision<Large>,
        ) -> Option<Choice> {
            Some(Choice(0))
        }
    }

    let table = table(1, 14, 3);
    let mut state = position(&table, Profile::EXACT).widen();
    let mut scratch = Scratch::new();
    let Step::Decide(decision) = next(&mut state, &table, &mut scratch, &mut First) else {
        panic!("priority before the spell");
    };
    cast(&mut state, &table, &mut scratch, &mut First, &decision);
    let Step::Decide(keep) = next(&mut state, &table, &mut scratch, &mut First) else {
        panic!("the subgame needs its opening decisions");
    };
    assert!(state.in_subgame());
    assert_eq!(keep.kind, DecisionKind::KeepOrMulligan);
}

#[test]
fn determinization_samples_both_games_from_the_same_cards() {
    let table = table(1, 14, 14);
    let mut state = position(&table, Profile::EXACT).widen();
    let hidden_hand_card = state.library(Seat::P1)[0];
    state.board().place(hidden_hand_card, Zone::Hand).unwrap();
    let mut scratch = Scratch::new();
    cast_from_position(&mut state, &table, &mut scratch);
    play_until_depth(&mut state, &table, &mut scratch, 1);

    let seen = state.library(Seat::P1)[0];
    state.board().set_known(Seat::P0, seen, true);
    let mut rng = SplitMix64::new(31);
    let mut seen_outside = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let sampled =
            crate::observe(&state, &table, &mut scratch, Seat::P0).determinize(&table, &mut rng);
        let parent = sampled.suspended_game().unwrap();
        assert_eq!(
            sampled.definition_name(&table, seen),
            state.definition_name(&table, seen),
        );
        assert_eq!(sampled.object(hidden_hand_card).zone(), Zone::Nowhere);
        assert!(!parent.knows(Seat::P0, hidden_hand_card));
        seen_outside.insert(parent.definition_name(&table, hidden_hand_card));

        let participating = parent.library(Seat::P1);
        let mut carried: Vec<_> = participating
            .iter()
            .map(|&slot| parent.definition_name(&table, slot))
            .collect();
        let mut active: Vec<_> = participating
            .iter()
            .map(|&slot| sampled.definition_name(&table, slot))
            .collect();
        carried.sort_unstable();
        active.sort_unstable();
        assert_eq!(carried, active);
    }
    assert_eq!(seen_outside.len(), 2);
}

#[test]
fn a_subgame_carries_library_cards_from_the_pool_and_offers_no_wish() {
    static WISH: CardDef = CardDef {
        name: "Test Wish",
        shape: beryl_card::definition::Shape::Single,
        tokens: &[],
        faces: &[Face {
            types: Types::SORCERY,
            abilities: &[Ability::spell().effect(&wish(Filter::Types(Types::SORCERY)))],
            ..Face::EMPTY
        }],
    };
    static CARDS: CardTable = CardTable::new(&[&SUBGAME, &LAND, &DUAL, &WISH]);
    let table = GameTable::with_sideboards(
        Format::CONSTRUCTED,
        &CARDS,
        [&[(1, SUBGAME.name), (7, LAND.name)], &[(14, LAND.name)]],
        [&[(1, WISH.name), (1, SUBGAME.name)], &[]],
    )
    .unwrap();
    let player = Seat::P0;
    let mut state = position(&table, Profile::EXACT).widen();
    let admitted = state
        .prepare_sideboard_card(&table, player, 0)
        .unwrap()
        .unwrap();
    state.board().place(admitted, Zone::Library).unwrap();
    let mut scratch = Scratch::new();
    cast_from_position(&mut state, &table, &mut scratch);
    play_until_depth(&mut state, &table, &mut scratch, 1);

    let parent = state.suspended_game().unwrap();
    assert_eq!(parent.sideboard(&table, player).count(), 1);
    assert_eq!(state.sideboard(&table, player).count(), 0);
    assert_ne!(state.object(admitted).zone(), Zone::Nowhere);
    assert_eq!(state.check_invariants(&table), Ok(()));
    let sampled = crate::observe(&state, &table, &mut scratch, Seat::P1)
        .determinize(&table, &mut SplitMix64::new(1));
    let parent = sampled.suspended_game().unwrap();
    assert!(
        parent
            .library(player)
            .iter()
            .any(|&slot| { parent.definition_name(&table, slot) == WISH.name })
    );
    assert!(LargeSlot::all().any(|slot| {
        let object = sampled.object(slot);
        object.owner() == player
            && matches!(object.zone(), Zone::Library | Zone::Hand)
            && sampled.definition_name(&table, slot) == WISH.name
    }));

    let mut wish_slot = None;
    for _ in 0..48 {
        let Step::Decide(decision) = next(&mut state, &table, &mut scratch, &mut Replay) else {
            panic!("the subgame should still be playing");
        };
        if let Some(slot) = wish_slot {
            assert_ne!(decision.kind, DecisionKind::ChooseUpTo);
            if state.object(slot).zone() == Zone::Graveyard {
                return;
            }
        }
        let mut answer = Choice(0);
        for choice in (0..decision.len() as u16).map(Choice) {
            if let Some(Opt::Cast { slot, .. }) = decision.candidates.option(choice)
                && state.definition_name(&table, slot) == WISH.name
            {
                wish_slot = Some(slot);
                answer = choice;
                break;
            }
        }
        apply_decided(
            &mut state,
            &table,
            &mut scratch,
            &mut Replay,
            &decision,
            answer,
        );
    }
    panic!("the wish should have resolved");
}
