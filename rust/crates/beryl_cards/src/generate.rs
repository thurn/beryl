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

use beryl_engine::card::{
    AbilityKind, CostAction, Face, ObjectAbility, Subtype, Supertypes, TriggerEvent, Types,
};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{DecisionKind, Step, TurnStep, Zone, ZoneKey};
use beryl_scenario::{Action, Live, P0, Permanent, Pick, PlayerSetup, Scenario};

use crate::{CARDS, names};

/// The card the dump files generated scenarios under.
pub const CARD: &str = "Generated";

/// Implemented cards the generator leaves out: those outside the WU Control deck that V1
/// plays. Cards in no deck that exist only for scenarios, like Drudge Skeletons, stay in.
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

/// One way to draw a card from a pool: the card, and a key fixed by its name and which copy of
/// it this is ([`Pool::draw`]).
type Entry = (&'static str, u64);

/// Cards to draw from, where adding a card changes few draws.
///
/// Drawing by `rng.below(len)` made every draw depend on the pool's size and order, so adding
/// one implemented card renumbered every generated scenario: the oracle exporter gaps listed
/// by scenario name went stale, and every position was new to the oracles at once. A draw
/// here takes one random number whatever the pool's size, and picks the entry whose key mixed
/// with that number is lowest (rendezvous hashing). A new card changes only the draws it
/// wins, about one in the pool's size, and the random numbers after it stay the same.
#[derive(Default)]
struct Pool(Vec<Entry>);

impl Pool {
    fn add(&mut self, name: &'static str, copies: u64) {
        let key = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        });
        self.0
            .extend((0..copies).map(|copy| (name, mix(key.wrapping_add(copy)))));
    }

    /// A card from the pool, chosen by one random number of `rng`.
    fn draw(&self, rng: &mut SplitMix64) -> &'static str {
        let r = rng.next_u64();
        self.0
            .iter()
            .min_by_key(|&&(name, key)| (mix(key ^ r), name))
            .expect("generator pools are never empty")
            .0
    }

    /// `n` cards drawn from the pool, repeats allowed.
    fn some(&self, rng: &mut SplitMix64, n: u64) -> Vec<&'static str> {
        (0..n).map(|_| self.draw(rng)).collect()
    }
}

/// `SplitMix64`'s output function, which spreads every bit of `x` over the result.
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The cards a generated position draws from.
#[derive(Default)]
struct Pools {
    /// Lands, the basics three times over.
    lands: Pool,
    /// Permanent cards that can start on the battlefield alone: no Auras, which would need
    /// something to enchant.
    permanents: Pool,
    /// Every card but the basic lands.
    cards: Pool,
}

impl Pools {
    fn new() -> Pools {
        let mut pools = Pools::default();
        for def in CARDS.cards() {
            if OUTSIDE_CUBE.contains(&def.name) {
                continue;
            }
            let front = def.front();
            let basic = front.supertypes.contains(Supertypes::BASIC);
            if front.types.contains(Types::LAND) {
                let copies = if basic { 3 } else { 1 };
                pools.lands.add(def.name, copies);
            } else if !front.is_instant_or_sorcery()
                && !front.subtypes.contains(Subtype::Aura)
                && !triggers_before_main(front)
            {
                pools.permanents.add(def.name, 1);
            }
            if !basic {
                pools.cards.add(def.name, 1);
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
        a.trigger().is_some_and(|trigger| {
            !trigger.delayed
                && matches!(trigger.event, TriggerEvent::Step(step)
                    if (step as u8) < TurnStep::PrecombatMain as u8)
        })
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
/// other Strip Mine. Where `paid_before`, mana may have been paid earlier in the script, so
/// an activation that taps a mana source is left out too ([`taps_a_mana_source`]).
pub(crate) fn unambiguous(
    live: &Live,
    d: &beryl_engine::Decision,
    paid_before: bool,
) -> Vec<Action> {
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
            } => {
                alone(live, player, card)
                    && !(paid_before && taps_a_mana_source(live, player, card, ability))
            }
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
        .map(|zone| s.zone(ZoneKey::Of(player, zone)).filter(named).count() as usize)
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
    ability: ObjectAbility,
) -> bool {
    let s = live.state();
    let ObjectAbility::Own(i) = ability else {
        return false;
    };
    s.battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == player && live.name(slot) == card)
        .any(|&slot| {
            let face = s.copiable(slot).face(live.table());
            face.abilities
                .get(i.index())
                .is_some_and(|a| a.cost().does(CostAction::Tap))
                && face.abilities.iter().any(|a| a.kind() == AbilityKind::Mana)
        })
}

/// Whether a library holds a card other than the Islands it started with that its owner
/// doesn't know the place of, as after Timetwister shuffles a hand and graveyard in. Which
/// cards a player then draws is chance, and an oracle shuffles its own way: it may draw other
/// cards, and with them have a spell to cast where Beryl had none, so the game stops for the
/// final snapshot somewhere else, as in another turn.
fn chance_in_library(live: &Live) -> bool {
    let s = live.state();
    beryl_engine::Seat::BOTH.into_iter().any(|player| {
        !s.zone(ZoneKey::Of(player, Zone::Library))
            .all(|slot| live.name(slot) == names::ISLAND || s.knows(player, slot))
    })
}

fn leak<T>(v: Vec<T>) -> &'static [T] {
    Vec::leak(v)
}

fn pick<T: Copy>(rng: &mut SplitMix64, from: &[T]) -> T {
    from[rng.below(from.len() as u64) as usize]
}

/// One side of a generated position.
fn player(rng: &mut SplitMix64, pools: &Pools) -> PlayerSetup {
    let lands = 3 + rng.below(4);
    let mut battlefield: Vec<Permanent> = pools
        .lands
        .some(rng, lands)
        .into_iter()
        .map(Permanent::new)
        .collect();
    // Distinct, so no legend rule applies before the oracles reach the main phase.
    let others = rng.below(3);
    let mut permanents = pools.permanents.some(rng, others);
    permanents.sort_unstable();
    permanents.dedup();
    battlefield.extend(permanents.into_iter().map(Permanent::new));
    let held = 1 + rng.below(3);
    let mut hand = pools.cards.some(rng, held);
    if rng.below(2) == 0 {
        hand.push(pools.lands.draw(rng));
    }
    let buried = rng.below(3);
    let graveyard = pools.cards.some(rng, buried);
    PlayerSetup {
        life: 20,
        hand: leak(hand),
        battlefield: &[],
        permanents: leak(battlefield),
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
        if chance_in_library(&live) {
            return None;
        }
        let s = live.state();
        let turn = *s.turn();
        if turn.number() != TURN || s.outcome().is_some() {
            break;
        }
        let intent = if !s.stack().is_empty() {
            Intent::Resolve
        } else if played < plays {
            Intent::Play
        } else if combat && !advanced && (turn.step() as u8) < TurnStep::BeginningOfCombat as u8 {
            Intent::Advance
        } else if advanced && (turn.step() as u8) <= TurnStep::CombatDamage as u8 {
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
        if now.number() != TURN {
            break;
        }
        let picks = unambiguous(&live, &d, true);
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
        let past_combat = (now.step() as u8) > TurnStep::CombatDamage as u8;
        // Something went on the stack as the game played on, as a trigger of what resolved:
        // passing is what this stop allows, where only a resolution could stop otherwise.
        let arrived = !live.state().stack().is_empty() && !matches!(intent, Intent::Resolve);
        let action = match intent {
            _ if arrived => Action::pass(d.player),
            Intent::Resolve if !options.is_empty() && rng.below(3) == 0 => pick(&mut rng, &options),
            Intent::Resolve => Action::Resolve,
            Intent::Play
                if d.player == P0
                    && now.step() == TurnStep::PrecombatMain
                    && !options.is_empty() =>
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
    if actions.is_empty() || chance_in_library(&live) {
        return None;
    }
    let scenario = Scenario {
        actions: leak(actions),
        ..setup
    };
    beryl_scenario::run(&CARDS, &scenario).ok()?;
    Some(scenario)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adding a card to a pool of 40 changes about one draw in 41, and leaves the random
    /// numbers after each draw as they were.
    #[test]
    fn adding_a_card_changes_few_draws() {
        let names: Vec<&'static str> = (0..41)
            .map(|i| &*String::leak(format!("card {i}")))
            .collect();
        let mut before = Pool::default();
        for &name in &names[..40] {
            before.add(name, 1);
        }
        let mut after = Pool::default();
        after.add(names[40], 1);
        for &name in &names[..40] {
            after.add(name, 1);
        }
        let (mut a, mut b) = (SplitMix64::new(7), SplitMix64::new(7));
        let changed = (0..4100)
            .filter(|_| before.draw(&mut a) != after.draw(&mut b))
            .count();
        assert!(
            (50..=150).contains(&changed),
            "{changed} of 4100 draws changed"
        );
        assert_eq!(a.next_u64(), b.next_u64());
    }
}
