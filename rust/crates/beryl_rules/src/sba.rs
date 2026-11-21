//! State-based actions (architecture §8.4).
//!
//! Implements: CR 104.2a, CR 104.3a, CR 104.4a, CR 704.3, CR 704.5a, CR 704.5b, CR 704.5d, CR 704.5e, CR 704.5f, CR 704.5g, CR 704.5i, CR 704.5j, CR 704.5m, CR 704.5n

use beryl_kernel::format::Format;

use crate::PlayerSet;

use crate::actions::attach;
use crate::bitboard::Bitboard;
use crate::card::{Keywords, Subtype, Supertypes, Types};
use crate::continuous::layers;
use crate::decision::{Outcome, StallReason};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, PerPlayer, Table};
use crate::replacement;
use crate::replacement::event::{self, Entry, Event, LossCause, Reason, Sink};
use crate::state::{Pending, State, Zone};
use crate::table::GameTable;
use beryl_card::Regeneration;

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
    if check_losses(s, t.format(), sink) {
        return Ok(true);
    }
    layers::refresh(s, t, sink)?;
    // State triggers are no state-based actions (see CR 603.8: "Note that state triggers aren't the
    // same as state-based actions."), but they are found at the same moments.
    crate::stack::trigger::match_states(s, t)?;
    let (mut graveyard, tokens, legendary) = doomed(s, t);
    let loose = loose_equipment(s, t);
    let legends = legend_group_in(s, t, &legendary, Bitboard::EMPTY).is_some();
    if graveyard.is_empty() && tokens.is_empty() && loose.is_empty() && !legends {
        s.reset_state_based_checks();
        return Ok(false);
    }
    if s.count_state_based_check() > s.profile().limits.state_based_iterations {
        s.end_game(Outcome::Stalled(StallReason::StateBasedIterations));
        return Ok(true);
    }
    // CR 704.3 checks all conditions before performing any actions. Detaching Equipment
    // can change a creature's toughness, so classify lethal damage before detaching it.
    let lethal = if s.has_regeneration_shields() {
        destroyed_for_damage(s, t, graveyard)
    } else {
        Bitboard::EMPTY
    };
    if !loose.is_empty() {
        for index in 0..s.battlefield().len() {
            let slot = s.battlefield()[index];
            if loose.contains(slot) {
                let equipment = s.permanent_at(slot).expect("Equipment on the battlefield");
                s.unattach(equipment);
            }
        }
    }
    if graveyard.is_empty() && tokens.is_empty() {
        if legends {
            s.ask(Pending::LegendRule {
                kept: Bitboard::EMPTY,
            });
        }
        return Ok(true);
    }
    // A creature with lethal damage is destroyed (CR 704.5g), so its destruction is proposed
    // to the replacement effects of destruction: a regeneration shield replaces it
    // (see [CR 614.8](https://yawgatog.com/resources/magic-rules/#R6148)): "Regeneration is a
    // destruction-replacement effect."
    if !lethal.is_empty() {
        let destroyed = replacement::destruction(s, t, sink, lethal, Regeneration::Allowed)?;
        graveyard &= !(lethal & !destroyed);
    }
    // "As a single event": the one batch below is every action of this check that has an
    // event, and a batch's events are kept as one group (`event::move_batch`).
    s.reclaim_tokens(tokens)?;
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
/// other than the battlefield, it ceases to exist." A copy of a spell, which is in the shared pool
/// with the tokens, ceases to exist in a zone other than the stack ([CR 704.5e]).
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
/// into its owner's graveyard." What is legal is what the enchant abilities the Aura has now
/// allow, with protection ([`attach::may_enchant`]): Animate Dead attached to a creature
/// card that left its graveyard, or to a creature it did not put onto the battlefield, goes
/// to the graveyard. An Aura whose object left the battlefield or a graveyard is attached to
/// nothing (`zone::leave` detaches it from a permanent, `event::move_batch_to` from a card).
///
/// With them, in the same pass, the legendary permanents each player controls, for the legend
/// rule.
///
/// [CR 704.5e]: https://yawgatog.com/resources/magic-rules/#R7045e
fn doomed<L: Layout>(s: &State<L>, t: &GameTable) -> Doomed<L> {
    // A game none of whose cards is an Aura or makes one never asks (the count gate).
    let aura = t.subtypes().has(Subtype::Aura);
    let auras = aura.possible();
    let mut graveyard = Bitboard::EMPTY;
    let mut legendary: Legendary<L> = Table::filled(Bitboard::EMPTY);
    for (slot, d) in s.permanents_with_derived() {
        if d.supertypes.contains(Supertypes::LEGENDARY) {
            legendary[d.controller.index()].insert(slot);
        }
        let o = s.object(slot);
        let types = d.types;
        // See CR 702.12b: indestructible permanents "ignore the state-based action that checks
        // for lethal damage".
        let dies = types.contains(Types::CREATURE)
            && (d.toughness <= 0
                || (o.damage() > 0
                    && i16::from(o.damage()) >= i16::from(d.toughness)
                    && !d.keywords.contains(Keywords::INDESTRUCTIBLE)));
        let unattached = auras
            && aura.of(d.subtypes)
            && match o.attached_to() {
                None => true,
                Some(host) => !attach::may_enchant(s, t, slot, host),
            };
        if dies || unattached || (types.contains(Types::PLANESWALKER) && o.counters().loyalty == 0)
        {
            graveyard.insert(slot);
        }
    }
    // The zones' memberships hold everything off the battlefield and in a zone, so the
    // token slots need no scan. A token on the stack is a copy of a spell, which no token
    // ever is otherwise, and stays there
    // ([CR 704.5e](https://yawgatog.com/resources/magic-rules/#R7045e)): "If a copy of a spell is in a zone
    // other than the stack, it ceases to exist."
    let away = [Zone::Library, Zone::Hand, Zone::Graveyard, Zone::Exile]
        .into_iter()
        .fold(Bitboard::EMPTY, |away, zone| away | s.in_zone(zone));
    let tokens = (away & L::TOKEN_POOL).filter(|slot| s.object(slot).is_token());
    (graveyard, tokens, legendary)
}

/// Those of `graveyard`, the permanents [`doomed`] found, that go there only because they are
/// destroyed for lethal damage (CR 704.5g), which a regeneration shield may replace. A
/// creature with toughness 0 is put into its owner's graveyard and not destroyed
/// ([CR 704.5f](https://yawgatog.com/resources/magic-rules/#R7045f)), and neither is an
/// Aura attached to nothing or a planeswalker with no loyalty, so no shield saves them.
/// Only a game with a shield asks, so [`doomed`], which every check calls, reads no more.
fn destroyed_for_damage<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    graveyard: Bitboard<L::Slot>,
) -> Bitboard<L::Slot> {
    let aura = t.subtypes().has(Subtype::Aura);
    graveyard.filter(|slot| {
        let (d, o) = (s.derived(slot), s.object(slot));
        let lethal = d.types.contains(Types::CREATURE)
            && d.toughness > 0
            && i16::from(o.damage()) >= i16::from(d.toughness);
        let unattached = o.attached_to().is_none() && aura.of(d.subtypes);
        let no_loyalty = d.types.contains(Types::PLANESWALKER) && o.counters().loyalty == 0;
        lethal && !unattached && !no_loyalty
    })
}

/// The Equipment attached to a permanent that may not be equipped by it
/// ([CR 704.5n](https://yawgatog.com/resources/magic-rules/#R7045n)): "If an Equipment or
/// Fortification is attached to an illegal permanent or to a player, it becomes unattached
/// from that permanent or player. It remains on the battlefield." What may be equipped is
/// `attach::may_equip`'s (see CR 301.5c). An Equipment whose creature left the battlefield is
/// already attached to nothing (`zone::leave` detaches it).
#[inline]
fn loose_equipment<L: Layout>(s: &State<L>, t: &GameTable) -> Bitboard<L::Slot> {
    // A game none of whose cards is an Equipment never asks, and the engine loop, which runs
    // this check after every action, keeps only the test inline.
    if !t.subtypes().has(Subtype::Equipment).possible() {
        return Bitboard::EMPTY;
    }
    attached_illegally(s, t)
}

/// [`loose_equipment`] in a game with an Equipment.
#[cold]
#[inline(never)]
fn attached_illegally<L: Layout>(s: &State<L>, t: &GameTable) -> Bitboard<L::Slot> {
    let mut loose = Bitboard::EMPTY;
    for (slot, d) in s.permanents_with_derived() {
        let Some(host) = s.object(slot).attached_to() else {
            continue;
        };
        // An Aura's is CR 704.5m's, above. An Equipment that lost the subtype is an object
        // that may equip nothing (see CR 301.5c).
        if t.subtypes().has(Subtype::Aura).of(d.subtypes) {
            continue;
        }
        if !crate::actions::attach::may_equip_in(s, t, slot, host) {
            loose.insert(slot);
        }
    }
    loose
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
fn check_losses<L: Layout>(s: &mut State<L>, format: Format, sink: &mut Sink<L>) -> bool {
    let losses = format.losses();
    let mut causes: Causes<L> = Table::filled(None);
    // Each player's cause is their own: the result is the same in any order.
    for player in s.players().in_seat_order() {
        let p = s.player(player);
        // Both may hold at one check: life is named.
        causes[player.index()] = if p.life() <= losses.life {
            Some(LossCause::Life)
        } else if losses.empty_library && p.drew_from_empty() {
            Some(LossCause::Library)
        } else {
            None
        };
    }
    if causes.iter().all(Option::is_none) {
        return false;
    }
    end(s, format, sink, &causes);
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
    format: Format,
    sink: &mut Sink<L>,
    loser: Seat,
    cause: LossCause<L>,
) {
    let mut causes: Causes<L> = Table::filled(None);
    causes[loser.index()] = Some(cause);
    end(s, format, sink, &causes);
}

/// Ends the game as an effect has each of `losers` lose it
/// (see [CR 104.3e](https://yawgatog.com/resources/magic-rules/#R1043e)): "An effect may state
/// that a player loses the game." With none of them, nothing ends.
pub(crate) fn lose_game<L: Layout>(
    s: &mut State<L>,
    format: Format,
    sink: &mut Sink<L>,
    losers: PlayerSet,
    cause: LossCause<L>,
) {
    if losers.is_empty() {
        return;
    }
    let mut causes: Causes<L> = Table::filled(None);
    // Every one of them gets the one cause: the result is the same in any order.
    for loser in losers.in_seat_order() {
        causes[loser.index()] = Some(cause);
    }
    end(s, format, sink, &causes);
}

/// Ends the game as each player with a cause loses it. The one player left wins
/// ([CR 104.2a](https://yawgatog.com/resources/magic-rules/#R1042a)): "A player still in the
/// game wins the game if that player's opponents have all left the game." With nobody left it
/// is a draw ([CR 104.4a](https://yawgatog.com/resources/magic-rules/#R1044a)): "If all the
/// players remaining in a game lose simultaneously, the game is a draw."
///
/// The format determines whether the remaining players constitute one side.
fn end<L: Layout>(s: &mut State<L>, format: Format, sink: &mut Sink<L>, causes: &Causes<L>) {
    let left = s
        .players()
        .filter(|player| causes[player.index()].is_none());
    let Some(outcome) = format.outcome_after_loss(left) else {
        return;
    };
    s.end_game(outcome);
    // They lose at once: the events of one happening, by seat, kept as one group.
    sink.together();
    for player in s.players().in_seat_order() {
        sink.push(match causes[player.index()] {
            Some(cause) => Event::Lose { player, cause },
            None => Event::Win { player },
        });
    }
    sink.apart();
}

/// The name of the permanent in `slot`: its face up's (see CR 712.6c, 712.6d).
fn name<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> &'static str {
    s.definition(slot).name(t)
}

/// The legendary permanents each player controls.
fn legendary<L: Layout>(s: &State<L>) -> Legendary<L> {
    let mut by_controller: Legendary<L> = Table::filled(Bitboard::EMPTY);
    // A permanent's derived characteristics, its controller among them, are at its place in
    // the battlefield list.
    for (slot, d) in s.permanents_with_derived() {
        if d.supertypes.contains(Supertypes::LEGENDARY) {
            by_controller[d.controller.index()].insert(slot);
        }
    }
    by_controller
}

/// Whether the permanents in `a` and `b` have the same name. The same card with the same face
/// up does without comparing the names.
fn same_name<L: Layout>(s: &State<L>, t: &GameTable, a: L::Slot, b: L::Slot) -> bool {
    let (ca, cb) = (s.definition(a), s.definition(b));
    ca == cb || ca.name(t) == cb.name(t)
}

/// Whether some player controls two legendary permanents of `legendary` with the same name:
/// whether any legend-rule group exists, kept or not.
fn any_group<L: Layout>(s: &State<L>, t: &GameTable, legendary: &Legendary<L>) -> bool {
    legendary
        .iter()
        .any(|&mine| mine.any_pair(|a, b| same_name(s, t, a, b)))
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
