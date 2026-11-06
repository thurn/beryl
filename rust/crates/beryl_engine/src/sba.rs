//! State-based actions (architecture §8.4).

use crate::bitboard::Bitboard;
use crate::card::{Subtype, Supertypes, Types};
use crate::decision::{Outcome, StallReason};
use crate::event::{self, Entry, Event, Reason, Sink};
use crate::fixed::Full;
use crate::ids::{Seat, Slot};
use crate::layers;
use crate::state::{ObjectFlags, Pending, State, TOKEN_POOL, Zone};
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
    layers::refresh(s, t, sink);
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
///
/// [CR 704.5m](https://yawgatog.com/resources/magic-rules/#R7045m): "If an Aura is attached to
/// an illegal object or player, or is not attached to an object or player, that Aura is put
/// into its owner's graveyard." The cube's one Aura, Swift Reconfiguration, makes what it
/// enchants a Vehicle, which its "enchant creature or Vehicle" allows, so only the second
/// case arises: what it enchanted left the battlefield (`zone::leave` detaches it).
fn doomed(s: &State) -> (Bitboard, Bitboard) {
    let mut graveyard = Bitboard::EMPTY;
    for &slot in &s.battlefield {
        let d = s.derived(slot);
        let o = s.object(slot);
        let types = d.types;
        let dies = types.contains(Types::CREATURE)
            && (d.toughness <= 0
                || (o.damage > 0 && i16::from(o.damage) >= i16::from(d.toughness)));
        let unattached = o.attached_to.is_none() && d.subtypes.contains(Subtype::Aura);
        if dies || unattached || (types.contains(Types::PLANESWALKER) && o.counters.loyalty == 0) {
            graveyard.insert(slot);
        }
    }
    // The zones' memberships hold everything off the battlefield and in a zone, so the
    // token slots need no scan.
    let mut away = s.zones.stack;
    for player in Seat::BOTH {
        for zone in [Zone::Library, Zone::Hand, Zone::Graveyard, Zone::Exile] {
            away |= s.zone(player, zone);
        }
    }
    let tokens = (away & TOKEN_POOL)
        .iter()
        .filter(|&slot| s.object(slot).flags.has(ObjectFlags::TOKEN))
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
    if lost == [false, false] {
        return false;
    }
    end(s, sink, lost);
    true
}

/// Ends the game as `loser` loses it, as their opponent wins it by an effect
/// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
/// that a player wins the game." In a two-player game that is one ending
/// ([CR 104.2a](https://yawgatog.com/resources/magic-rules/#R1042a)): "A player still in the
/// game wins the game if that player's opponents have all left the game."
pub(crate) fn end_game(s: &mut State, sink: &mut Sink, loser: Seat) {
    let mut lost = [false; 2];
    lost[loser.index()] = true;
    end(s, sink, lost);
}

fn end(s: &mut State, sink: &mut Sink, lost: [bool; 2]) {
    s.outcome = Some(match lost {
        [true, false] => Outcome::Win(Seat::P1),
        [false, true] => Outcome::Win(Seat::P0),
        _ => Outcome::Draw,
    });
    for player in Seat::BOTH {
        sink.push(if lost[player.index()] {
            Event::Lose { player }
        } else {
            Event::Win { player }
        });
    }
}

fn is_legendary(s: &State, slot: Slot) -> bool {
    s.derived(slot).supertypes.contains(Supertypes::LEGENDARY)
}

/// The name of the permanent in `slot`: its face up's (CR 712.8d, 712.8e).
fn name(s: &State, t: &GameTable, slot: Slot) -> &'static str {
    let o = s.object(slot);
    t.def(o.card).face_name(o.face)
}

/// The legendary permanents each player controls.
fn legendary(s: &State) -> [Bitboard; 2] {
    let mut by_controller = [Bitboard::EMPTY; 2];
    for &slot in &s.battlefield {
        if is_legendary(s, slot) {
            by_controller[s.controller(slot).index()].insert(slot);
        }
    }
    by_controller
}

/// The legendary permanents with the same controller and name as the one in `slot`, of
/// `legendary`, each player's.
fn legend_group(s: &State, t: &GameTable, legendary: &[Bitboard; 2], slot: Slot) -> Bitboard {
    let name_of = name(s, t, slot);
    legendary[s.controller(slot).index()]
        .iter()
        .filter(|&other| name(s, t, other) == name_of)
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
) -> Option<(Seat, Bitboard)> {
    let legendary = legendary(s);
    // A group needs two legendary permanents of one controller, which most checks lack.
    if legendary.iter().all(|b| b.count() < 2) {
        return None;
    }
    for player in apnap(s) {
        let mut seen = Bitboard::EMPTY;
        for &slot in &s.battlefield {
            if !legendary[player.index()].contains(slot) || seen.contains(slot) {
                continue;
            }
            let group = legend_group(s, t, &legendary, slot);
            if group.count() >= 2 && (group & kept).is_empty() {
                return Some((player, group));
            }
            seen |= group;
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
    let legendary = legendary(s);
    let mut doomed = Bitboard::EMPTY;
    for slot in (legendary[0] | legendary[1]).iter() {
        let group = legend_group(s, t, &legendary, slot);
        if group.count() >= 2 {
            doomed |= group & !kept;
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
