//! State-based actions (architecture §8.4).

use crate::bitboard::Bitboard;
use crate::card::{Supertypes, Types};
use crate::decision::{Outcome, StallReason};
use crate::event::{self, Entry, Event, Reason, Sink};
use crate::fixed::Full;
use crate::layers;
use crate::state::{ObjectFlags, Pending, SLOTS, State, TOKEN_SLOTS, Zone};
use crate::table::GameTable;
use crate::turn::apnap;
use crate::zone;

/// Checks state-based actions and performs them, returning whether any were performed. A
/// player losing ends the game; a legend-rule choice opens [`Pending::LegendRule`], whose
/// answer performs it, and `next` then checks again.
///
/// [CR 704.3](https://yawgatog.com/resources/magic-rules/#R7043): "Whenever a player would get
/// priority (see rule 117, "Timing and Priority"), the game checks for any of the listed
/// conditions for state-based actions, then performs all applicable state-based actions
/// simultaneously as a single event. If any state-based actions are performed as a result of
/// a check, the check is repeated".
///
/// Repeated checks that keep performing actions are bounded by the profile's
/// `state_based_iterations` (architecture §8.4).
///
/// The legend rule asks its controller first, so it follows the other actions of the same
/// check, in the repeated check, rather than with them.
pub(crate) fn check(s: &mut State, t: &GameTable, sink: &mut Sink) -> Result<bool, Full> {
    if check_losses(s, sink) {
        return Ok(true);
    }
    layers::refresh(s, t);
    let (graveyard, tokens) = doomed(s);
    let legends = unresolved_legends(s, t, Bitboard::EMPTY).is_some();
    if graveyard.is_empty() && tokens.is_empty() && !legends {
        s.budgets.state_based_iterations = 0;
        return Ok(false);
    }
    s.budgets.state_based_iterations += 1;
    if s.budgets.state_based_iterations > s.profile.limits.state_based_iterations {
        s.outcome = Some(Outcome::Stalled(StallReason::StateBasedIterations));
        return Ok(true);
    }
    if graveyard.is_empty() && tokens.is_empty() {
        s.pending = Pending::LegendRule {
            kept: Bitboard::EMPTY,
        };
        return Ok(true);
    }
    for slot in tokens.iter() {
        zone::cease_to_exist(s, slot);
    }
    event::move_batch(
        s,
        t,
        sink,
        graveyard,
        Zone::Graveyard,
        Reason::StateBased,
        Entry::default(),
    )?;
    Ok(true)
}

/// The permanents state-based actions put into graveyards, and the tokens that cease to
/// exist.
///
/// [CR 704.5d](https://yawgatog.com/resources/magic-rules/#R7045d): "If a token is in a zone
/// other than the battlefield, it ceases to exist."
///
/// [CR 704.5f](https://yawgatog.com/resources/magic-rules/#R7045f): "If a creature has
/// toughness 0 or less, it's put into its owner's graveyard."
///
/// [CR 704.5g](https://yawgatog.com/resources/magic-rules/#R7045g): "If a creature has
/// toughness greater than 0, it has damage marked on it, and the total damage marked on it is
/// greater than or equal to its toughness, that creature has been dealt lethal damage and is
/// destroyed."
///
/// [CR 704.5i](https://yawgatog.com/resources/magic-rules/#R7045i): "If a planeswalker has
/// loyalty 0, it's put into its owner's graveyard."
fn doomed(s: &State) -> (Bitboard, Bitboard) {
    let mut graveyard = Bitboard::EMPTY;
    for &slot in &s.battlefield {
        let d = s.derived(slot);
        let o = s.object(slot);
        let types = Types(d.types);
        let dies = types.contains(Types::CREATURE)
            && (d.toughness <= 0 || (o.damage > 0 && i16::from(o.damage) >= i16::from(d.toughness)));
        if dies || (types.contains(Types::PLANESWALKER) && o.counters.loyalty == 0) {
            graveyard.insert(slot);
        }
    }
    let tokens = (usize::from(TOKEN_SLOTS)..SLOTS)
        .map(|slot| slot as u8)
        .filter(|&slot| {
            let o = s.object(slot);
            o.flags.has(ObjectFlags::TOKEN) && !matches!(o.zone, Zone::Battlefield | Zone::Nowhere)
        })
        .collect();
    (graveyard, tokens)
}

/// Ends the game if a player has lost.
///
/// [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0 or less
/// life, that player loses the game."
///
/// [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b): "If a player attempted to
/// draw a card from a library with no cards in it since the last time state-based actions were
/// checked, that player loses the game."
///
/// [CR 104.4a](https://yawgatog.com/resources/magic-rules/#R1044a): "If all the players
/// remaining in a game lose simultaneously, the game is a draw."
fn check_losses(s: &mut State, sink: &mut Sink) -> bool {
    let lost = s.players.map(|p| p.life <= 0 || p.drew_from_empty);
    s.outcome = match lost {
        [false, false] => return false,
        [true, true] => Some(Outcome::Draw),
        [true, false] => Some(Outcome::Win(1)),
        [false, true] => Some(Outcome::Win(0)),
    };
    for player in 0..2u8 {
        sink.push(if lost[usize::from(player)] {
            Event::Lose { player }
        } else {
            Event::Win { player }
        });
    }
    true
}

fn is_legendary(s: &State, slot: u8) -> bool {
    Supertypes(s.derived(slot).supertypes).contains(Supertypes::LEGENDARY)
}

/// The legendary permanents with the same controller and name as the one in `slot`.
fn legend_group(s: &State, t: &GameTable, slot: u8) -> Bitboard {
    let name = t.def(s.object(slot).card).name;
    let controller = s.controller(slot);
    s.battlefield
        .iter()
        .copied()
        .filter(|&other| {
            s.controller(other) == controller
                && t.def(s.object(other).card).name == name
                && is_legendary(s, other)
        })
        .collect()
}

/// The first legend-rule group, in APNAP order, with no permanent in `kept`: its controller
/// and members.
///
/// [CR 704.5j](https://yawgatog.com/resources/magic-rules/#R7045j): "If two or more legendary
/// permanents with the same name are controlled by the same player, that player chooses one of
/// them, and the rest are put into their owners' graveyards. This is called the 'legend
/// rule.'"
pub(crate) fn unresolved_legends(
    s: &State,
    t: &GameTable,
    kept: Bitboard,
) -> Option<(u8, Bitboard)> {
    for player in apnap(s) {
        for &slot in &s.battlefield {
            if s.controller(slot) != player || !is_legendary(s, slot) {
                continue;
            }
            let group = legend_group(s, t, slot);
            if group.count() >= 2 && (group & kept).is_empty() {
                return Some((player, group));
            }
        }
    }
    None
}

/// Applies the legend rule once every group has a kept permanent: the rest go to their
/// owners' graveyards, simultaneously.
pub(crate) fn apply_legend_rule(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    kept: Bitboard,
) -> Result<(), Full> {
    let mut doomed = Bitboard::EMPTY;
    for &slot in &s.battlefield {
        if is_legendary(s, slot) {
            let group = legend_group(s, t, slot);
            if group.count() >= 2 {
                doomed |= group & !kept;
            }
        }
    }
    s.pending = Pending::None;
    event::move_batch(
        s,
        t,
        sink,
        doomed,
        Zone::Graveyard,
        Reason::StateBased,
        Entry::default(),
    )
}
