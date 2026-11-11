//! Derived characteristics: continuous effects applied in layers, recomputed from base when
//! dirty (architecture §9, decision D5, CR 613).

use beryl_kernel::derive::Rederive;

use crate::card::{
    AbilityKind, Affected, CastingKind, Colors, Modification, Rules, StaticCondition, Subtypes,
    Types,
};
use crate::event::{Sink, kind};
use crate::filter;
use crate::ids::CardId;
use crate::layout::{Layout, PerSeat, Queue};
use crate::state::{Affects, CounterKind, Derived, GrantRef, LayerChange, State, Zone};
use crate::table::GameTable;
use crate::trigger::face;

/// One continuous effect to apply in this recompute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Apply<C: CardId> {
    layer: u8,
    timestamp: u16,
    /// Static abilities on one object share its timestamp; their order on the card breaks
    /// the tie.
    order: u8,
    /// The battlefield index of the permanent it affects.
    target: u8,
    /// The game index of the effect's source card, which a granted ability is printed on.
    source: C,
    change: Option<LayerChange>,
}

/// The effects one recompute applies. A list longer than its capacity would need more
/// effects than the cards can create at once.
type Applying<L> = <L as Layout>::Queue<Apply<<L as Layout>::Card>, 192>;

/// Recomputes `derived` if anything changed since the last recompute. Every chokepoint that
/// mutates a layer input sets `derived_dirty`, and every path that reads `derived` after one
/// calls this first, so reads never see stale values and never recompute.
pub(crate) fn refresh<L: Layout>(s: &mut State<L>, t: &GameTable, sink: &mut Sink<L>) {
    if s.is_derived_stale() {
        sink.work.layer_recomputes += 1;
        recompute(s, t);
    }
}

/// The kinds of event the permanents on the battlefield have triggered abilities for. A
/// face-down permanent has none, whichever card it is
/// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)): it has "no text, no
/// name, no subtypes, and no mana cost". So it counts for nothing
/// ([`crate::query::Copiable::may_listen`]), and masking or determinizing its hidden
/// identity leaves this unchanged.
pub(crate) fn listening<L: Layout>(s: &State<L>, t: &GameTable) -> u16 {
    s.battlefield()
        .iter()
        .fold(0, |k, &slot| k | s.copiable(slot).may_listen(t))
}

/// Whether a change in the number of `kind` counters on the object in `slot` can change its
/// derived characteristics, so that the layers must run again. Only +1/+1 and -1/-1 counters
/// modify power and toughness
/// ([CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c)): "Effects and counters
/// that modify power and/or toughness ... are applied". Any other kind matters only to a
/// static ability of its own that applies while it has some: loyalty, which planeswalkers
/// gain and lose every turn, never does.
pub(crate) fn counters_matter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    kind: CounterKind,
) -> bool {
    matches!(kind, CounterKind::PlusOne | CounterKind::MinusOne)
        || s.object(slot).zone() != Zone::Battlefield
        || face(s, t, slot).abilities.iter().any(|a| {
            a.layer.is_some_and(|spec| match spec.condition {
                StaticCondition::Always => false,
                StaticCondition::HasCounter(k) => k == kind,
                StaticCondition::Impending => kind == CounterKind::Time,
            })
        })
}

/// Recomputes every permanent's characteristics from base
/// ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131)): "The values of an
/// object's characteristics are determined by starting with the actual object. For a card,
/// that means the values of the characteristics printed on that card. For a token or a copy
/// of a spell or card, that means the values of the characteristics defined by the effect
/// that created it. Then all applicable continuous effects are applied in a series of layers
/// in the following order".
///
/// Within a layer, effects apply in timestamp order
/// ([CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137)): "An effect with an earlier
/// timestamp is applied before an effect with a later timestamp." The only
/// characteristic-defining ability, Adeline's, sits alone in layer 7a, so "apply effects from
/// characteristic-defining abilities first" (CR 613.3) holds by the layer order. No two of the
/// cards' effects in one layer depend on each other
/// ([CR 613.8](https://yawgatog.com/resources/magic-rules/#R6138)), so timestamps decide.
///
/// The kernel has ended the effects on objects that left their zone as they left, and stores
/// the results, with what each face-down permanent's controller now sees, as the
/// recompute finishes ([`beryl_kernel::derive::Rederive::finish`]), and with them the rules
/// the permanents' static abilities change for each player ([`player_rules`]).
fn recompute<L: Layout>(s: &mut State<L>, t: &GameTable) {
    let Some(mut s) = s.rederive() else {
        return;
    };
    let listening = listening(&s, t);

    // Made at the first effect that applies: most recomputes have none, and the list is big.
    let mut list: Option<Applying<L>> = None;
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        let o = *s.object(slot);
        // Layer 1: the copiable values (CR 613.2a). A token copy's are those of what it
        // copies, with the 1/1 exception of CR 707.9b.
        let what = s.copiable(slot);
        let f = what.face(t);
        s.results()[i] = Derived {
            power: what.power(t),
            toughness: what.toughness(t),
            types: f.types,
            supertypes: f.supertypes,
            colors: f.colors,
            controller: s.base_controller(slot),
            protection: Colors::NONE,
            keywords: f.keywords,
            subtypes: f.subtypes,
            granted: [None; 4],
        };
        for (n, ability) in f.abilities.iter().enumerate() {
            let Some(spec) = ability.layer else { continue };
            let applies = match spec.condition {
                StaticCondition::Always => true,
                StaticCondition::HasCounter(kind) => s.counters_of(slot, kind) > 0,
                StaticCondition::Impending => {
                    s.counters_of(slot, CounterKind::Time) > 0
                        && s.paid(slot, CastingKind::Impending)
                }
            };
            let target = match spec.affected {
                Affected::Itself => Some(i),
                Affected::Enchanted => o.attached_to().and_then(|a| s.battlefield_position(a)),
            };
            if let (true, Some(target)) = (applies, target) {
                crate::coverage::hit(ability);
                // A list longer than its capacity would need more effects than the cards
                // can create at once.
                // No static ability of the cards changes control. One that did would name
                // its source's controller as layer 2 has it, not as the permanent entered.
                debug_assert_ne!(spec.modification, Modification::Control);
                let change = LayerChange::resolve(spec.modification, s.base_controller(slot));
                let _ = list.get_or_insert_with(Default::default).push(Apply {
                    layer: change.layer() as u8,
                    timestamp: o.timestamp(),
                    order: n as u8,
                    target: target as u8,
                    source: what.record(t).0,
                    change: Some(change),
                });
            }
        }
    }
    resolved_effects(&s, &mut list);
    let len = s.battlefield().len();
    if let Some(list) = &mut list {
        list.as_mut_slice()
            .sort_unstable_by_key(|a| (a.layer, a.timestamp, a.order, a.target));
        for a in list.iter() {
            let Some(change) = a.change else { continue };
            let i = usize::from(a.target);
            apply(s.results(), i, a.source, change);
        }
    }
    // Layer 7c: counters modify power and toughness
    // ([CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c)): "Effects and counters
    // that modify power and/or toughness (but don't set power and/or toughness to a specific
    // number or value) are applied." Additions commute, so they follow the effects.
    for i in 0..len {
        let c = s.object(s.battlefield()[i]).counters();
        let d = &mut s.results()[i];
        let delta = c.plus_one as i8 - c.minus_one as i8;
        d.power = d.power.saturating_add(delta);
        d.toughness = d.toughness.saturating_add(delta);
    }
    // A permanent that may change a rule for players said so in the mask just read for
    // the listeners: with none on the battlefield, one AND.
    let rules = if listening & kind::RULE_CHANGE != 0 {
        player_rules(&mut s, t)
    } else {
        [Rules::NONE; _]
    };
    s.finish(listening, rules);
}

/// Lists the continuous effects of resolved spells and abilities that change a permanent
/// on the battlefield now, for [`recompute`] to apply with the static abilities' in layer
/// and timestamp order. The kernel has ended those on objects that left their zone.
#[inline]
fn resolved_effects<L: Layout>(s: &Rederive<'_, L>, list: &mut Option<Applying<L>>) {
    for (e, change) in s.layer_effects() {
        let effect = |target: usize| Apply {
            layer: change.layer() as u8,
            timestamp: e.timestamp,
            order: 0,
            target: target as u8,
            source: e.source,
            change: Some(change),
        };
        match e.affected {
            Affects::Object { slot, .. } => {
                if s.object(slot).zone() != Zone::Battlefield {
                    continue;
                }
                if let Some(target) = s.battlefield_position(slot) {
                    let _ = list
                        .get_or_insert_with(Default::default)
                        .push(effect(target));
                }
            }
            // One effect on each permanent it locked in that is still the object it was
            // ([CR 611.2c](https://yawgatog.com/resources/magic-rules/#R6112c): "the set of
            // objects it affects is determined when that continuous effect begins. After
            // that point, the set won't change."), all with its one timestamp.
            Affects::Set(set) => {
                let members = s.locked_in(set);
                for target in 0..s.battlefield().len() {
                    if members.contains(s.battlefield()[target]) {
                        let _ = list
                            .get_or_insert_with(Default::default)
                            .push(effect(target));
                    }
                }
            }
            // A player is affected by a rule change alone.
            Affects::Player(_) => {}
        }
    }
}

/// The rules the permanents' static abilities change for each player, by seat
/// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some continuous
/// effects affect game rules rather than objects. ... These effects are applied after all
/// other continuous effects have been applied." So control is settled: "each opponent" and
/// "you" in such an ability are read for the permanent's controller as layer 2 left it
/// ([CR 109.5](https://yawgatog.com/resources/magic-rules/#R1095): "For a static ability,
/// this is the current controller of the object it's on."). A face-down permanent has no
/// abilities ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)), which its
/// copiable values answer for.
///
/// Out of line: only a recompute with such a permanent on the battlefield comes here.
#[inline(never)]
fn player_rules<L: Layout>(s: &mut Rederive<'_, L>, t: &GameTable) -> PerSeat<Rules> {
    let mut rules = [Rules::NONE; _];
    for i in 0..s.battlefield().len() {
        let what = s.copiable(s.battlefield()[i]);
        if what.player_rules(t).is_empty() {
            continue;
        }
        let controller = s.results()[i].controller;
        for ability in what.face(t).abilities {
            let (AbilityKind::Static, Some(whose)) = (ability.kind, ability.rule.whose()) else {
                continue;
            };
            // The rule is in force for those players from here: the ability did what it
            // says, as one that changes a permanent does above.
            crate::coverage::hit(ability);
            // Each player's set is a union: the order they are met in is nothing.
            for player in s.players_in_seat_order(s.players()) {
                if filter::is_whose(whose, player, controller) {
                    rules[player.index()] = rules[player.index()].with(ability.rule);
                }
            }
        }
    }
    rules
}

/// Applies one modification to the permanent at battlefield index `i`.
///
/// Out of line, as it was before it was generic over the layout: most recomputes apply
/// nothing, and inlined into [`recompute`] it costs the rollout benchmark 0.07% of its
/// instructions.
#[inline(never)]
fn apply<L: Layout>(derived: &mut [Derived<L>], i: usize, source: L::Card, change: LayerChange) {
    match change {
        // CR 613.1b: "Layer 2: Control-changing effects are applied."
        LayerChange::Control(player) => derived[i].controller = player,
        // CR 613.1d: "Layer 4: Type-changing effects are applied. These include effects that
        // change an object's card type, subtype, and/or supertype."
        LayerChange::AddTypes(types) => derived[i].types = derived[i].types.union(types),
        LayerChange::RemoveTypes(types) => derived[i].types = derived[i].types.minus(types),
        // CR 205.1a: "Some effects set an object's card type. In most such cases, the new
        // card type(s) replaces any existing card types. ... If an object's card type is
        // removed, the subtypes correlated with that card type will remain if they are also
        // the subtypes of a card type the object currently has; otherwise, they are also
        // removed".
        LayerChange::SetTypes(types) => {
            derived[i].types = types;
            let mut kept = Subtypes::NONE;
            for subtype in derived[i].subtypes.iter() {
                if types.intersects(subtype.card_type()) {
                    kept = kept.with(subtype);
                }
            }
            derived[i].subtypes = kept;
        }
        LayerChange::AddSubtype(subtype) => {
            derived[i].subtypes = derived[i].subtypes.with(subtype);
        }
        // CR 613.1f: "Layer 6: Ability-adding effects, keyword counters, ability-removing
        // effects, and effects that say an object can't have an ability are applied."
        LayerChange::AddKeywords(k) => derived[i].keywords = derived[i].keywords.union(k),
        LayerChange::Grant(grant) => {
            if let Some(free) = derived[i].granted.iter_mut().find(|g| g.is_none()) {
                *free = Some(GrantRef {
                    card: source,
                    grant,
                });
            }
        }
        // CR 613.4a: "Layer 7a: Effects from characteristic-defining abilities that define
        // power and/or toughness are applied." Layers 2 to 6 are done for every permanent by
        // now, so the count sees each one's final types and controller.
        LayerChange::PowerIsCreaturesYouControl => {
            let you = derived[i].controller;
            let creatures = derived
                .iter()
                .filter(|d| d.controller == you && d.types.contains(Types::CREATURE))
                .count();
            derived[i].power = i8::try_from(creatures).unwrap_or(i8::MAX);
        }
        LayerChange::ModifyPt(p, t) => {
            derived[i].power = derived[i].power.saturating_add(p);
            derived[i].toughness = derived[i].toughness.saturating_add(t);
        }
    }
}
