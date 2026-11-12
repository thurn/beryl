//! State-based actions (architecture §8.4).

use crate::bitboard::Bitboard;
use crate::card::{Subtype, Supertypes, Types};
use crate::decision::{Outcome, StallReason};
use crate::event::{self, Entry, Event, LossCause, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Layout, PerPlayer, Table};
use crate::state::{ObjectFlags, Pending, State, Zone};
use crate::table::GameTable;

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
pub(crate) fn check<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<bool, Full> {
    if check_losses(s, sink) {
        return Ok(true);
    }
    layers::refresh(s, t, sink)?;
    let (graveyard, tokens, legendary) = doomed(s);
    let legends = legend_group_in(s, t, &legendary, Bitboard::EMPTY).is_some();
    if graveyard.is_empty() && tokens.is_empty() && !legends {
        s.reset_state_based_checks();
        return Ok(false);
    }
    if s.count_state_based_check() > s.profile().limits.state_based_iterations {
        s.end_game(Outcome::Stalled(StallReason::StateBasedIterations));
        return Ok(true);
    }
    if graveyard.is_empty() && tokens.is_empty() {
        s.ask(Pending::LegendRule {
            kept: Bitboard::EMPTY,
        });
        return Ok(true);
    }
    // Each token ceases to exist on its own, with no event, so the order is nothing.
    for slot in s.in_candidate_order(tokens) {
        s.cease_to_exist(slot);
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

/// What [`doomed`] finds: the permanents bound for graveyards, the tokens that cease to exist,
/// and each player's legendary permanents.
type Doomed<L> = (
    Bitboard<<L as Layout>::Slot>,
    Bitboard<<L as Layout>::Slot>,
    Legendary<L>,
);

/// The legendary permanents each player controls.
type Legendary<L> = PerPlayer<L, Bitboard<<L as Layout>::Slot>>;

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
///
/// With them, in the same pass, the legendary permanents each player controls, for the legend
/// rule.
fn doomed<L: Layout>(s: &State<L>) -> Doomed<L> {
    let mut graveyard = Bitboard::EMPTY;
    let mut legendary: Legendary<L> = Table::filled(Bitboard::EMPTY);
    for (slot, d) in s.permanents() {
        if d.supertypes.contains(Supertypes::LEGENDARY) {
            legendary[d.controller.index()].insert(slot);
        }
        let o = s.object(slot);
        let types = d.types;
        let dies = types.contains(Types::CREATURE)
            && (d.toughness <= 0
                || (o.damage() > 0 && i16::from(o.damage()) >= i16::from(d.toughness)));
        let unattached = o.attached_to().is_none() && d.subtypes.contains(Subtype::Aura);
        if dies || unattached || (types.contains(Types::PLANESWALKER) && o.counters().loyalty == 0)
        {
            graveyard.insert(slot);
        }
    }
    // The zones' memberships hold everything off the battlefield and in a zone, so the
    // token slots need no scan.
    let away = [
        Zone::Stack,
        Zone::Library,
        Zone::Hand,
        Zone::Graveyard,
        Zone::Exile,
    ]
    .into_iter()
    .fold(Bitboard::EMPTY, |away, zone| away | s.in_zone(zone));
    let tokens =
        (away & L::TOKEN_POOL).filter(|slot| s.object(slot).flags().has(ObjectFlags::TOKEN));
    (graveyard, tokens, legendary)
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
fn check_losses<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>) -> bool {
    let mut causes: Causes<L> = Table::filled(None);
    // Each player's cause is their own: the result is the same in any order.
    for player in s.players_in_seat_order(s.players()) {
        let p = s.player(player);
        // Both may hold at one check: life is named.
        causes[player.index()] = if p.life() <= 0 {
            Some(LossCause::Life)
        } else if p.drew_from_empty() {
            Some(LossCause::Library)
        } else {
            None
        };
    }
    if causes.iter().all(Option::is_none) {
        return false;
    }
    end(s, sink, &causes);
    true
}

/// Why each player loses the game, if they do.
type Causes<L> = PerPlayer<L, Option<LossCause<L>>>;

/// Ends the game as `loser` loses it for `cause`: they conceded
/// ([CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a)): "A player can concede
/// the game at any time. A player who concedes leaves the game immediately. That player loses
/// the game."
pub(crate) fn end_game<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    loser: Seat,
    cause: LossCause<L>,
) {
    let mut causes: Causes<L> = Table::filled(None);
    causes[loser.index()] = Some(cause);
    end(s, sink, &causes);
}

/// Ends the game as an effect has `winner` win it
/// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
/// that a player wins the game." Each of their opponents loses it for `cause`, which leaves
/// the winner alone in the game.
pub(crate) fn win_game<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    winner: Seat,
    cause: LossCause<L>,
) {
    let mut causes: Causes<L> = Table::filled(None);
    // Every opponent gets the one cause: the result is the same in any order.
    for loser in s.players_in_seat_order(s.opponents_of(winner)) {
        causes[loser.index()] = Some(cause);
    }
    end(s, sink, &causes);
}

/// Ends the game as each player with a cause loses it. The one player left wins
/// ([CR 104.2a](https://yawgatog.com/resources/magic-rules/#R1042a)): "A player still in the
/// game wins the game if that player's opponents have all left the game." With nobody left it
/// is a draw ([CR 104.4a](https://yawgatog.com/resources/magic-rules/#R1044a)): "If all the
/// players remaining in a game lose simultaneously, the game is a draw."
///
/// The game always ends here, which is the two-player game's: with more players a loss
/// that leaves two or more in the game ends nothing, and they play on.
fn end<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, causes: &Causes<L>) {
    let left = s
        .players()
        .filter(|player| causes[player.index()].is_none());
    s.end_game(left.sole().map_or(Outcome::Draw, Outcome::Win));
    // They lose at once: the events of one happening, by seat.
    for player in s.players_in_seat_order(s.players()) {
        sink.push(match causes[player.index()] {
            Some(cause) => Event::Lose { player, cause },
            None => Event::Win { player },
        });
    }
}

/// The name of the permanent in `slot`: its face up's (CR 712.6c, 712.6d).
fn name<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> &'static str {
    s.copiable(slot).name(t)
}

/// The legendary permanents each player controls.
fn legendary<L: Layout>(s: &State<L>) -> Legendary<L> {
    let mut by_controller: Legendary<L> = Table::filled(Bitboard::EMPTY);
    // A permanent's derived characteristics, its controller among them, are at its place in
    // the battlefield list.
    for (slot, d) in s.permanents() {
        if d.supertypes.contains(Supertypes::LEGENDARY) {
            by_controller[d.controller.index()].insert(slot);
        }
    }
    by_controller
}

/// Whether the permanents in `a` and `b` have the same name. The same card with the same face
/// up does without comparing the names.
fn same_name<L: Layout>(s: &State<L>, t: &GameTable, a: L::Slot, b: L::Slot) -> bool {
    let (ca, cb) = (s.copiable(a), s.copiable(b));
    ca == cb || ca.name(t) == cb.name(t)
}

/// Whether some player controls two legendary permanents of `legendary` with the same name:
/// whether any legend-rule group exists, kept or not.
fn any_group<L: Layout>(s: &State<L>, t: &GameTable, legendary: &Legendary<L>) -> bool {
    // Each pair is compared once, and whether one matches is the same in any order: the
    // candidate order saves the half of the comparisons that `any` within `any` would repeat,
    // in a check the engine makes after every action.
    legendary.iter().any(|&mine| {
        let mut rest = mine;
        s.in_candidate_order(mine).any(|a| {
            rest.remove(a);
            rest.any(|b| same_name(s, t, a, b))
        })
    })
}

/// The legendary permanents with the same controller and name as the one in `slot`, of
/// `legendary`, each player's.
fn legend_group<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    legendary: &Legendary<L>,
    slot: L::Slot,
) -> Bitboard<L::Slot> {
    let name_of = name(s, t, slot);
    legendary[s.controller(slot).index()].filter(|other| name(s, t, other) == name_of)
}

/// The first legend-rule group, in APNAP order, with no permanent in `kept`: its controller
/// and members.
///
/// [CR 704.5j](https://yawgatog.com/resources/magic-rules/#R7045j): "If two or more legendary
/// permanents with the same name are controlled by the same player, that player chooses one of
/// them, and the rest are put into their owners' graveyards. This is called the 'legend
/// rule.'"
pub(crate) fn unresolved_legends<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    kept: Bitboard<L::Slot>,
) -> Option<(Seat, Bitboard<L::Slot>)> {
    legend_group_in(s, t, &legendary(s), kept)
}

/// [`unresolved_legends`], of `legendary`, the legendary permanents each player controls.
fn legend_group_in<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    legendary: &Legendary<L>,
    kept: Bitboard<L::Slot>,
) -> Option<(Seat, Bitboard<L::Slot>)> {
    // A group needs two legendary permanents of one controller with one name, which most
    // checks lack.
    if legendary.iter().all(|b| b.count() < 2) || !any_group(s, t, legendary) {
        return None;
    }
    for player in s.players_in_turn_order(s.players()) {
        let mut seen = Bitboard::EMPTY;
        for &slot in s.battlefield() {
            if !legendary[player.index()].contains(slot) || seen.contains(slot) {
                continue;
            }
            let group = legend_group(s, t, legendary, slot);
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
pub(crate) fn apply_legend_rule<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    kept: Bitboard<L::Slot>,
) -> Result<(), Full> {
    let legendary = legendary(s);
    // A permanent is in its own group, so the doomed are those not kept whose group has
    // another member.
    let legends = legendary
        .iter()
        .fold(Bitboard::EMPTY, |all, &mine| all | mine);
    let doomed = (legends & !kept).filter(|slot| legend_group(s, t, &legendary, slot).count() >= 2);
    s.settle_question();
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
