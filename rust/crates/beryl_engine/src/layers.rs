//! Derived characteristics: continuous effects applied in layers, recomputed from base when
//! dirty (architecture §9, decision D5, CR 613).

use crate::card::{Affected, Colors, FaceIx, Modification, StaticCondition, Subtypes, Types};
use crate::event::Sink;
use crate::ids::CardIx;
use crate::layout::{Layout, Queue, Table};
use crate::state::{Continuous, CounterKind, Derived, GrantRef, ObjectFlags, State, Zone};
use crate::table::GameTable;
use crate::trigger::face;

/// One continuous effect to apply in this recompute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Apply {
    layer: u8,
    timestamp: u16,
    /// Static abilities on one object share its timestamp; their order on the card breaks
    /// the tie.
    order: u8,
    /// The battlefield index of the permanent it affects.
    target: u8,
    /// The game index of the effect's source card, which a granted ability is printed on.
    source: CardIx,
    modification: Option<Modification>,
}

/// Recomputes `derived` if anything changed since the last recompute. Every chokepoint that
/// mutates a layer input sets `derived_dirty`, and every path that reads `derived` after one
/// calls this first, so reads never see stale values and never recompute.
pub(crate) fn refresh<L: Layout>(s: &mut State<L>, t: &GameTable, sink: &mut Sink<L>) {
    if s.flags.derived_dirty {
        sink.work.layer_recomputes += 1;
        recompute(s, t);
        s.flags.derived_dirty = false;
    }
}

/// The kinds of event the permanents on the battlefield have triggered abilities for. A
/// face-down permanent has none, whichever card it is
/// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)): it has "no text, no
/// name, no subtypes, and no mana cost". So it counts for nothing, and masking or
/// determinizing its hidden identity leaves this unchanged.
pub(crate) fn listening<L: Layout>(s: &State<L>, t: &GameTable) -> u16 {
    s.battlefield
        .iter()
        .map(|&slot| s.object(slot))
        .filter(|o| o.face != FaceIx::DOWN)
        .fold(0, |k, o| k | t.listens(o.card))
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
        || s.object(slot).zone != Zone::Battlefield
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
fn recompute<L: Layout>(s: &mut State<L>, t: &GameTable) {
    // Effects on an object that left its zone end: that object no longer exists (CR 400.7).
    s.drop_stale_effects();

    s.listening = listening(s, t);

    // Made at the first effect that applies: most recomputes have none, and the list is big.
    let mut list: Option<L::Queue<Apply, 192>> = None;
    for i in 0..s.battlefield.len() {
        let slot = s.battlefield.as_slice()[i];
        let o = *s.object(slot);
        let f = face(s, t, slot);
        // Layer 1: the copiable values. A token copy points at the card it copies, so its
        // values are the card's, with the 1/1 exception of CR 707.9b.
        let (power, toughness) = if o.flags.has(ObjectFlags::ONE_ONE) {
            (1, 1)
        } else {
            (f.power.unwrap_or(0), f.toughness.unwrap_or(0))
        };
        s.derived[i] = Derived {
            power,
            toughness,
            types: f.types,
            supertypes: f.supertypes,
            colors: f.colors,
            controller: o.base_controller,
            protection: Colors::NONE,
            keywords: f.keywords,
            subtypes: f.subtypes,
            granted: [None; 4],
        };
        for (n, ability) in f.abilities.iter().enumerate() {
            let Some(spec) = ability.layer else { continue };
            let applies = match spec.condition {
                StaticCondition::Always => true,
                StaticCondition::HasCounter(kind) => o.counters.get(kind) > 0,
                StaticCondition::Impending => {
                    o.flags.has(ObjectFlags::IMPENDING) && o.counters.get(CounterKind::Time) > 0
                }
            };
            let target = match spec.affected {
                Affected::Itself => Some(i),
                Affected::Enchanted => o.attached_to.and_then(|a| s.battlefield_position(a)),
            };
            if let (true, Some(target)) = (applies, target) {
                crate::coverage::hit(ability);
                // A list longer than its capacity would need more effects than the cards
                // can create at once.
                let _ = list.get_or_insert_with(Default::default).push(Apply {
                    layer: spec.modification.layer() as u8,
                    timestamp: o.timestamp,
                    order: n as u8,
                    target: target as u8,
                    source: o.card,
                    modification: Some(spec.modification),
                });
            }
        }
    }
    for e in s.effects.iter() {
        let Continuous::Layer(m) = e.effect else {
            continue;
        };
        if s.object(e.affected.slot).zone != Zone::Battlefield {
            continue;
        }
        if let Some(target) = s.battlefield_position(e.affected.slot) {
            let _ = list.get_or_insert_with(Default::default).push(Apply {
                layer: m.layer() as u8,
                timestamp: e.timestamp,
                order: 0,
                target: target as u8,
                source: e.source,
                modification: Some(m),
            });
        }
    }
    let len = s.battlefield.len();
    if let Some(list) = &mut list {
        list.as_mut_slice()
            .sort_unstable_by_key(|a| (a.layer, a.timestamp, a.order, a.target));
        for a in list.iter() {
            let Some(m) = a.modification else { continue };
            let i = usize::from(a.target);
            apply(&mut s.derived.as_mut_slice()[..len], i, a.source, m);
        }
    }
    // Layer 7c: counters modify power and toughness
    // ([CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c)): "Effects and counters
    // that modify power and/or toughness (but don't set power and/or toughness to a specific
    // number or value) are applied." Additions commute, so they follow the effects.
    for i in 0..len {
        let c = s.object(s.battlefield.as_slice()[i]).counters;
        let d = &mut s.derived[i];
        let delta = c.plus_one as i8 - c.minus_one as i8;
        d.power = d.power.saturating_add(delta);
        d.toughness = d.toughness.saturating_add(delta);
    }
    // [CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085): "At any time, you may
    // look at ... a face-down permanent you control". Control is settled here, so a player who
    // gains control of one sees it from then on.
    for i in 0..len {
        let slot = s.battlefield.as_slice()[i];
        if s.object(slot).face == FaceIx::DOWN {
            s.knowledge[s.derived[i].controller.index()].insert(slot);
        }
    }
}

/// Applies one modification to the permanent at battlefield index `i`.
fn apply(derived: &mut [Derived], i: usize, source: CardIx, m: Modification) {
    match m {
        // CR 613.1b: "Layer 2: Control-changing effects are applied."
        Modification::Control(player) => derived[i].controller = player,
        // CR 613.1d: "Layer 4: Type-changing effects are applied. These include effects that
        // change an object's card type, subtype, and/or supertype."
        Modification::AddTypes(types) => derived[i].types = derived[i].types.union(types),
        Modification::RemoveTypes(types) => derived[i].types = derived[i].types.minus(types),
        // CR 205.1a: "Some effects set an object's card type. In most such cases, the new
        // card type(s) replaces any existing card types. ... If an object's card type is
        // removed, the subtypes correlated with that card type will remain if they are also
        // the subtypes of a card type the object currently has; otherwise, they are also
        // removed".
        Modification::SetTypes(types) => {
            derived[i].types = types;
            let mut kept = Subtypes::NONE;
            for subtype in derived[i].subtypes.iter() {
                if types.intersects(subtype.card_type()) {
                    kept = kept.with(subtype);
                }
            }
            derived[i].subtypes = kept;
        }
        Modification::AddSubtype(subtype) => {
            derived[i].subtypes = derived[i].subtypes.with(subtype);
        }
        // CR 613.1f: "Layer 6: Ability-adding effects, keyword counters, ability-removing
        // effects, and effects that say an object can't have an ability are applied."
        Modification::AddKeywords(k) => derived[i].keywords = derived[i].keywords.union(k),
        Modification::Grant(grant) => {
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
        Modification::PowerIsCreaturesYouControl => {
            let you = derived[i].controller;
            let creatures = derived
                .iter()
                .filter(|d| d.controller == you && d.types.contains(Types::CREATURE))
                .count();
            derived[i].power = i8::try_from(creatures).unwrap_or(i8::MAX);
        }
        Modification::ModifyPt(p, t) => {
            derived[i].power = derived[i].power.saturating_add(p);
            derived[i].toughness = derived[i].toughness.saturating_add(t);
        }
    }
}
