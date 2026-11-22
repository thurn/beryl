//! Damage ([CR 120](https://yawgatog.com/resources/magic-rules/#R120)).
//!
//! Implements: CR 120, CR 609.7a, CR 615.3, CR 702.15b, CR 702.15d

use beryl_card::prompt::Purpose;
use beryl_card::{Amount, Filter, Keywords, Prevention, Prompt, Recipients, Words};

use crate::bitboard::Bitboard;
use crate::card::Stop;
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::game::Game;
use crate::game::life;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::replacement::event::Recipient;
use crate::state::{
    ChosenSource, Continuous, Duration, ObjectRef, PendingTrigger, Shield, StackEntry, Subject,
    ZoneKey,
};
use crate::{continuous, continuous::rule_change, interpret::choices, replacement};

/// Deals `amount` damage from `source` to a player or a permanent, combat damage with
/// `combat` ([CR 120.3](https://yawgatog.com/resources/magic-rules/#R1203)). A permanent that
/// has left the battlefield is dealt none.
///
/// [CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a): "Damage dealt to a player
/// by a source without infect causes that player to lose that much life."
///
/// [CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c): "Damage dealt to a
/// planeswalker causes that many loyalty counters to be removed from that planeswalker."
///
/// [CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e): "Damage dealt to a
/// creature by a source with neither wither nor infect causes that much damage to be marked on
/// that creature."
///
/// The damage is proposed first and its replacement and prevention effects applied
/// ([`replacement::damage`]); damage they prevent is not dealt, and no lifelink comes of it.
/// Returns how much was dealt, after them (see [CR 615.1]): prevention effects "apply to damage
/// and modify how much damage, if any, is dealt", so "damage dealt this way" counts none
/// that was prevented.
///
/// [CR 615.1]: https://yawgatog.com/resources/magic-rules/#R6151
#[inline]
pub fn deal<L: Layout>(
    game: &mut Game<'_, L>,
    source: ObjectRef<L>,
    to: Recipient<L>,
    amount: u8,
    combat: bool,
) -> Result<u8, Full> {
    if amount == 0 {
        return Ok(0);
    }
    let proposal = replacement::Damage {
        source,
        to,
        amount,
        combat,
        preventable: true,
    };
    let replacement::Damage {
        source,
        to,
        amount,
        combat,
        ..
    } = replacement::damage(game, proposal)?;
    if amount == 0 {
        return Ok(0);
    }
    // Read before the damage: a player's lower life total may make a static ability's layer
    // results stale (`Inputs::LIFE`), and lifelink uses the source as it was when it dealt the
    // damage.
    let lifelink = lifelink(game, source);
    let e = match to {
        // A player whose life total can't change is still dealt the damage, and loses no life
        // (see [CR 119.8](https://yawgatog.com/resources/magic-rules/#R1198)): "If an effect says
        // that a player can't lose life, that player can't make an exchange such that the
        // player's life total would become lower".
        Recipient::Player(p) => {
            let loses_life = rule_change::life_can_change(game.state, game.table, p);
            game.state
                .damage_player(source, p, amount, combat, loses_life)
        }
        Recipient::Permanent(r) => {
            let Some(permanent) = game.state.permanent(r) else {
                return Ok(0);
            };
            game.state.mark_damage(source, permanent, amount, combat)
        }
    };
    game.settle(e)?;
    if let Some(controller) = lifelink {
        life::gain(game, controller, amount)?;
    }
    Ok(amount)
}

/// Who gains the life a source's damage gives for its lifelink, if it has lifelink
/// ([CR 702.15b]): "Damage dealt by a source with lifelink causes that source's controller, or
/// its owner if it has no controller, to gain that much life (in addition to any other results
/// that damage causes)." The gain is part of the damage event, not a trigger, so it follows the
/// damage at once.
///
/// Per [CR 702.15d], "The lifelink rules function no matter what zone an object with lifelink deals
/// damage from." A permanent's keywords are its layer results; anywhere else, those of the face
/// it has there. A source that has left its zone since is a new object (see CR 400.7) whose keywords
/// are not kept, so it gives none.
///
/// [CR 702.15b]: https://yawgatog.com/resources/magic-rules/#R70215b
/// [CR 702.15d]: https://yawgatog.com/resources/magic-rules/#R70215d
#[inline]
fn lifelink<L: Layout>(game: &Game<'_, L>, source: ObjectRef<L>) -> Option<Seat> {
    let s = game.state();
    if !s.is_current(source) {
        return None;
    }
    s.keywords(game.table(), source.slot)
        .contains(Keywords::LIFELINK)
        .then(|| s.controller(source.slot))
}

/// [`Effect::DealDamage`](beryl_card::Effect::DealDamage): "This deals [amount] damage to
/// [recipients]": noncombat damage from the source of what is resolving
/// ([CR 120.2b](https://yawgatog.com/resources/magic-rules/#R1202b)), to each of the players
/// in turn order, then to each of the permanents in the order they entered the battlefield.
/// For "any target" ([`Recipients::Both`] of one target) only one of the two is there. The
/// outcome is the permanents dealt damage, and how much was dealt in all.
///
/// # Panics
///
/// For an emblem's ability, which has no source.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    to: Recipients,
    amount: Amount,
) -> Result<Outcome<L>, Full> {
    let (players, permanents) = match to {
        Recipients::Players(players) => (Some(players), None),
        Recipients::Permanents(objects) => (None, Some(objects)),
        Recipients::Both(players, objects) => (Some(players), Some(objects)),
        Recipients::DividedTwo(spec) => return divided_two(run, spec),
    };
    let amount = run.scene().amount(amount);
    let source = run.resolution().source();
    let (mut damaged, mut dealt) = (Bitboard::EMPTY, 0);
    if let Some(players) = players {
        for player in run.in_turn_order(players) {
            dealt += u32::from(deal(
                run.game(),
                source,
                Recipient::Player(player),
                amount,
                false,
            )?);
        }
    }
    if let Some(objects) = permanents {
        let mut left = run.permanents(objects);
        damaged = left.slots();
        while let Some(permanent) = run.next_permanent(&mut left) {
            let to = Recipient::Permanent(permanent.into());
            dealt += u32::from(deal(run.game(), source, to, amount, false)?);
        }
    }
    Ok(Outcome::of(damaged, dealt))
}

/// Deals the shares determined by the number of targets chosen while casting.
/// (see [CR 601.2d]): "Each of these targets must receive at least one of whatever is being
/// divided." With two points the number of chosen targets determines the only legal
/// division. Count those targets before checking which are still legal (see CR 608.2b).
///
/// [CR 601.2d]: https://yawgatog.com/resources/magic-rules/#R6012d
#[cold]
#[inline(never)]
fn divided_two<L: Layout>(
    run: &mut Run<'_, '_, L>,
    spec: beryl_card::SpecIx,
) -> Result<Outcome<L>, Full> {
    let resolution = run.resolution();
    let source = resolution.source();
    let count = resolution
        .entry
        .targets
        .iter()
        .filter(|target| target.spec() == spec)
        .count();
    let amount = if count == 1 { 2 } else { 1 };
    let mut targets = [None; 2];
    for (place, target) in targets.iter_mut().zip(run.scene().legal_targets(spec)) {
        *place = Some(target);
    }
    let mut damaged = Bitboard::EMPTY;
    let mut dealt = 0;
    for target in targets.into_iter().flatten() {
        let to = if let Some(player) = target.as_player() {
            Recipient::Player(player)
        } else {
            let object = target.as_object().expect("a permanent target");
            damaged.insert(object.slot);
            Recipient::Permanent(object)
        };
        dealt += u32::from(deal(run.game(), source, to, amount, false)?);
    }
    Ok(Outcome::of(damaged, dealt))
}

/// [`Effect::PreventDamage`](beryl_card::Effect::PreventDamage): a prevention shield on each
/// of the recipients until end of turn, players in turn order and then permanents in the
/// order they entered the battlefield ([CR 615.3]): "Such effects last until they're used up
/// or their duration has expired." For "a [filter] source of your choice" the controller
/// chooses the source first, among the eligible objects the filter accepts ([CR 609.7a]):
/// "The source is chosen when the effect is created." With none to choose, no shield begins.
/// The outcome is the permanents shielded.
///
/// [CR 615.3]: https://yawgatog.com/resources/magic-rules/#R6153
/// [CR 609.7a]: https://yawgatog.com/resources/magic-rules/#R6097a
pub(crate) fn prevent_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    to: Recipients,
    prevention: Prevention,
    from: Option<&'static Filter>,
) -> Result<Outcome<L>, Stop> {
    let chosen = match from {
        None => None,
        Some(filter) => {
            let (game, resolution) = run.resolving();
            let controller = resolution.controller();
            let words = resolution.words(game);
            let candidates = sources(game, &resolution.entry, filter, controller, &words);
            if candidates.is_empty() {
                return Ok(Outcome::NOTHING);
            }
            let prompt = Prompt::new(Purpose::Choose);
            let slot = choices::pick(game, resolution, controller, candidates, prompt)?;
            Some(ChosenSource {
                object: game.state().object_ref(slot),
                face: resolution.entry.face(),
                ability: resolution.entry.ability(),
                colors: words.color_changes(),
            })
        }
    };
    let left = match prevention {
        Prevention::NextTime => None,
        Prevention::Next(n) => Some(n),
    };
    let shield = Shield { left, from: chosen };
    let (players, permanents) = match to {
        Recipients::Players(players) => (Some(players), None),
        Recipients::Permanents(objects) => (None, Some(objects)),
        Recipients::Both(players, objects) => (Some(players), Some(objects)),
        Recipients::DividedTwo(_) => unreachable!("division is only used to deal damage"),
    };
    if let Some(players) = players {
        for player in run.in_turn_order(players) {
            let (game, resolution) = run.resolving();
            let effect = Continuous::Prevent(Subject::Player(player), shield);
            continuous::start(game, resolution, effect, Duration::EndOfTurn)?;
        }
    }
    let Some(objects) = permanents else {
        return Ok(Outcome::NOTHING);
    };
    let shielded = run.permanents(objects);
    let mut left = shielded;
    while let Some(permanent) = run.next_permanent(&mut left) {
        let (game, resolution) = run.resolving();
        let effect = Continuous::Prevent(Subject::Object(permanent.into()), shield);
        continuous::start(game, resolution, effect, Duration::EndOfTurn)?;
    }
    Ok(Outcome::acted_on(shielded.slots()))
}

/// The eligible damage sources `filter` accepts, read for `you` through `words`
/// ([CR 609.7a]): "they may choose a permanent; a spell on the stack (including a permanent
/// spell); any object referred to by an object on the stack, by a replacement or prevention
/// effect that's waiting to apply, or by a delayed triggered ability that's waiting to trigger".
/// Include the resolving entry, which the engine takes off its stack queue to run its body.
/// A slot-based choice can show only a current object. A stale reference never nominates the
/// new object in its slot (see CR 400.7).
///
/// [CR 609.7a]: https://yawgatog.com/resources/magic-rules/#R6097a
fn sources<L: Layout>(
    game: &Game<'_, L>,
    resolving: &StackEntry<L>,
    filter: &Filter,
    you: Seat,
    words: &Words,
) -> Bitboard<L::Slot> {
    let s = game.state();
    let mut candidates: Bitboard<L::Slot> = s.battlefield().iter().copied().collect();
    candidates |= s.zone(ZoneKey::Stack);
    let referred = s
        .stack()
        .iter()
        .chain(std::iter::once(resolving))
        .flat_map(StackEntry::object_references)
        .chain(
            s.delayed()
                .iter()
                .flat_map(PendingTrigger::object_references),
        )
        .chain(s.replacement_references());
    for object in referred.filter(|&object| s.is_current(object)) {
        candidates.insert(object.slot);
    }
    filter.select(s, game.table(), candidates, you, words)
}
