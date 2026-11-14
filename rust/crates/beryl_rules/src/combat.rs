//! Combat: declaring attackers and blockers one pick at a time, and combat damage
//! (architecture §11, decision D8, CR 506 to 511).
//!
//! Declarations and damage divisions are explicit machines whose step lives in
//! [`Pending`]; every candidate offered is legal, so a declaration never has to be undone
//! (CR 508.1, 509.1 return the game to before an illegal one).

use crate::bitboard::Bitboard;
use crate::card::{Keywords, Part, Prompt, Purpose, Types};
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::engine::collapse;
use crate::event::{self, Event, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{AttackTarget, CombatRole, Pending, State, TurnStep};
use crate::table::GameTable;

mod assignment;

pub use assignment::preferred_damage;
pub(crate) use assignment::{begin_damage, divide, division_decision, first_strike_step};

/// The defending player ([CR 506.2]): "During the combat phase of a two-player game, the nonactive
/// player is the defending player".
///
/// The declarations here are the two-player game's: [`Opt::Defender`] names a planeswalker or
/// "the" defending player, and one player declares every block. With more players each of the
/// attacking player's opponents is a defending player (CR 802.2), an attacker is declared
/// against one of them and each declares their own blocks.
///
/// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
fn defender<L: Layout>(s: &State<L>) -> Seat {
    s.defending_players()
        .sole()
        .expect("one defending player in a two-player game")
}

fn is_creature<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    s.derived(slot).types.contains(Types::CREATURE)
}

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

/// Whether any creature can attack now, so that the declaration is worth starting.
///
/// [CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a): "The active player chooses
/// which creatures that they control, if any, will attack. The chosen creatures must be untapped,
/// they can't also be battles, and each one must either have haste or have been controlled by the
/// active player continuously since the turn began." None of the cards has haste.
fn may_attack<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    let o = s.object(slot);
    s.controller(slot) == s.turn().active()
        && is_creature(s, slot)
        && !o.is_tapped()
        && !o.is_summoning_sick()
        && !o.is_attacking()
}

/// Whether the active player has a creature that can attack now, so that declaring attackers
/// is a decision.
pub(crate) fn can_declare_attackers<L: Layout>(s: &State<L>) -> bool {
    s.battlefield().iter().any(|&slot| may_attack(s, slot))
}

/// The declare attackers step's turn-based action begins ([CR 508.1]): "First, the active player
/// declares attackers. This turn-based action doesn't use the stack."
///
/// [CR 508.1]: https://yawgatog.com/resources/magic-rules/#R5081
pub(crate) fn begin_attackers<L: Layout>(s: &mut State<L>) {
    if can_declare_attackers(s) {
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
    attacker: Option<L::Slot>,
) -> Result<Decision<L>, Full> {
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    let (player, kind, part) = if attacker.is_none() {
        list.push(Opt::Done)?;
        let able: Bitboard<L::Slot> = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| may_attack(s, slot))
            .collect();
        // Candidates are listed in the candidate order, which a `Choice` counts in.
        for slot in s.in_candidate_order(collapse(s, able)) {
            list.push(Opt::Attack(slot))?;
        }
        (s.turn().active(), DecisionKind::Attacker, Part::Pick)
    } else {
        list = attack_targets(s)?;
        (s.turn().active(), DecisionKind::AttackTarget, Part::Aim)
    };
    Ok(Decision {
        player,
        kind,
        candidates: Candidates::Options(list),
        // The creature just chosen is the one being aimed.
        prompt: Prompt::new(Purpose::Attack).part(part),
        about: attacker.map_or(Bitboard::EMPTY, Bitboard::single),
    })
}

/// What a creature can attack: the defending player, then each planeswalker they control.
/// [`Full`] if the planeswalkers are more than a fixed layout's list of options holds.
pub(crate) fn attack_targets<L: Layout>(s: &State<L>) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let defending = defender(s);
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    list.push(Opt::Defender(None))?;
    for &slot in s.battlefield() {
        if s.controller(slot) == defending && s.derived(slot).types.contains(Types::PLANESWALKER) {
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
        Opt::Attack(slot) => {
            // CR 508.1k: "Each chosen creature still controlled by the active player becomes
            // an attacking creature."
            s.declare_attacker(slot, defender(s));
            s.ask(Pending::DeclareAttackers {
                attacker: Some(slot),
            });
            Ok(())
        }
        Opt::Defender(target) => {
            let Pending::DeclareAttackers {
                attacker: Some(attacker),
            } = *s.pending()
            else {
                unreachable!("an attack target without an attacker");
            };
            let target = target.map_or(
                AttackTarget::Player(defender(s)),
                AttackTarget::Planeswalker,
            );
            s.choose_attacked(attacker, target);
            s.ask(Pending::DeclareAttackers { attacker: None });
            Ok(())
        }
        _ => finish_attack(s, t, sink),
    }
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
/// names only the declared attackers ([CR 508.4]): creatures put onto the battlefield attacking,
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
        event::set_tapped(s, t, sink, s.object_ref(slot), true)
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
    let declared = s.declare_attack(attackers);
    event::settle(s, t, sink, declared)
}

/// Whether `blocker` may block `attacker`: an attacker attacking the defending player or a
/// planeswalker they control (one attacking nothing can't be blocked), within its evasion:
/// flying and its landwalk ([`landwalks`]).
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "For each of the chosen
/// creatures, the defending player chooses one creature for it to block that's attacking that
/// player, a planeswalker they control, or a battle they protect."
///
/// [CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b): "A creature with flying can't
/// be blocked except by creatures with flying and/or reach."
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
            s.permanents().any(|(_, d)| {
                d.controller == defending && d.types.contains(Types::LAND) && has.of(d.subtypes)
            })
        })
}

/// Whether a creature may still be declared as a blocker: an untapped creature of the
/// defending player that blocks nothing yet and can block some attacker.
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "The defending player chooses
/// which creatures they control, if any, will block. The chosen creatures must be untapped and they
/// can't also be battles."
fn may_block<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let o = s.object(slot);
    s.defending_players().contains(s.controller(slot))
        && is_creature(s, slot)
        && !o.is_tapped()
        && !o.is_blocking()
        && s.battlefield().iter().any(|&a| can_block(s, t, slot, a))
}

/// Whether the defending player has a creature that can block now, so that declaring blockers
/// is a decision.
pub(crate) fn can_declare_blockers<L: Layout>(s: &State<L>, t: &GameTable) -> bool {
    s.battlefield().iter().any(|&slot| may_block(s, t, slot))
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
    let player = defender(s);
    if let Some(blocker) = blocker {
        let attackers = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&a| can_block(s, t, blocker, a))
            .collect();
        return Ok(Decision {
            player,
            kind: DecisionKind::BlockTarget,
            candidates: Candidates::Objects(collapse(s, attackers)),
            // The creature just chosen is the one being aimed.
            prompt: Prompt::new(Purpose::Block).part(Part::Aim),
            about: Bitboard::single(blocker),
        });
    }
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    list.push(Opt::Done)?;
    let able = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&slot| may_block(s, t, slot))
        .collect();
    // Candidates are listed in the candidate order, which a `Choice` counts in.
    for slot in s.in_candidate_order(collapse(s, able)) {
        list.push(Opt::Block(slot))?;
    }
    Ok(Decision {
        player,
        kind: DecisionKind::Blocker,
        candidates: Candidates::Options(list),
        prompt: Prompt::new(Purpose::Block),
        about: Bitboard::EMPTY,
    })
}

/// Declares a blocker, whose attacker is the next pick.
///
/// [CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g): "Each chosen creature still
/// controlled by the defending player becomes a blocking creature."
pub(crate) fn block_with<L: Layout>(s: &mut State<L>, blocker: L::Slot) {
    s.declare_blocker(blocker);
    s.ask(Pending::DeclareBlockers {
        blocker: Some(blocker),
    });
}

/// Has the blocker just declared block `attacker`.
///
/// [CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h): "An attacking creature with one
/// or more creatures declared as blockers for it becomes a blocked creature". It becomes blocked as
/// soon as its first blocker is chosen, so a blocked and an unblocked attacker are never offered as
/// interchangeable to the next blocker.
pub(crate) fn block_attacker<L: Layout>(s: &mut State<L>, attacker: L::Slot) -> Result<(), Full> {
    let Pending::DeclareBlockers {
        blocker: Some(blocker),
    } = *s.pending()
    else {
        unreachable!("a block target without a blocker");
    };
    s.block(blocker, attacker)?;
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
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        let (attacker, walker) = match *s.object(slot).combat() {
            CombatRole::None => continue,
            CombatRole::Attacking {
                target: AttackTarget::Planeswalker(walker),
                ..
            } => (true, Some(walker)),
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
        if !is_creature(s, slot) || changed(s.controller(slot)) {
            s.remove_from_combat(slot);
        } else if let Some(walker) = walker
            && (!defending.contains(s.controller(walker))
                || !s.derived(walker).types.contains(Types::PLANESWALKER))
        {
            s.forget_attacked(slot);
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

/// Ends combat ([CR 511.3]): "As soon as the end of combat step ends, all creatures, battles, and
/// planeswalkers are removed from combat."
///
/// [CR 511.3]: https://yawgatog.com/resources/magic-rules/#R5113
pub(crate) fn end_combat<L: Layout>(s: &mut State<L>) {
    s.end_combat();
}
