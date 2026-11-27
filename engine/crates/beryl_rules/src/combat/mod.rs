//! Combat: declaring attackers and blockers one pick at a time, assigning combat damage
//! and ending combat (architecture §11, decision D8).
//!
//! Declarations and damage divisions are explicit machines whose step lives in
//! [`Pending`]; every candidate offered is legal, so a declaration never has to be undone
//! (CR 508.1, 509.1 return the game to before an illegal one).
//!
//! Implements: CR 506.2, CR 506.4, CR 508.1, CR 508.3a, CR 509.1, CR 702.9b, CR 702.10b, CR 702.14c

use crate::bitboard::Bitboard;
use crate::card::{Keywords, Part, Prompt, Purpose, Types};
use crate::continuous::rule_change;
use crate::decision::{Candidates, Decision, DecisionKind, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{self, Event, Sink};
use crate::replacement::protection;
use crate::state::{AttackTarget, CombatRole, Count, Pending, State, Tracker, TurnStep};
use crate::table::{Capability, GameTable};
use beryl_kernel::reference::Permanent;

mod assignment;
mod banding;
pub mod damage;

pub use assignment::{assigner, preferred_damage};
pub(crate) use assignment::{
    begin_damage, divide, division_decision, exact_decision, first_strike_step,
};

fn has<L: Layout>(s: &State<L>, slot: L::Slot, keyword: Keywords) -> bool {
    s.derived(slot).keywords.contains(keyword)
}

/// Whether the object in `slot` is attacking or blocking.
fn in_combat_now<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    !matches!(s.object(slot).combat(), CombatRole::None)
}

/// The creatures blocking an attacker.
fn blockers_of<L: Layout>(s: &State<L>, attacker: L::Slot) -> Bitboard<L::Slot> {
    s.battlefield()
        .iter()
        .copied()
        .filter(|&slot| s.object(slot).blocks(attacker))
        .collect()
}

/// The active player's creatures that can still be declared as attackers.
///
/// [CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a): "The active player chooses
/// which creatures that they control, if any, will attack. The chosen creatures must be untapped,
/// they can't also be battles, and each one must either have haste or have been controlled by the
/// active player continuously since the turn began."
/// [CR 702.10b](https://yawgatog.com/resources/magic-rules/#R70210b): "If a creature has haste,
/// it can attack even if it hasn't been controlled by its controller continuously since their
/// most recent turn began."
///
/// Nor may a creature that a restriction keeps from attacking every player it could
/// ([`rule_change::cant_attack`]), read only when some card of the game says one
/// (`restricted`, [`rule_change::attacks_restricted`]). In a two-player game every
/// planeswalker it could attack is the one defending player's, so a restriction read for that
/// player is the whole check.
fn eligible_attackers<'a, L: Layout, const NEW_COMBAT: bool>(
    s: &'a State<L>,
    t: &'a GameTable,
) -> impl Iterator<Item = L::Slot> + 'a {
    let active = s.turn().active();
    let restricted = rule_change::attacks_restricted(t);
    s.permanents_with_derived()
        .filter_map(move |(slot, derived)| {
            let o = s.object(slot);
            (derived.controller == active
                && derived.types.contains(Types::CREATURE)
                && !o.is_tapped()
                && (!o.is_summoning_sick() || derived.keywords.contains(Keywords::HASTE))
                && (NEW_COMBAT || !o.is_attacking())
                && !(restricted && rule_change::cant_attack(s, t, slot)))
            .then_some(slot)
        })
}

/// Whether the active player has a creature that can attack now, so that declaring attackers
/// is a decision.
pub(crate) fn can_declare_attackers<L: Layout>(s: &State<L>, t: &GameTable) -> bool {
    eligible_attackers::<L, false>(s, t).next().is_some()
}

/// Why the viewer's creature in `slot` can't be declared as an attacker now, for a client to
/// say why clicking it does nothing ([CR 508.1a], [CR 508.1c]; see [CR 302.6]). None if it can
/// attack, is already attacking, or is not a creature the viewer controls on the battlefield.
///
/// [CR 508.1a]: https://yawgatog.com/resources/magic-rules/#R5081a
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
/// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
pub fn attack_refusal<L: Layout>(
    obs: crate::observation::Observation<'_, L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<crate::decision::Refusal<L>> {
    let (s, player) = (obs.shown(), obs.viewer());
    let (_, derived) = s
        .permanents_with_derived()
        .find(|&(perm, _)| perm == slot)?;
    if derived.controller != player || !derived.types.contains(Types::CREATURE) {
        return None;
    }
    let o = s.object(slot);
    if o.is_attacking() {
        return None;
    }
    if o.is_tapped() {
        return Some(crate::decision::Refusal::Tapped);
    }
    if o.is_summoning_sick() && !derived.keywords.contains(Keywords::HASTE) {
        return Some(crate::decision::Refusal::SummoningSickness);
    }
    if rule_change::attacks_restricted(t) {
        return rule_change::attack_refusal(s, t, slot);
    }
    None
}

/// Whether a later combat would offer an attack from this position. Current combat roles
/// expire before that combat begins (see [CR 511.3]): "all creatures, battles, and planeswalkers
/// are removed from combat."
///
/// [CR 511.3]: https://yawgatog.com/resources/magic-rules/#R5113
pub(crate) fn can_attack_in_later_combat<L: Layout>(s: &State<L>, t: &GameTable) -> bool {
    eligible_attackers::<L, true>(s, t).next().is_some()
}

/// The declare attackers step's turn-based action begins ([CR 508.1]): "First, the active player
/// declares attackers. This turn-based action doesn't use the stack."
///
/// [CR 508.1]: https://yawgatog.com/resources/magic-rules/#R5081
pub(crate) fn begin_attackers<L: Layout>(s: &mut State<L>, t: &GameTable) {
    if can_declare_attackers(s, t) {
        s.ask(Pending::DeclareAttackers { attacker: None });
    }
}

/// The next pick of the attack declaration: a creature or done, or, for the creature just
/// chosen, what it attacks.
///
/// [CR 508.1b](https://yawgatog.com/resources/magic-rules/#R5081b): the active player "announces
/// which player, planeswalker, or battle each of the chosen creatures is attacking." Offered only
/// when the defending player controls a planeswalker; otherwise the one candidate is applied
/// silently (§7.2).
pub(crate) fn attack_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    attacker: Option<L::Slot>,
) -> Result<Decision<L>, Full> {
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    let (player, kind, part) = if attacker.is_none() {
        list.push(Opt::Done)?;
        let attackers = collapse(s, eligible_attackers::<L, false>(s, t).collect());
        Candidates::<L>::extend_options(&mut list, attackers, |out, slot| {
            out.push(Opt::attack(slot))
        })?;
        if t.can(Capability::Banding) {
            banding::append_attack_options(s, t, attackers, &mut list)?;
        }
        (s.turn().active(), DecisionKind::Attacker, Part::Pick)
    } else {
        list = attack_targets(s)?;
        (s.turn().active(), DecisionKind::AttackTarget, Part::Aim)
    };
    Ok(Decision {
        player,
        decider: player,
        kind,
        candidates: Candidates::Options(list),
        // The creature just chosen is the one being aimed.
        prompt: Prompt::new(Purpose::Attack).part(part),
        about: attacker.map_or(Bitboard::EMPTY, Bitboard::single),
    })
}

/// What a creature can attack ([CR 506.2]): "that player, planeswalkers they control, and
/// battles they protect may be attacked." The player option comes before the planeswalkers.
/// [`Full`] if the planeswalkers are more than a fixed layout's list of options holds.
///
/// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
pub(crate) fn attack_targets<L: Layout>(s: &State<L>) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let defending = s.defending_players();
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    list.push(Opt::Defender(None))?;
    for (slot, derived) in s.permanents_with_derived() {
        if derived.types.contains(Types::PLANESWALKER) && defending.contains(derived.controller) {
            list.push(Opt::Defender(Some(slot)))?;
        }
    }
    Ok(list)
}

/// Applies one pick of the attack declaration.
pub(crate) fn attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    opt: Opt<L>,
) -> Result<(), Full> {
    match opt {
        Opt::Attack { slot, band: None } => {
            declare_attacker(s, t, s.permanent_at(slot).expect("a declared attacker"));
            s.ask(Pending::DeclareAttackers {
                attacker: Some(slot),
            });
            Ok(())
        }
        Opt::Attack {
            slot,
            band: Some(first),
        } => banding::attack(
            s,
            t,
            s.permanent_at(slot).expect("a declared attacker"),
            first,
        ),
        Opt::Defender(target) => {
            let Pending::DeclareAttackers {
                attacker: Some(attacker),
            } = *s.pending()
            else {
                unreachable!("an attack target without an attacker");
            };
            if let Some(target) = target {
                let attacker = s.permanent_at(attacker).expect("a declared attacker");
                s.choose_attacked(attacker, AttackTarget::Planeswalker(target));
            }
            s.ask(Pending::DeclareAttackers { attacker: None });
            Ok(())
        }
        Opt::Pass
        | Opt::PlayLand(_)
        | Opt::Play
        | Opt::Draw
        | Opt::Keep
        | Opt::Mulligan
        | Opt::FreeMulligan
        | Opt::Trigger(_)
        | Opt::Yes
        | Opt::No
        | Opt::Done
        | Opt::Block(_)
        | Opt::Lethal(_)
        | Opt::Cast { .. }
        | Opt::Activate { .. }
        | Opt::Mana { .. }
        | Opt::Pool(_)
        | Opt::Number(_)
        | Opt::Plan(_)
        | Opt::Player(_)
        | Opt::Ability(_)
        | Opt::Object(_)
        | Opt::TurnFaceUp(_)
        | Opt::Companion => finish_attack(s, t, sink),
    }
}

/// Makes the chosen creature an attacker. Under [CR 508.1k], "Each chosen creature still
/// controlled by the active player becomes an attacking creature."
///
/// [CR 508.1k]: https://yawgatog.com/resources/magic-rules/#R5081k
#[inline]
fn declare_attacker<L: Layout>(s: &mut State<L>, t: &GameTable, attacker: Permanent<L>) {
    let mut defending = s.players_in_turn_order(s.defending_players());
    let player = defending.next().expect("combat has a defending player");
    s.declare_attacker(attacker, player);
    // See CR 508.4: creatures put onto the battlefield attacking never "attacked"
    // for effects. Only a declaration pick records the turn's attack history.
    s.track(t, Tracker::AttackedThisTurn, attacker);
    s.count(t, Count::AttackersDeclared, s.turn().active());
}

/// Finishes the attack declaration: the attackers tap, then attack triggers trigger.
///
/// [CR 508.1f](https://yawgatog.com/resources/magic-rules/#R5081f): "The active player taps the
/// chosen creatures. Tapping a creature when it's declared as an attacker isn't a cost; attacking
/// simply causes creatures to become tapped."
///
/// [CR 508.3a](https://yawgatog.com/resources/magic-rules/#R5083a): "An ability that reads
/// "Whenever [a creature] attacks, . . ." triggers if that creature is declared as an attacker. ...
/// Such abilities won't trigger if a creature is put onto the battlefield attacking." The event
/// names only the declared attackers (see [CR 508.4]): creatures put onto the battlefield attacking,
/// "for the purposes of trigger events and effects, ... never 'attacked.'"
///
/// The taps and the attack are kept as one group: the one declaration ([CR 508.1]: "First, the
/// active player declares attackers. This turn-based action doesn't use the stack.").
///
/// [CR 508.4]: https://yawgatog.com/resources/magic-rules/#R5084
fn finish_attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    s.settle_question();
    sink.together();
    let declared = declare_attack(s, t, sink);
    sink.apart();
    declared
}

/// [`finish_attack`], inside the group it opens.
fn declare_attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let attackers: Bitboard<L::Slot> = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&slot| s.object(slot).is_attacking())
        .collect();
    // In battlefield order, never slot order: the taps' events are seen in this order, and a
    // slot number says nothing about a card (architecture §13). Tapping moves nothing. A
    // sole attacker has no order, and needs no walk of the battlefield.
    let tap = |s: &mut State<L>, sink: &mut Sink<L>, slot| {
        if has(s, slot, Keywords::VIGILANCE) {
            return Ok(());
        }
        let Some(permanent) = s.permanent_at(slot) else {
            return Ok(());
        };
        event::set_tapped(s, t, sink, permanent, true)
    };
    let mut untapped = attackers;
    if attackers.count() > 1 {
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            if untapped.contains(slot) {
                untapped.remove(slot);
                tap(s, sink, slot)?;
                if untapped.is_empty() {
                    break;
                }
            }
        }
    }
    if let Some(slot) = untapped.sole() {
        tap(s, sink, slot)?;
    }
    if attackers.is_empty() {
        return Ok(());
    }
    let declared = s.declare_attack(s.permanents_in(attackers));
    event::settle(s, t, sink, declared)
}

/// Whether `blocker` may block `attacker`: an attacker attacking the defending player or a
/// planeswalker they control (one attacking nothing can't be blocked), within its evasion:
/// flying, its landwalk ([`landwalks`]) and its protection.
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "For each of the chosen
/// creatures, the defending player chooses one creature for it to block that's attacking that
/// player, a planeswalker they control, or a battle they protect."
///
/// [CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b): "A creature with flying can't
/// be blocked except by creatures with flying and/or reach."
///
/// See [CR 702.16f](https://yawgatog.com/resources/magic-rules/#R70216f): "Attacking creatures with
/// protection can’t be blocked by creatures that have the stated quality."
fn can_block<L: Layout>(s: &State<L>, t: &GameTable, blocker: L::Slot, attacker: L::Slot) -> bool {
    let Some(target) = s.object(attacker).attack_target() else {
        return false;
    };
    if !matches!(
        target,
        AttackTarget::Player(_) | AttackTarget::Planeswalker(_)
    ) {
        return false;
    }
    let evasion = s.derived(attacker).keywords;
    (!evasion.contains(Keywords::FLYING)
        || has(s, blocker, Keywords::FLYING)
        || has(s, blocker, Keywords::REACH))
        && !(evasion.intersects(Keywords::ANY_LANDWALK) && landwalks(s, t, evasion, target))
        && !protection::protected(s, t, attacker, blocker)
}

/// Whether an attacker with the keywords `walk`, attacking `target`, can't be blocked for its
/// landwalk ([CR 702.14c]): "A creature with landwalk can't be blocked as long as the defending
/// player controls at least one land with the specified land type (as in “islandwalk”)". The
/// defending player is the one attacked, or the controller of the planeswalker attacked. Most
/// creatures have no landwalk, and [`can_block`] tests that before it calls this, which it
/// does for each blocker and attacker, so the scan of the lands is out of line.
///
/// [CR 702.14c]: https://yawgatog.com/resources/magic-rules/#R70214c
#[cold]
#[inline(never)]
fn landwalks<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    walk: Keywords,
    target: AttackTarget<L>,
) -> bool {
    let defending = match target {
        AttackTarget::Player(player) => player,
        AttackTarget::Planeswalker(walker) => s.controller(walker),
        AttackTarget::Removed => return false,
    };
    Keywords::LANDWALK
        .iter()
        .filter(|&&(keyword, _)| walk.contains(keyword))
        .any(|&(_, subtype)| {
            let has = t.subtypes().has(subtype);
            s.permanents_with_derived().any(|(_, d)| {
                d.controller == defending && d.types.contains(Types::LAND) && has.of(d.subtypes)
            })
        })
}

/// Untapped creatures of the defending players that can still be assigned a block.
/// A rule change can lift the limit of one block or prevent blocking altogether.
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "The defending player chooses
/// which creatures they control, if any, will block. The chosen creatures must be untapped and they
/// can't also be battles."
fn eligible_blockers<'a, L: Layout>(
    s: &'a State<L>,
    t: &'a GameTable,
) -> impl Iterator<Item = L::Slot> + 'a {
    let defending = s.defending_players();
    s.permanents_with_derived()
        .filter_map(move |(slot, derived)| {
            if !defending.contains(derived.controller) || !derived.types.contains(Types::CREATURE) {
                return None;
            }
            let o = s.object(slot);
            let can = !o.is_tapped()
                && if o.is_blocking() {
                    rule_change::multiple_blocks_possible(t) && can_block_again(s, t, slot)
                } else {
                    !rule_change::cant_block(s, t, slot)
                        && s.battlefield().iter().any(|&a| can_block(s, t, slot, a))
                };
            can.then_some(slot)
        })
}

/// Whether the creature can add a block to those already declared (see CR 509.1a).
#[cold]
#[inline(never)]
fn can_block_again<L: Layout>(s: &State<L>, t: &GameTable, blocker: L::Slot) -> bool {
    rule_change::can_block_any_number(s, t, blocker)
        && !rule_change::cant_block(s, t, blocker)
        && s.battlefield().iter().any(|&attacker| {
            !s.object(blocker).blocks(attacker) && can_block(s, t, blocker, attacker)
        })
}

/// Whether the defending player has a creature that can block now, so that declaring blockers
/// is a decision.
pub(crate) fn can_declare_blockers<L: Layout>(s: &State<L>, t: &GameTable) -> bool {
    eligible_blockers(s, t).next().is_some()
}

/// The declare blockers step's turn-based action begins ([CR 509.1]): "First, the defending player
/// declares blockers. This turn-based action doesn't use the stack."
///
/// [CR 509.1]: https://yawgatog.com/resources/magic-rules/#R5091
pub(crate) fn begin_blockers<L: Layout>(s: &mut State<L>, t: &GameTable) {
    if can_declare_blockers(s, t) {
        s.ask(Pending::DeclareBlockers { blocker: None });
    }
}

/// The next pick of the block declaration: a creature or done, or, for the creature just
/// chosen, the attacker it blocks. Evasion prunes both.
pub(crate) fn block_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    blocker: Option<L::Slot>,
) -> Result<Decision<L>, Full> {
    if let Some(blocker) = blocker {
        let player = s.controller(blocker);
        let legal = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&a| can_block(s, t, blocker, a));
        let attackers = if rule_change::multiple_blocks_possible(t) {
            legal.filter(|&a| !s.object(blocker).blocks(a)).collect()
        } else {
            legal.collect()
        };
        return Ok(Decision {
            player,
            decider: player,
            kind: DecisionKind::BlockTarget,
            candidates: Candidates::Objects(collapse(s, attackers)),
            // The creature just chosen is the one being aimed.
            prompt: Prompt::new(Purpose::Block).part(Part::Aim),
            about: Bitboard::single(blocker),
        });
    }
    let player = s
        .players_in_turn_order(s.defending_players())
        .next()
        .expect("a block declaration has a defending player");
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    list.push(Opt::Done)?;
    let able = eligible_blockers(s, t).collect();
    Candidates::<L>::extend_options(&mut list, collapse(s, able), |out, slot| {
        out.push(Opt::Block(slot))
    })?;
    Ok(Decision {
        player,
        decider: player,
        kind: DecisionKind::Blocker,
        candidates: Candidates::Options(list),
        prompt: Prompt::new(Purpose::Block),
        about: Bitboard::EMPTY,
    })
}

/// Chooses the next blocker or finishes the declaration ([CR 509.1a]):
/// "The defending player chooses which creatures they control, if any, will block."
///
/// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
pub(crate) fn choose_blocker<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, option: Opt<L>) {
    match option {
        Opt::Block(slot) => block_with(s, s.permanent_at(slot).expect("a declared blocker")),
        Opt::Pass
        | Opt::PlayLand(_)
        | Opt::Play
        | Opt::Draw
        | Opt::Keep
        | Opt::Mulligan
        | Opt::FreeMulligan
        | Opt::Trigger(_)
        | Opt::Yes
        | Opt::No
        | Opt::Done
        | Opt::Attack { .. }
        | Opt::Defender(_)
        | Opt::Lethal(_)
        | Opt::Cast { .. }
        | Opt::Activate { .. }
        | Opt::Mana { .. }
        | Opt::Pool(_)
        | Opt::Number(_)
        | Opt::Plan(_)
        | Opt::Player(_)
        | Opt::Ability(_)
        | Opt::Object(_)
        | Opt::TurnFaceUp(_)
        | Opt::Companion => finish_blocks(s, sink),
    }
}

/// Declares a blocker, whose attacker is the next pick.
///
/// [CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g): "Each chosen creature still
/// controlled by the defending player becomes a blocking creature."
fn block_with<L: Layout>(s: &mut State<L>, blocker: Permanent<L>) {
    if !s.object(blocker.slot()).is_blocking() {
        s.declare_blocker(blocker);
    }
    s.ask(Pending::DeclareBlockers {
        blocker: Some(blocker.slot()),
    });
}

/// Has the blocker just declared block `attacker`.
///
/// [CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h): "An attacking creature with one
/// or more creatures declared as blockers for it becomes a blocked creature". It becomes blocked as
/// soon as its first blocker is chosen, so a blocked and an unblocked attacker are never offered as
/// interchangeable to the next blocker.
pub(crate) fn block_attacker<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    attacker: L::Slot,
) -> Result<(), Full> {
    let Pending::DeclareBlockers {
        blocker: Some(blocker),
    } = *s.pending()
    else {
        unreachable!("a block target without a blocker");
    };
    let blocker = s.permanent_at(blocker).expect("a declared blocker");
    let attacker = s.permanent_at(attacker).expect("a declared attacker");
    s.block(blocker, attacker)?;
    if t.can(Capability::Banding) {
        banding::spread_block(s, blocker, attacker)?;
    }
    s.ask(Pending::DeclareBlockers { blocker: None });
    Ok(())
}

/// Finishes the block declaration, and says what was declared: each blocker and the creature it
/// blocks, in the order the blockers entered the battlefield, as one group ([CR 509.1]: "First, the
/// defending player declares blockers. This turn-based action doesn't use the stack."). Only a
/// recording sink is told: no card yet reads a block, so the declaration goes through no trigger
/// matching, and a search pays one branch for it.
///
/// [CR 509.1]: https://yawgatog.com/resources/magic-rules/#R5091
pub(crate) fn finish_blocks<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>) {
    s.settle_question();
    if sink.records() {
        note_blocks(s, sink);
    }
}

#[cold]
fn note_blocks<L: Layout>(s: &State<L>, sink: &mut Sink<L>) {
    sink.together();
    for &slot in s.battlefield() {
        for attacker in s.object(slot).blocked_attackers().iter() {
            sink.note(Event::Block {
                player: s.controller(slot),
                blocker: s.object_ref(slot),
                attacker: s.object_ref(attacker),
            });
        }
    }
    sink.apart();
}

/// Removes from combat what CR 506.4 removes while combat goes on: "A permanent is removed
/// from combat if it leaves the battlefield, if its controller ... changes, ... if it's a
/// planeswalker that's being attacked and stops being a planeswalker, ... or if it's an
/// attacking or blocking creature that ... stops being a creature". Leaving the battlefield
/// is handled as objects change zones; this handles the rest, after the layers settle.
///
/// [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A creature that's removed from
/// combat stops being an attacking, blocking, blocked, and/or unblocked creature. A planeswalker or
/// battle that's removed from combat stops being attacked."
pub(crate) fn prune<L: Layout>(s: &mut State<L>) {
    // A permanent keeps no record of who controlled it as it was declared. An attacker's
    // controller was the attacking player. A blocker's, and an attacked planeswalker's, was a
    // defending player, of which a two-player game has one: with more, a change from one
    // defending player to another is a change this does not see.
    let attacking = s.turn().active();
    let defending = s.defending_players();
    let mut index = 0;
    while let Some(&slot) = s.battlefield().get(index) {
        index += 1;
        let (attacker, walker) = match s.object(slot).combat() {
            CombatRole::None => continue,
            CombatRole::Attacking {
                target: AttackTarget::Planeswalker(walker),
                ..
            } => (true, Some(*walker)),
            CombatRole::Attacking { .. } => (true, None),
            CombatRole::Blocking { .. } => (false, None),
        };
        let changed = |controller: Seat| {
            if attacker {
                controller != attacking
            } else {
                !defending.contains(controller)
            }
        };
        if !s.derived(slot).types.contains(Types::CREATURE) || changed(s.controller(slot)) {
            let permanent = s.permanent_at(slot).expect("a permanent in combat");
            s.remove_from_combat(permanent);
        } else if let Some(walker) = walker
            && (!defending.contains(s.controller(walker))
                || !s.derived(walker).types.contains(Types::PLANESWALKER))
        {
            let attacker = s.permanent_at(slot).expect("an attacking permanent");
            s.choose_attacked(attacker, AttackTarget::Removed);
        }
    }
}

/// Whether the game is in the combat phase, after attackers may have been declared.
pub(crate) fn in_combat(step: TurnStep) -> bool {
    matches!(
        step,
        TurnStep::DeclareAttackers
            | TurnStep::DeclareBlockers
            | TurnStep::FirstStrikeDamage
            | TurnStep::CombatDamage
            | TurnStep::EndOfCombat
    )
}
