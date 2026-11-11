//! Generated scenarios: random positions over the implemented cards, played by random choices
//! through the strict runner, for the oracles to check past what the cards' authors thought to
//! script (architecture §17.2).
//!
//! Each is a seeded turn 3 main phase, player 0 active: a few lands and permanents a side,
//! cards in hand and graveyard, and libraries of Islands to draw. Player 0 takes one to three
//! actions, each answered at random, and either player may respond to what is on the stack
//! before it resolves; a third of them then go to combat. A [`Live`] runner plays every
//! action, so each script replays as written, and the dump adds the final snapshot (§17.2)
//! for the oracles to compare. A seed whose script the runner refuses is skipped.

use beryl_engine::card::{AbilityIx, AbilityKind, Face, Subtype, Supertypes, Trigger, Types};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{DecisionKind, Step, TurnStep, Zone, ZoneKey};
use beryl_scenario::{Action, Live, P0, Permanent, Pick, PlayerSetup, Scenario};

use crate::{CARDS, names};

/// The card the dump files generated scenarios under.
pub const CARD: &str = "Generated";

/// The implemented cards outside the cube (AGENTS.md), which V1 never plays against a drafted
/// deck: the generator leaves them out.
const OUTSIDE_CUBE: [&str; 3] = [
    names::AANGS_ICEBERG,
    names::SWIFT_RECONFIGURATION,
    names::SPHINX_OF_FALSE_CONCLUSIONS,
];

/// The turn every generated scenario is played in.
const TURN: u16 = 3;

/// Up to `count` generated scenarios, from seeds `seed`, `seed + 1`, and on.
pub fn scenarios(count: usize, seed: u64) -> Vec<Scenario> {
    let pools = Pools::new();
    (0..count as u64 * 20)
        .filter_map(|i| one(&pools, seed.wrapping_add(i)))
        .take(count)
        .collect()
}

/// The cards a generated position draws from.
struct Pools {
    /// Lands, the basics three times over.
    lands: Vec<&'static str>,
    /// Permanent cards that can start on the battlefield alone: no Auras, which would need
    /// something to enchant.
    permanents: Vec<&'static str>,
    /// Every card but the basic lands.
    cards: Vec<&'static str>,
}

impl Pools {
    fn new() -> Pools {
        let mut pools = Pools {
            lands: Vec::new(),
            permanents: Vec::new(),
            cards: Vec::new(),
        };
        for def in CARDS.cards() {
            if OUTSIDE_CUBE.contains(&def.name) {
                continue;
            }
            let front = def.front();
            let basic = front.supertypes.contains(Supertypes::BASIC);
            if front.types.contains(Types::LAND) {
                let copies = if basic { 3 } else { 1 };
                pools.lands.extend(std::iter::repeat_n(def.name, copies));
            } else if !front.is_instant_or_sorcery()
                && !front.subtypes.contains(Subtype::Aura)
                && !triggers_before_main(front)
            {
                pools.permanents.push(def.name);
            }
            if !basic {
                pools.cards.push(def.name);
            }
        }
        pools
    }
}

/// Whether a face has an ability that triggers in the untap, upkeep or draw step. The oracles
/// play turn 3's beginning phase before the main phase a scenario starts in, where Beryl
/// starts there with it done, so such a trigger would go off in an oracle alone.
fn triggers_before_main(face: &Face) -> bool {
    face.abilities.iter().any(|a| {
        matches!(a.trigger, Trigger::Step(step, _)
            if (step as u8) < TurnStep::PrecombatMain as u8)
    })
}

/// What a generated script does next.
#[derive(Clone, Copy)]
enum Intent {
    /// Resolve the top of the stack, or respond to it.
    Resolve,
    /// Play a land, cast a spell or activate an ability in the main phase.
    Play,
    /// Go to combat.
    Advance,
    /// Declare attackers and blockers, and let damage happen.
    Fight,
    /// Stop at the quiet position reached.
    End,
}

/// The candidates of `d` the scenario vocabulary takes, as actions, but those naming a card
/// another candidate also names: the runner takes the first of them, and an oracle, which
/// finds a card by name too, may take another, as a Strip Mine may target itself or the
/// other Strip Mine.
fn unambiguous(live: &Live, d: &beryl_engine::Decision) -> Vec<Action> {
    let picks = live.picks(d);
    let card_of = |a: &Action| match *a {
        Action::Answer {
            pick: Pick::Card { card },
            ..
        } => Some(card),
        _ => None,
    };
    let names: Vec<&str> = (0..d.len() as u16)
        .filter_map(|c| live.name_of(d, beryl_engine::Choice(c)))
        .collect();
    picks
        .into_iter()
        .map(|(_, a)| a)
        .filter(|a| card_of(a).is_none_or(|card| names.iter().filter(|&&n| n == card).count() == 1))
        .filter(|a| match *a {
            Action::Activate {
                player,
                card,
                ability,
            } => alone(live, player, card) && !taps_a_mana_source(live, player, card, ability),
            _ => true,
        })
        .collect()
}

/// Whether `player` controls or holds one object named `card`: an oracle finds the object
/// whose ability a script activates by name, and of two may find another, as the Spara's
/// Headquarters on the battlefield rather than the one cycled from a hand, or the Strip Mine
/// it tapped for mana rather than the untapped one.
fn alone(live: &Live, player: beryl_engine::Seat, card: &str) -> bool {
    let s = live.state();
    let named = |slot| live.name(slot) == card;
    let on_battlefield = s
        .battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == player && named(slot))
        .count();
    let elsewhere: usize = [Zone::Hand, Zone::Graveyard, Zone::Exile]
        .into_iter()
        .map(|zone| {
            s.zone(ZoneKey::Of(player, zone))
                .iter()
                .filter(|&slot| named(slot))
                .count()
        })
        .sum();
    on_battlefield + elsewhere == 1
}

/// Whether the ability `ability` of the permanent named `card` that `player` controls taps a
/// permanent that also makes mana, as Strip Mine's does: an oracle pays a cost its own way, and
/// may have tapped that permanent for mana before the script activates it.
fn taps_a_mana_source(
    live: &Live,
    player: beryl_engine::Seat,
    card: &str,
    ability: AbilityIx,
) -> bool {
    let s = live.state();
    let AbilityIx::Own(i) = ability else {
        return false;
    };
    s.battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == player && live.name(slot) == card)
        .any(|&slot| {
            let face = live.table().def(s.card(slot)).face(s.object(slot).face);
            face.abilities
                .get(usize::from(i))
                .is_some_and(|a| a.cost.tap)
                && face.abilities.iter().any(|a| a.kind == AbilityKind::Mana)
        })
}

fn leak<T>(v: Vec<T>) -> &'static [T] {
    Vec::leak(v)
}

fn pick<T: Copy>(rng: &mut SplitMix64, from: &[T]) -> T {
    from[rng.below(from.len() as u64) as usize]
}

/// `n` cards drawn from `from`, repeats allowed.
fn some(rng: &mut SplitMix64, from: &[&'static str], n: u64) -> Vec<&'static str> {
    (0..n).map(|_| pick(rng, from)).collect()
}

/// One side of a generated position.
fn player(rng: &mut SplitMix64, pools: &Pools) -> PlayerSetup {
    let lands = 3 + rng.below(4);
    let mut battlefield: Vec<Permanent> = some(rng, &pools.lands, lands)
        .into_iter()
        .map(Permanent::new)
        .collect();
    // Distinct, so no legend rule applies before the oracles reach the main phase.
    let others = rng.below(3);
    let mut permanents = some(rng, &pools.permanents, others);
    permanents.sort_unstable();
    permanents.dedup();
    battlefield.extend(permanents.into_iter().map(Permanent::new));
    let held = 1 + rng.below(3);
    let mut hand = some(rng, &pools.cards, held);
    if rng.below(2) == 0 {
        hand.push(pick(rng, &pools.lands));
    }
    let buried = rng.below(3);
    let graveyard = some(rng, &pools.cards, buried);
    PlayerSetup {
        life: 20,
        hand: leak(hand),
        battlefield: leak(battlefield),
        graveyard: leak(graveyard),
        exile: &[],
        library: &[names::ISLAND; 8],
    }
}

/// The scenario of `seed`, or `None` where the runner refuses its script or it has no
/// action.
fn one(pools: &Pools, seed: u64) -> Option<Scenario> {
    let mut rng = SplitMix64::new(seed);
    let name: &'static str = String::leak(format!("Generated {seed}"));
    let mut setup = Scenario::new(name)
        .seed(seed)
        .at(TURN, P0, TurnStep::PrecombatMain);
    setup.setup.players = [player(&mut rng, pools), player(&mut rng, pools)];
    let mut live = Live::new(&CARDS, &setup).ok()?;
    let plays = 1 + rng.below(3);
    let combat = rng.below(3) == 0;
    let mut played = 0;
    let mut advanced = false;
    for _ in 0..80 {
        let s = live.state();
        let turn = *s.turn();
        if turn.number != TURN || s.outcome().is_some() {
            break;
        }
        let intent = if !s.stack().is_empty() {
            Intent::Resolve
        } else if played < plays {
            Intent::Play
        } else if combat && !advanced && (turn.step as u8) < TurnStep::BeginningOfCombat as u8 {
            Intent::Advance
        } else if advanced && (turn.step as u8) <= TurnStep::CombatDamage as u8 {
            Intent::Fight
        } else {
            Intent::End
        };
        // Where the game stops before an action depends on its kind (`Live::position`): an
        // advance or a resolution stops where passing is all a player can do, and nothing
        // else does. So the kind of action is chosen first, and an answer to a question
        // stops the same way before any of them.
        let advance = Action::AdvanceTo {
            turn: TURN,
            step: TurnStep::BeginningOfCombat,
        };
        let probe = match intent {
            Intent::Resolve => Action::Resolve,
            Intent::Advance => advance,
            Intent::Play | Intent::Fight | Intent::End => Action::pass(P0),
        };
        let Step::Decide(d) = live.position(probe).ok()? else {
            break;
        };
        // The turn as the game stopped: a step or a turn may have passed since the last
        // action, where nobody could act.
        let now = *live.state().turn();
        if now.number != TURN {
            break;
        }
        let picks = unambiguous(&live, &d);
        if d.kind != DecisionKind::Priority {
            if picks.is_empty() {
                return None;
            }
            live.act(pick(&mut rng, &picks)).ok()?;
            continue;
        }
        let options: Vec<Action> = picks
            .into_iter()
            .filter(|a| !matches!(a, Action::Answer { .. }))
            .collect();
        let past_combat = (now.step as u8) > TurnStep::CombatDamage as u8;
        // Something went on the stack as the game played on, as a trigger of what resolved:
        // passing is what this stop allows, where only a resolution could stop otherwise.
        let arrived = !live.state().stack().is_empty() && !matches!(intent, Intent::Resolve);
        let action = match intent {
            _ if arrived => Action::pass(d.player),
            Intent::Resolve if !options.is_empty() && rng.below(3) == 0 => pick(&mut rng, &options),
            Intent::Resolve => Action::Resolve,
            Intent::Play
                if d.player == P0 && now.step == TurnStep::PrecombatMain && !options.is_empty() =>
            {
                played += 1;
                pick(&mut rng, &options)
            }
            Intent::Advance => {
                advanced = true;
                advance
            }
            Intent::Fight if !past_combat => Action::pass(d.player),
            // A quiet position past what the script set out to do, or with nothing to play:
            // the end.
            Intent::Play | Intent::Fight | Intent::End => break,
        };
        live.act(action).ok()?;
    }
    let actions = live.actions().to_vec();
    if actions.is_empty() {
        return None;
    }
    let scenario = Scenario {
        actions: leak(actions),
        ..setup
    };
    beryl_scenario::run(&CARDS, &scenario).ok()?;
    Some(scenario)
}
