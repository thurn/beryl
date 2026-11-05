//! State-based actions (architecture §8.4).

use crate::bitboard::Bitboard;
use crate::card::Supertypes;
use crate::decision::{Outcome, StallReason};
use crate::fixed::Full;
use crate::state::{Pending, State, Zone};
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
pub(crate) fn check(s: &mut State, t: &GameTable) -> bool {
    if check_losses(s) {
        return true;
    }
    if unresolved_legends(s, t, Bitboard::EMPTY).is_none() {
        s.budgets.state_based_iterations = 0;
        return false;
    }
    s.budgets.state_based_iterations += 1;
    if s.budgets.state_based_iterations > s.profile.limits.state_based_iterations {
        s.outcome = Some(Outcome::Stalled(StallReason::StateBasedIterations));
        return true;
    }
    s.pending = Pending::LegendRule {
        kept: Bitboard::EMPTY,
    };
    true
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
fn check_losses(s: &mut State) -> bool {
    let lost = s.players.map(|p| p.life <= 0 || p.drew_from_empty);
    s.outcome = match lost {
        [false, false] => return false,
        [true, true] => Some(Outcome::Draw),
        [true, false] => Some(Outcome::Win(1)),
        [false, true] => Some(Outcome::Win(0)),
    };
    true
}

fn is_legendary(s: &State, t: &GameTable, slot: u8) -> bool {
    let o = s.object(slot);
    t.def(o.card).faces[usize::from(o.face)]
        .supertypes
        .contains(Supertypes::LEGENDARY)
}

/// The legendary permanents with the same controller and name as the one in `slot`.
fn legend_group(s: &State, t: &GameTable, slot: u8) -> Bitboard {
    let o = s.object(slot);
    let name = t.def(o.card).name;
    s.battlefield
        .iter()
        .copied()
        .filter(|&other| {
            let x = s.object(other);
            x.controller == o.controller && t.def(x.card).name == name && is_legendary(s, t, other)
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
            if s.object(slot).controller != player || !is_legendary(s, t, slot) {
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
pub(crate) fn apply_legend_rule(s: &mut State, t: &GameTable, kept: Bitboard) -> Result<(), Full> {
    let mut doomed = Bitboard::EMPTY;
    for &slot in &s.battlefield {
        if is_legendary(s, t, slot) {
            let group = legend_group(s, t, slot);
            if group.count() >= 2 {
                doomed |= group & !kept;
            }
        }
    }
    for slot in doomed.iter() {
        zone::move_to(s, slot, Zone::Graveyard)?;
    }
    s.pending = Pending::None;
    Ok(())
}
