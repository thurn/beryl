//! Damage proposed before it is dealt, and the replacement and prevention effects that
//! modify it ([CR 616.1]): protection, prevention shields from the Effects store, and static
//! abilities that multiply damage.
//!
//! Implements: CR 615.6, CR 615.9, CR 615.12, CR 702.16e, CR 702.16j
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use crate::card::{Ability, AbilityKind, Effect, ObjectAbility, Replaced, Replacement, Rewrite};
use crate::continuous::rule_change;
use crate::decision::{Decision, DecisionKind, OPTIONS, Opt};
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{Recipient, ReplacementKind};
use crate::replacement::protection;
use crate::state::{ChosenSource, ObjectRef, ShieldAt, State, Subject, Zone, ZoneKey};
use crate::table::GameTable;

use beryl_card::prompt::Purpose;
use beryl_card::{Filter, Prompt, Words};

/// Damage proposed before it is dealt: the proposal that prevention and replacement effects
/// of damage rewrite ([`damage`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Damage<L: Layout> {
    pub source: ObjectRef<L>,
    pub to: Recipient<L>,
    pub amount: u8,
    /// It is combat damage (see [CR 510.2]).
    ///
    /// [CR 510.2]: https://yawgatog.com/resources/magic-rules/#R5102
    pub combat: bool,
    /// It can be prevented: no effect says it can't ([CR 615.12]), "Some effects state that
    /// damage “can’t be prevented.” If unpreventable damage would be dealt, any applicable
    /// prevention effects are still applied to it. Those effects won’t prevent any damage,
    /// but any additional effects they have will take place. Existing damage prevention
    /// shields won’t be reduced by damage that can’t be prevented." No prevention effect here
    /// has an additional effect, so none is applied to it.
    ///
    /// [CR 615.12]: https://yawgatog.com/resources/magic-rules/#R61512
    pub preventable: bool,
}

/// Applies the replacement and prevention effects of damage to `proposal`: protection
/// ([CR 702.16e]), "Any damage that would be dealt by sources that have the stated quality to
/// a permanent or player with protection is prevented"; the prevention shields on the
/// recipient; and static abilities that multiply damage. Prevented damage is never dealt
/// ([CR 615.6]): "If damage that would be dealt is prevented, it never happens." A shield is
/// used up as it applies.
///
/// Gated on the kind: while no effect gives protection, no permanent replaces damage and no
/// shield is on anything, the proposal is returned as built after two ANDs.
///
/// [`Full`] if the layout's scratch lists fill or a choice of order stops the run
/// ([`super::answer`]).
///
/// [CR 702.16e]: https://yawgatog.com/resources/magic-rules/#R70216e
/// [CR 615.6]: https://yawgatog.com/resources/magic-rules/#R6156
#[inline]
pub(crate) fn damage<L: Layout>(
    game: &mut Game<'_, L>,
    proposal: Damage<L>,
) -> Result<Damage<L>, Full> {
    let s = game.state();
    let live = s
        .replacing()
        .is_some_and(|r| r.protects() || r.replaces(ReplacementKind::Damage));
    if !live && !s.has_prevention_shields() && !player_protected(s, game.table(), &proposal) {
        return Ok(proposal);
    }
    replaced(game, proposal)
}

/// Whether the damage would be dealt to a player with protection from everything, which
/// prevents it ([CR 702.16j]): "All damage that would be dealt to such a permanent or player is
/// prevented." One AND while no card in the game gives a player protection.
///
/// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
#[inline]
fn player_protected<L: Layout>(s: &State<L>, t: &GameTable, p: &Damage<L>) -> bool {
    matches!(p.to, Recipient::Player(player) if rule_change::protected_from_everything(s, t, player))
}

/// A replacement or prevention effect of damage, by where it comes from: what [CR 614.5]
/// counts as having applied. "A replacement effect doesn't invoke itself repeatedly; it gets
/// only one opportunity to affect an event or any modified events that may replace that
/// event."
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key<S> {
    /// The recipient's protection.
    Protection,
    /// A shield of the Effects store, by its timestamp.
    Shield(u16),
    /// A static ability of a permanent, which may be one of a card under it.
    Ability(S, ObjectAbility),
}

/// What an effect that applies does to the damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Change {
    /// Prevents all of it: protection, or a shield for the next time (see [CR 615.8]): "These
    /// effects prevent the next instance of damage from that source, regardless of how much
    /// damage that is."
    ///
    /// [CR 615.8]: https://yawgatog.com/resources/magic-rules/#R6158
    PreventAll,
    /// Prevents up to what is left of a shield of an amount (see [CR 615.7]).
    ///
    /// [CR 615.7]: https://yawgatog.com/resources/magic-rules/#R6157
    PreventSome,
    /// Multiplies it by N.
    Multiply(u8),
}

/// An effect that applies to the damage now: where it comes from, what it does, the shield's
/// place in the store for a shield, and the object a player choosing an order is shown for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Candidate<S> {
    key: Key<S>,
    change: Change,
    at: u8,
    shown: Option<S>,
}

/// The scratch lists in [`replaced`] hold this many effects in `Small` and grow in `Large`.
const MOST: usize = 16;

type Candidates<L> = <L as Layout>::Queue<Option<Candidate<<L as Layout>::Slot>>, MOST>;

/// [`damage`] once some effect may apply: the loop of [CR 616.1]: "If two or more replacement
/// and/or prevention effects are attempting to modify the way an event affects an object or
/// player, the affected object's controller (or its owner if it has no controller) or the
/// affected player chooses one to apply". The effects that apply and have not applied are
/// collected, one is applied, and they are collected again ([CR 616.1f]): "Once the chosen
/// effect has been applied, this process is repeated (taking into account only replacement or
/// prevention effects that would now be applicable) until there are no more left to apply."
/// Once no damage is left, nothing is left to modify. None of these is a self-replacement
/// effect or changes control, copies or turns a face (CR 616.1a to 616.1d), so any may be
/// chosen ([CR 616.1e]): "Any of the applicable replacement and/or prevention effects may be
/// chosen."
///
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 616.1f]: https://yawgatog.com/resources/magic-rules/#R6161f
/// [CR 616.1e]: https://yawgatog.com/resources/magic-rules/#R6161e
#[cold]
#[inline(never)]
fn replaced<L: Layout>(game: &mut Game<'_, L>, mut p: Damage<L>) -> Result<Damage<L>, Full> {
    let mut applied = L::Queue::<Option<Key<L::Slot>>, MOST>::default();
    while p.amount > 0 {
        let found = candidates(game.state(), game.table(), &p, applied.as_slice())?;
        let Some(c) = choose(game, &p, found.as_slice())? else {
            break;
        };
        applied.push(Some(c.key))?;
        p.amount = match c.change {
            Change::PreventAll if matches!(c.key, Key::Protection) => 0,
            Change::PreventAll | Change::PreventSome => {
                p.amount - game.state.prevent_with(c.at, p.amount)
            }
            Change::Multiply(n) => p.amount.saturating_mul(n),
        };
    }
    Ok(p)
}

/// The effects that apply to `p` now and are not among `applied`, protection first, then the
/// shields in the order they began, then the static abilities in the order of the
/// battlefield.
fn candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    p: &Damage<L>,
    applied: &[Option<Key<L::Slot>>],
) -> Result<Candidates<L>, Full> {
    let mut found = Candidates::<L>::default();
    let mut add = |c: Candidate<L::Slot>| {
        if applied.contains(&Some(c.key)) {
            Ok(())
        } else {
            found.push(Some(c))
        }
    };
    if p.preventable {
        let permanent_protected = |r: ObjectRef<L>| {
            protection::live(s) && protection::protected_from_cold(s, t, r.slot, p.source.slot)
        };
        let protected = match p.to {
            Recipient::Permanent(r) => permanent_protected(r),
            Recipient::Player(_) => player_protected(s, t, p),
        };
        if protected {
            add(Candidate {
                key: Key::Protection,
                change: Change::PreventAll,
                at: 0,
                shown: None,
            })?;
        }
        if s.has_prevention_shields() {
            for shield in s.shields_on(subject(p.to)) {
                if applies(s, t, p, shield) {
                    add(Candidate {
                        key: Key::Shield(shield.timestamp),
                        change: match shield.shield.left {
                            Some(_) => Change::PreventSome,
                            None => Change::PreventAll,
                        },
                        at: shield.at,
                        shown: shown(s, t, shield)?,
                    })?;
                }
            }
        }
    }
    if s.replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Damage))
    {
        for &slot in s.battlefield() {
            if !super::may_replace(s, t, slot, ReplacementKind::Damage) {
                continue;
            }
            let multiplies = |a: &Ability, r: &Replacement| match *r {
                Replacement {
                    replaces: Replaced::Damage,
                    rewrite: Rewrite::Multiply(n),
                } if a.kind() == AbilityKind::Replacement => Some(n),
                _ => None,
            };
            for (ability, a, n) in super::statics(s, t, slot, multiplies) {
                crate::coverage::hit(a);
                add(Candidate {
                    key: Key::Ability(slot, ability),
                    change: Change::Multiply(n),
                    at: 0,
                    shown: Some(slot),
                })?;
            }
        }
    }
    Ok(found)
}

/// The effect of `found` to apply next, or none when none applies.
///
/// One that prevents all the damage is applied first, protection before a shield, without
/// asking: every order then ends with no damage dealt, and this one uses up the fewest
/// shields, which is what the affected player would choose. Shields of an amount are applied
/// in the order they began, and multipliers in the order found, which give the same damage in
/// any order. The one choice that changes how much damage is dealt is a multiplier against a
/// shield of an amount (Furnace of Rath's ruling: "That player can either (a) prevent 4
/// damage first and then let Furnace of Rath double the remaining 1 damage, taking 2 damage,
/// or (b) double the damage to 10 and then prevent 4 damage, taking 6 damage."): the affected
/// player is asked which applies first, the shield first by default. A shield whose card is
/// in no public zone cannot be shown, and is applied first without asking.
fn choose<L: Layout>(
    game: &mut Game<'_, L>,
    p: &Damage<L>,
    found: &[Option<Candidate<L::Slot>>],
) -> Result<Option<Candidate<L::Slot>>, Full> {
    let found = || found.iter().flatten().copied();
    if let Some(all) = found().find(|c| c.change == Change::PreventAll) {
        return Ok(Some(all));
    }
    let shield = found().find(|c| c.change == Change::PreventSome);
    let multiplier = found().find(|c| matches!(c.change, Change::Multiply(_)));
    let (Some(shield), Some(multiplier)) = (shield, multiplier) else {
        return Ok(found().next());
    };
    let (Some(shown), Some(permanent)) = (shield.shown, multiplier.shown) else {
        return Ok(Some(shield));
    };
    let mut options = <L::Queue<Opt<L>, OPTIONS>>::default();
    options.push_bounded(Opt::Object(shown));
    options.push_bounded(Opt::Object(permanent));
    let d = Decision::options(
        affected(game.state(), p.to),
        DecisionKind::ChooseObject,
        options,
        Prompt::new(Purpose::Replace),
    );
    let c = super::answer(game.state, game.sink, &d)?;
    Ok(Some(match d.candidates.option(c) {
        Some(Opt::Object(slot)) if slot == permanent => multiplier,
        _ => shield,
    }))
}

/// Whether `shield` applies to `p`: a shield against any source does, and one for a chosen
/// source does to damage from that object while it still passes the filter it was chosen
/// with ([CR 615.9]): "When the source would deal damage, the shield rechecks the source's
/// properties. If the properties no longer match, the damage isn't prevented or replaced and
/// the shield isn't used up." The filter is read for the affected player.
///
/// [CR 615.9]: https://yawgatog.com/resources/magic-rules/#R6159
fn applies<L: Layout>(s: &State<L>, t: &GameTable, p: &Damage<L>, shield: ShieldAt<L>) -> bool {
    let Some(chosen) = shield.shield.from else {
        return true;
    };
    // A shield outlives the object that made it, so its filter is read through the color
    // words it kept.
    let words = Words::of_color_changes(chosen.colors);
    chosen.object == p.source
        && s.is_current(p.source)
        && chosen_with(t, shield.source, chosen)
            .is_none_or(|filter| filter.matches(s, t, p.source.slot, affected(s, p.to), &words))
}

/// The filter a source was chosen with for a shield of the card `card`: the `from` of the
/// prevention instruction in the ability that made it.
fn chosen_with<L: Layout>(
    t: &GameTable,
    card: L::Card,
    chosen: ChosenSource<L>,
) -> Option<&'static Filter> {
    let face = t.def(card).face(chosen.face);
    let ability = match chosen.ability {
        Some(ability) => face.ability(ability),
        None => face
            .abilities
            .iter()
            .find(|a| a.kind() == AbilityKind::Spell)?,
    };
    let crate::card::Behavior::Data(effect) = ability.body() else {
        return None;
    };
    prevention_filter(effect)
}

/// The filter of the first prevention instruction in `effect` that names one.
fn prevention_filter(effect: &'static Effect) -> Option<&'static Filter> {
    match *effect {
        Effect::PreventDamage {
            from: Some(filter), ..
        } => Some(filter),
        Effect::Sequence(effects) => effects.iter().find_map(prevention_filter),
        Effect::If {
            then, otherwise, ..
        } => prevention_filter(then).or_else(|| otherwise.and_then(prevention_filter)),
        Effect::May { effect, .. }
        | Effect::Unless { effect, .. }
        | Effect::ForEachPlayer { effect, .. } => prevention_filter(effect),
        Effect::Choose { .. }
        | Effect::LookAt { .. }
        | Effect::Counter(_)
        | Effect::CounterInto { .. }
        | Effect::CopySpell { .. }
        | Effect::Destroy { .. }
        | Effect::Regenerate(_)
        | Effect::PhaseOut(_)
        | Effect::Exile(_)
        | Effect::ExileUntilThisLeaves(_)
        | Effect::ExileLinked(_)
        | Effect::Sacrifice(_)
        | Effect::ReturnToHand(_)
        | Effect::PutIntoGraveyard(_)
        | Effect::ReturnToBattlefield(_)
        | Effect::PutOntoBattlefield(_)
        | Effect::ReturnTransformed(_)
        | Effect::PutIntoLibrary { .. }
        | Effect::PutBack { .. }
        | Effect::ShuffleInto { .. }
        | Effect::Dig { .. }
        | Effect::ExileUntil { .. }
        | Effect::Fateseal { .. }
        | Effect::Search { .. }
        | Effect::Draw { .. }
        | Effect::Discard { .. }
        | Effect::Mill { .. }
        | Effect::Scry { .. }
        | Effect::Explore(_)
        | Effect::ManifestDread(_)
        | Effect::FlipCoin(_)
        | Effect::DealDamage { .. }
        | Effect::PreventDamage { .. }
        | Effect::GainLife { .. }
        | Effect::LoseLife { .. }
        | Effect::CreateTokens { .. }
        | Effect::CreateAttackingTokens { .. }
        | Effect::CreateCopies { .. }
        | Effect::BecomeCopy { .. }
        | Effect::Mutate(_)
        | Effect::AddCounters { .. }
        | Effect::RemoveCounters { .. }
        | Effect::AddMana { .. }
        | Effect::AddManaOfKind { .. }
        | Effect::Modify { .. }
        | Effect::ChangeText { .. }
        | Effect::ExchangeControl(..)
        | Effect::Attach { .. }
        | Effect::ChangeRule { .. }
        | Effect::GainFlashback(_)
        | Effect::PermitCast { .. }
        | Effect::MayCast { .. }
        | Effect::UntapUpTo { .. }
        | Effect::Delay { .. }
        | Effect::GetEmblem { .. }
        | Effect::WinGame(_)
        | Effect::LoseGame(_)
        | Effect::ControlPlayer(_)
        | Effect::EndTurn
        | Effect::ExtraTurn(_) => None,
    }
}

/// The object a player choosing an order is shown for a shield: its card, while that card is
/// in a public zone.
fn shown<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    shield: ShieldAt<L>,
) -> Result<Option<L::Slot>, Full> {
    let mut public = s.zone(ZoneKey::Stack);
    for player in s.players().in_seat_order() {
        for zone in [Zone::Battlefield, Zone::Graveyard, Zone::Exile] {
            public |= s.zone(ZoneKey::Of(player, zone));
        }
    }
    let copies = public.filter(|slot| s.card_behind(slot) == Some(shield.source));
    Ok(s.in_game_order(t, copies)?.as_slice().first().copied())
}

/// What a shield on the recipient is on.
fn subject<L: Layout>(to: Recipient<L>) -> Subject<L> {
    match to {
        Recipient::Player(player) => Subject::Player(player),
        Recipient::Permanent(r) => Subject::Object(r),
    }
}

/// The player who chooses the order of the effects on damage to `to` (CR 616.1): the player
/// dealt it, or the controller of the permanent dealt it.
fn affected<L: Layout>(s: &State<L>, to: Recipient<L>) -> Seat {
    match to {
        Recipient::Player(player) => player,
        Recipient::Permanent(r) => s.controller(r.slot),
    }
}
