//! Derived characteristics: continuous effects applied in layers, recomputed from base when
//! dirty (architecture §9, decision D5, CR 613).

use beryl_kernel::derive::Rederive;

use beryl_card::Amount;

use crate::card::{AbilityIx, AbilityKind, Colors, Modification, Objects};
use crate::decision::{Outcome, StallReason};
use crate::event::{Sink, kind};
use crate::filter::{self, Filter};
use crate::fixed::Full;
use crate::ids::{CardId, Seat};
use crate::layout::{Layout, PerSeat, Queue, index};
use crate::state::{
    CounterKind, Derived, GrantRef, LayerChange, LockedIn, Permanents, Rules, State, Zone,
};
use crate::static_ability;
use crate::table::GameTable;
use crate::trigger::face;

/// What one continuous effect changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Target<L: Layout> {
    /// The permanent at this battlefield position. The layout's index names every position
    /// of its battlefield, however many permanents that holds.
    Position(L::Index),
    /// Each permanent the effect locked in as it began that is still the object it was
    /// ([CR 611.2c](https://yawgatog.com/resources/magic-rules/#R6112c)): "the set of objects
    /// it affects is determined when that continuous effect begins. After that point, the
    /// set won't change."
    Set(LockedIn<L>),
}

/// One continuous effect to apply in this recompute, whatever the number of permanents it
/// changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Apply<L: Layout> {
    layer: u8,
    timestamp: u16,
    /// Static abilities on one object share its timestamp; their order on the card breaks
    /// the tie.
    order: u8,
    target: Target<L>,
    /// The game index of the effect's source card, which a granted ability is printed on.
    source: L::Card,
    change: LayerChange,
}

impl<L: Layout> Apply<L> {
    /// Where it comes among the effects
    /// ([CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137)): "Within a layer or
    /// sublayer, determining which order effects are applied in is usually done using a
    /// timestamp system." Static abilities of permanents that share a timestamp apply in
    /// the order of the permanents they change. An effect on a set needs no such order: it
    /// has a timestamp of its own, and one change to each member commutes.
    fn key(&self) -> (u8, u16, u8, u16) {
        let position: u16 = match self.target {
            Target::Position(i) => i.into(),
            Target::Set(_) => 0,
        };
        (self.layer, self.timestamp, self.order, position)
    }
}

/// Filler for the list's unused places: a change that changes nothing.
impl<L: Layout> Default for Apply<L> {
    fn default() -> Self {
        Apply {
            layer: 0,
            timestamp: 0,
            order: 0,
            target: Target::Position(L::Index::default()),
            source: L::Card::FIRST,
            change: LayerChange::ModifyPt(0, 0),
        }
    }
}

/// The effects one recompute applies: one entry for each effect of the Effects store
/// ([`crate::state::EFFECTS`], 64 in a fixed layout) and one for each static ability of a
/// permanent that changes characteristics. A fixed layout's list can fill, with more such
/// abilities on the battlefield than are left of its 192 places; the recompute then reports
/// [`Full`] and the game stalls into a layout whose list grows.
type Applying<L> = <L as Layout>::Queue<Apply<L>, 192>;

/// Recomputes `derived` if anything changed since the last recompute. Every chokepoint that
/// mutates a layer input sets `derived_dirty`, and every path that reads `derived` after one
/// calls this first, so reads never see stale values and never recompute.
///
/// [`Full`] if an effect that applies had no place in the list of them: the results are
/// stored without it, so nothing reads a stale one before the caller stalls the game, as it
/// must.
pub(crate) fn refresh<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if s.is_derived_stale() {
        sink.work.layer_recomputes += 1;
        return recompute(s, t);
    }
    Ok(())
}

/// The game's outcome if it is over, and if it is not, [`refresh`] for what the engine's
/// loop reads next. A recompute with no room for an effect ends the game stalled, as any
/// capacity that runs out does, so the game is then over.
pub(crate) fn over_or_refreshed<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Option<Outcome> {
    if s.outcome().is_none() && refresh(s, t, sink).is_err() {
        s.end_game(Outcome::Stalled(StallReason::Capacity));
    }
    s.outcome()
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
            a.layer()
                .and_then(|spec| spec.condition.as_ref())
                .is_some_and(|condition| static_ability::reads_counters(condition, kind))
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
/// timestamp is applied before an effect with a later timestamp." A characteristic-defining
/// ability defines power alone and sits in layer 7a with no other effect, so "apply effects
/// from characteristic-defining abilities first" (CR 613.3) holds by the layer order. No two of the
/// cards' effects in one layer depend on each other
/// ([CR 613.8](https://yawgatog.com/resources/magic-rules/#R6138)), so timestamps decide.
///
/// The kernel has ended the effects on objects that left their zone as they left, and stores
/// the results, with what each face-down permanent's controller now sees, as the
/// recompute finishes ([`beryl_kernel::derive::Rederive::finish`]), and with them the rules
/// the permanents' static abilities change for each player ([`player_rules`]).
///
/// [`Full`] if the list of effects had no place for one: the results are stored all the
/// same, lacking that effect, for a caller that ends the game.
fn recompute<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    let Some(mut s) = s.rederive() else {
        return Ok(());
    };
    let listening = listening(&s, t);

    // Made at the first effect that applies: most recomputes have none, and the list is big.
    let mut list: Option<Applying<L>> = None;
    let mut listed = Ok(());
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
            subtypes: what.subtypes(t),
            granted: [None; 4],
        };
        for (n, ability) in f.abilities.iter().enumerate() {
            let Some(spec) = ability.layer() else {
                continue;
            };
            // CR 611.3a: "A continuous effect generated by a static ability isn't 'locked
            // in'; it applies at any given moment to whatever its text indicates." Its "as
            // long as" and what it affects are read for this permanent each time.
            let applies = spec
                .condition
                .as_ref()
                .is_none_or(|condition| static_ability::holds(&s, slot, condition));
            let target = match spec.affected {
                Objects::This => Some(i),
                Objects::AttachedTo => o.attached_to().and_then(|a| s.battlefield_position(a)),
                Objects::Target(_)
                | Objects::Those
                | Objects::TriggeredOn
                | Objects::TriggeredOnIn(_)
                | Objects::ExiledWithThis
                | Objects::InZoneOf { .. }
                | Objects::CardsIn { .. }
                | Objects::All { .. }
                | Objects::Paid(_) => {
                    unreachable!("the card table refuses a static ability on any other set")
                }
            };
            if let (true, Some(target)) = (applies, target) {
                crate::coverage::hit(ability);
                // No static ability of the cards changes control. One that did would name
                // its source's controller as layer 2 has it, not as the permanent entered.
                debug_assert_ne!(spec.modification, Modification::Control);
                let (source, face) = what.record(t);
                // An amount read as its layer applies is named by the ability that says it.
                let change = LayerChange::resolve(spec.modification, s.base_controller(slot))
                    .unwrap_or_else(|| LayerChange::DefinedPower {
                        face,
                        ability: AbilityIx::new(n as u8),
                    });
                let pushed = list.get_or_insert_with(Default::default).push(Apply {
                    layer: change.layer() as u8,
                    timestamp: o.timestamp(),
                    order: n as u8,
                    target: Target::Position(index::<L>(target)),
                    source,
                    change,
                });
                listed = listed.and(pushed);
            }
        }
    }
    listed = listed.and(resolved_effects(&s, &mut list));
    let len = s.battlefield().len();
    if let Some(list) = &mut list {
        list.as_mut_slice().sort_unstable_by_key(Apply::key);
        for a in list.iter() {
            match a.target {
                Target::Position(i) => apply(t, s.results(), i.into(), a.source, a.change),
                Target::Set(set) => {
                    let members = s.locked_in(set);
                    for i in 0..len {
                        if members.contains(s.battlefield()[i]) {
                            apply(t, s.results(), i, a.source, a.change);
                        }
                    }
                }
            }
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
    listed
}

/// Lists the continuous effects of resolved spells and abilities that change a permanent
/// on the battlefield now, for [`recompute`] to apply with the static abilities' in layer
/// and timestamp order. The kernel has ended those on objects that left their zone.
///
/// An effect is one entry, whether it changes one permanent or a set of them: the list is
/// as long as the effects are many, whatever the size of a set. [`Full`] if one had no
/// place.
#[inline]
fn resolved_effects<L: Layout>(
    s: &Rederive<'_, L>,
    list: &mut Option<Applying<L>>,
) -> Result<(), Full> {
    let mut listed = Ok(());
    for (e, on, change) in s.layer_effects() {
        let target = match on {
            Permanents::One(object) => {
                let slot = object.slot;
                if s.object(slot).zone() != Zone::Battlefield {
                    continue;
                }
                let Some(position) = s.battlefield_position(slot) else {
                    continue;
                };
                Target::Position(index::<L>(position))
            }
            // One effect on the permanents it locked in, with its one timestamp
            // ([CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137)); the members
            // are read as it applies.
            Permanents::Set(set) => Target::Set(set),
        };
        let pushed = list.get_or_insert_with(Default::default).push(Apply {
            layer: change.layer() as u8,
            timestamp: e.timestamp,
            order: 0,
            target,
            source: e.source,
            change,
        });
        listed = listed.and(pushed);
    }
    listed
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
        let made = what.player_rules(t);
        if made.is_empty() {
            continue;
        }
        let controller = s.results()[i].controller;
        for ability in what.face(t).abilities {
            let (AbilityKind::Static, Some(rule)) = (ability.kind(), ability.rule()) else {
                continue;
            };
            // The card table has numbered every rule change its cards name.
            let Some(id) = t.rule_id(rule).filter(|&id| made.contains(id)) else {
                continue;
            };
            // The rule is in force for those players from here: the ability did what it
            // says, as one that changes a permanent does above.
            crate::coverage::hit(ability);
            // Each player's set is a union: the order they are met in is nothing.
            for player in s.players_in_seat_order(s.players()) {
                if filter::is_for(&rule.players, player, controller) {
                    rules[player.index()] = rules[player.index()].with(id);
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
fn apply<L: Layout>(
    t: &GameTable,
    derived: &mut [Derived<L>],
    i: usize,
    source: L::Card,
    change: LayerChange,
) {
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
            derived[i].subtypes = derived[i].subtypes.of_types(t.subtypes(), types);
        }
        LayerChange::AddSubtype(subtype) => {
            derived[i].subtypes = derived[i].subtypes.with(t.subtypes(), subtype);
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
        LayerChange::DefinedPower { face, ability } => {
            let said_by = &t.def(source).face(face).abilities[ability.index()];
            let Some(Ok(Some(amount))) = said_by.layer().map(static_ability::defined_power) else {
                unreachable!("the card table has only the defined powers the layers read");
            };
            let power = defined(t, amount, derived, derived[i].controller);
            derived[i].power = i8::try_from(power).unwrap_or(i8::MAX);
        }
        LayerChange::ModifyPt(p, t) => {
            derived[i].power = derived[i].power.saturating_add(p);
            derived[i].toughness = derived[i].toughness.saturating_add(t);
        }
    }
}

/// The value of an amount a characteristic-defining ability defines, read as its layer
/// applies for the permanent's controller `you`: of the permanents as layers 2 to 6 left
/// them, so a count sees each one's final types and controller. The card table has only
/// amounts read this way (`beryl_kernel`'s `static_ability::defined_power`).
fn defined<L: Layout>(t: &GameTable, amount: &Amount, derived: &[Derived<L>], you: Seat) -> usize {
    match *amount {
        Amount::Fixed(n) => usize::from(n),
        Amount::Times(n, amount) => usize::from(n).saturating_mul(defined(t, amount, derived, you)),
        Amount::Count(&Objects::All { filter, .. }) => {
            derived.iter().filter(|d| passes(t, filter, d, you)).count()
        }
        _ => unreachable!("the card table refuses an amount the layers cannot read"),
    }
}

/// Whether a permanent with the characteristics `d` passes `filter`, read for `you`: what
/// [`Filter`]'s evaluation says of a permanent, of results the state does not yet hold.
fn passes<L: Layout>(t: &GameTable, filter: Filter, d: &Derived<L>, you: Seat) -> bool {
    match filter {
        Filter::Any => true,
        Filter::Types(types) => d.types.intersects(types),
        Filter::Supertypes(supertypes) => d.supertypes.intersects(supertypes),
        Filter::Subtype(subtype) => d.subtypes.contains(t.subtypes(), subtype),
        Filter::Colors(colors) => d.colors.intersects(colors),
        Filter::Whose(players) => filter::is_for(players, d.controller, you),
        Filter::Not(filter) => !passes(t, *filter, d, you),
        Filter::All(filters) => filters.iter().all(|&filter| passes(t, filter, d, you)),
        Filter::AnyOf(filters) => filters.iter().any(|&filter| passes(t, filter, d, you)),
        Filter::Tapped | Filter::ManaValueAtMost(_) | Filter::ManaValueAtMostX => {
            unreachable!("the card table refuses a filter the layers cannot read")
        }
    }
}
