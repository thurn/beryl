//! Derived characteristics: continuous effects applied in layers, recomputed from base when
//! dirty (architecture §9, decision D5, CR 613).

mod apply;

use beryl_kernel::derive::Rederive;

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, AbilityKind, Colors, Layer, LayerSpec, Modification, Objects,
};
use crate::decision::{Outcome, StallReason};
use crate::event::{Listens, Replaces, Sink};
use crate::filter;
use crate::fixed::Full;
use crate::ids::CardId;
use crate::layout::{Layout, PerSeat, Queue, index};
use crate::state::{
    AbilityMask, CounterKind, Derived, LayerChange, LockedIn, Permanents, Rules, State, Zone,
    read_layer,
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
    /// ([CR 611.2c]): "the set of objects it affects is determined when that continuous effect
    /// begins. After that point, the set won't change."
    ///
    /// [CR 611.2c]: https://yawgatog.com/resources/magic-rules/#R6112c
    Set(LockedIn<L>),
    /// The permanents a static ability of the permanent at this battlefield position
    /// applies to, the ability at [`Apply::order`] of its face: found as the effect's layer
    /// applies, against the results of the layers before it
    /// ([`apply::static_members`]). A static ability on any set but its own permanent and
    /// the one that permanent is attached to, or with an "as long as" that reads the layer
    /// results, is listed this way.
    Static(L::Index),
}

/// One continuous effect to apply in this recompute, whatever the number of permanents it
/// changes, or one part of one: a static ability whose effect has parts in several layers
/// is listed once for each part ([CR 613.6]).
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Apply<L: Layout> {
    layer: u8,
    timestamp: u16,
    /// Static abilities on one object share its timestamp; their order on the card breaks
    /// the tie.
    order: u8,
    /// The part of a static ability's effect this is, in the order the card says them: two
    /// parts in one layer apply in that order.
    part: u8,
    target: Target<L>,
    /// The game index of the effect's source card, which a granted ability is printed on.
    source: L::Card,
    change: LayerChange,
    /// Of a static ability, the battlefield position of the permanent it is on, and whether
    /// this is the part in the ability's first layer, where the effect begins
    /// ([`Begun`]); none for a resolved spell's or ability's effect, which began as it
    /// resolved.
    from: Option<(L::Index, bool)>,
    /// How many parts the effect has: a static ability with more than one carries the set
    /// it began on to its later parts.
    parts: u8,
    /// Of a static ability's first part, found at the head of its layer: its permanent no
    /// longer has the ability, so the effect does not begin.
    lost: bool,
}

impl<L: Layout> Apply<L> {
    /// Where it comes among the effects ([CR 613.7]): "Within a layer or sublayer, determining
    /// which order effects are applied in is usually done using a timestamp system." Static
    /// abilities of permanents that share a timestamp apply in the order of the permanents they
    /// change. An effect on a set needs no such order: it has a timestamp of its own, and one
    /// change to each member commutes.
    ///
    /// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
    fn key(&self) -> (u8, u16, u8, u8, u16) {
        let position: u16 = match self.target {
            Target::Position(i) | Target::Static(i) => i.into(),
            Target::Set(_) => 0,
        };
        (self.layer, self.timestamp, self.order, self.part, position)
    }
}

/// Filler for the list's unused places: a change that changes nothing.
impl<L: Layout> Default for Apply<L> {
    fn default() -> Self {
        Apply {
            layer: 0,
            timestamp: 0,
            order: 0,
            part: 0,
            target: Target::Position(L::Index::default()),
            source: L::Card::FIRST,
            change: LayerChange::ModifyPt(0, 0),
            from: None,
            parts: 1,
            lost: false,
        }
    }
}

/// What a static ability with parts in several layers did where it began: the permanents
/// it applies to, or none if it did not begin, its permanent having lost the ability by the
/// time its first layer came ([CR 613.6]): "If an effect starts to apply in one layer and/or
/// sublayer, it will continue to be applied to the same set of objects in each other applicable layer
/// and/or sublayer, even if the ability generating the effect is removed during this
/// process." The later parts read it.
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Begun<L: Layout> {
    /// The battlefield position of the permanent with the ability.
    from: u16,
    /// The ability's index on that permanent's face.
    order: u8,
    members: Option<Bitboard<L::Slot>>,
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

/// [`refresh`], for a tool that changed a state through the board door
/// ([`State::board`]) and reads it next: the present-cost benchmark, which takes a card out
/// of a position and wants the cost of the recompute left out of what it counts after.
/// [`Full`] as [`refresh`] gives it.
pub fn rederive<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    refresh(s, t, &mut Sink::default())
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

/// The kinds of event the permanents on the battlefield have triggered abilities for, and the
/// kinds their static abilities replace (with whether one changes a rule for players), in one
/// pass. A face-down permanent has none of either, whichever card it is ([CR 708.2a]): it has "no
/// text, no name, no subtypes, and no mana cost". So it counts for nothing
/// ([`crate::query::Copiable::may_listen`]), and masking or determinizing its hidden identity
/// leaves both unchanged.
///
/// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
pub(crate) fn listening_of_all<L: Layout>(s: &State<L>, t: &GameTable) -> (Listens, Replaces) {
    s.battlefield()
        .iter()
        .fold((Listens::NONE, Replaces::NONE), |(l, r), &slot| {
            let what = s.copiable(slot);
            (l | what.may_listen(t), r | what.may_replace(t))
        })
}

/// [`listening_of_all`], leaving out the permanents that `rows`, the layer results, say have
/// lost every ability.
pub(crate) fn listening<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rows: &[Derived<L>],
) -> (Listens, Replaces) {
    s.battlefield().iter().zip(rows).fold(
        (Listens::NONE, Replaces::NONE),
        |(l, r), (&slot, row)| {
            // A permanent that lost all its abilities listens to nothing and replaces
            // nothing (CR 613.1f); one that lost some keeps its card's masks, which say what
            // it may do and are read again ability by ability.
            if { row.lost } == L::AbilityMask::ALL {
                return (l, r);
            }
            let what = s.copiable(slot);
            (l | what.may_listen(t), r | what.may_replace(t))
        },
    )
}

/// Whether a change in the number of `kind` counters on the object in `slot` can change its derived
/// characteristics, so that the layers must run again. Only +1/+1 and -1/-1 counters modify power
/// and toughness ([CR 613.4c]): "Effects and counters that modify power and/or toughness ... are
/// applied". Any other kind matters only to a static ability of its own that applies while it has
/// some: loyalty, which planeswalkers gain and lose every turn, never does.
///
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
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

/// Recomputes every permanent's characteristics from base ([CR 613.1]): "The values of an object's
/// characteristics are determined by starting with the actual object. For a card, that means the
/// values of the characteristics printed on that card. For a token or a copy of a spell or card,
/// that means the values of the characteristics defined by the effect that created it. Then all
/// applicable continuous effects are applied in a series of layers in the following order".
///
/// Within a layer, effects apply in timestamp order ([CR 613.7]): "An effect with an earlier
/// timestamp is applied before an effect with a later timestamp." A characteristic-defining ability
/// defines power alone and sits in layer 7a with no other effect, so "apply effects from
/// characteristic-defining abilities first" (CR 613.3) holds by the layer order. No two of the
/// cards' effects in one layer depend on each other ([CR 613.8]), so timestamps decide.
///
/// The layers apply one at a time: every effect of a layer is applied to every permanent
/// before any effect of the next. A static ability whose set is chosen by a filter, or whose
/// "as long as" reads characteristics, finds them as its layer comes up, against what the
/// layers before it made of each permanent ([`apply::static_members`]).
///
/// The kernel has ended the effects on objects that left their zone as they left, and stores
/// the results, with what each face-down permanent's controller now sees, as the
/// recompute finishes ([`beryl_kernel::derive::Rederive::finish`]), and with them the rules
/// the permanents' static abilities change for each player ([`player_rules`]).
///
/// [`Full`] if the list of effects had no place for one: the results are stored all the
/// same, lacking that effect, for a caller that ends the game.
///
/// [CR 613.1]: https://yawgatog.com/resources/magic-rules/#R6131
/// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
/// [CR 613.8]: https://yawgatog.com/resources/magic-rules/#R6138
fn recompute<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    let Some(mut s) = s.rederive() else {
        return Ok(());
    };

    // Made at the first effect that applies: most recomputes have none, and the list is big.
    let mut list: Option<Applying<L>> = None;
    let mut listed = Ok(());
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
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
            lost: Default::default(),
        };
        for (n, ability) in f.abilities.iter().enumerate() {
            let Some(spec) = ability.layer() else {
                continue;
            };
            listed = listed.and(list_static(&s, t, &mut list, i, n, ability, spec));
        }
    }
    listed = listed.and(resolved_effects(&s, &mut list));
    let mut removed = false;
    match &mut list {
        Some(list) => {
            list.as_mut_slice().sort_unstable_by_key(Apply::key);
            // The counters of layer 7c apply after its effects and before those of 7d.
            let split = (list.as_slice()).partition_point(|a| a.layer <= Layer::PtModifying as u8);
            let (to_7c, after) = list.as_mut_slice().split_at_mut(split);
            let mut begun = None;
            let first = apply_in_order(&mut s, t, to_7c, &mut begun);
            counters(&mut s);
            let second = apply_in_order(&mut s, t, after, &mut begun);
            listed = listed.and(first).and(second);
            removed = list.iter().any(|a| a.change.removes_abilities());
        }
        None => counters(&mut s),
    }
    let (listening, replacing) = if removed {
        listening(&s, t, s.rows())
    } else {
        listening_of_all(&s, t)
    };
    // A permanent that may change a rule for players said so in the mask just read for
    // the replacements: with none on the battlefield, one AND.
    let rules = if replacing.changes_player_rules() {
        player_rules(&mut s, t)
    } else {
        [Rules::NONE; _]
    };
    s.finish(listening, replacing, rules, removed);
    listed
}

/// Lists the static ability at `n` of the face of the permanent at battlefield position `i`,
/// whose continuous effect is `spec`, among the effects to apply, if it applies now: one entry
/// for each part of its effect, each in its own layer ([CR 613.6]): "If an effect should be
/// applied in different layers and/or sublayers, the parts of the effect each apply in their
/// appropriate ones."
///
/// Out of line: few permanents have such an ability, and the scan runs for every one.
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
#[inline(never)]
fn list_static<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    list: &mut Option<Applying<L>>,
    i: usize,
    n: usize,
    ability: &'static Ability,
    spec: &'static LayerSpec,
) -> Result<(), Full> {
    let slot = s.battlefield()[i];
    let o = s.object(slot);
    let what = s.copiable(slot);
    // CR 611.3a: "A continuous effect generated by a static ability isn't 'locked
    // in'; it applies at any given moment to whatever its text indicates." Its "as
    // long as" and what it affects are read each time. One on its own permanent or
    // the one that is attached to, whose condition reads only what the layers start
    // from, is settled here; any other as its layer applies.
    let target = match spec.affected {
        Objects::This | Objects::AttachedTo
            if !spec
                .condition
                .as_ref()
                .is_some_and(static_ability::reads_results) =>
        {
            let applies = spec
                .condition
                .as_ref()
                .is_none_or(|condition| apply::holds_from_base(s, slot, condition));
            let target = match spec.affected {
                Objects::This => Some(i),
                _ => o.attached_to().and_then(|a| s.battlefield_position(a)),
            };
            match (applies, target) {
                (true, Some(target)) => {
                    crate::coverage::hit(ability);
                    Target::Position(index::<L>(target))
                }
                _ => return Ok(()),
            }
        }
        _ => Target::Static(index::<L>(i)),
    };
    let (source, face) = what.record(t);
    let you = s.base_controller(slot);
    // The part in the earliest layer, where the effect begins: the first said of those.
    let layer_of = |modification: Modification| {
        LayerChange::resolve(modification, you)
            .map_or_else(|| read_layer(modification), LayerChange::layer)
    };
    let first = (0..spec.modifications.len())
        .min_by_key(|&part| (layer_of(spec.modifications[part]), part))
        .unwrap_or(0);
    let mut listed = Ok(());
    for (part, &modification) in spec.modifications.iter().enumerate() {
        // No static ability of the cards changes control. One that did would name
        // its source's controller as layer 2 has it, not as the permanent entered.
        debug_assert_ne!(modification, Modification::Control);
        // An amount read as its layer applies is named by the ability that says it.
        let change = LayerChange::resolve(modification, you).unwrap_or(LayerChange::Read {
            layer: read_layer(modification),
            face,
            ability: AbilityIx::new(n as u8),
            part: part as u8,
        });
        let pushed = list.get_or_insert_with(Default::default).push(Apply {
            layer: change.layer() as u8,
            timestamp: o.timestamp(),
            order: n as u8,
            part: part as u8,
            target,
            source,
            change,
            from: Some((index::<L>(i), part == first)),
            parts: spec.modifications.len() as u8,
            lost: false,
        });
        listed = listed.and(pushed);
    }
    listed
}

/// Applies `list`, effects sorted by layer and timestamp: all of a layer's before any of the
/// next one's, so each layer applies to what the layers before it made of every permanent,
/// and an effect that finds what it applies to as it applies sees those results.
///
/// At the head of each layer, each static ability whose first layer it is begins, if its
/// permanent still has the ability ([`Begun`]). One whose effect has parts in later layers
/// carries the set it found to them in `begun`.
///
/// [`Full`] if an ability's set had no place in `begun`, or a removal of abilities no place
/// in a permanent's mask ([`apply::apply`]): the rest still applies, for a caller that
/// stalls the game.
///
/// Out of line: most recomputes apply nothing, and the rest of the recompute runs for every
/// one.
#[inline(never)]
fn apply_in_order<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    list: &mut [Apply<L>],
    begun: &mut Option<L::Queue<Begun<L>, 32>>,
) -> Result<(), Full> {
    let mut applied = Ok(());
    let mut head = 0;
    while head < list.len() {
        let layer = list[head].layer;
        let end = head + list[head..].partition_point(|a| a.layer == layer);
        let segment = &mut list[head..end];
        // Whether the permanent of each ability beginning in this layer still has it, read
        // before any effect of the layer applies.
        for a in segment.iter_mut() {
            if let Some((from, true)) = a.from {
                a.lost = !s.has_ability_so_far(position::<L>(from), AbilityIx::new(a.order));
            }
        }
        for a in segment.iter() {
            let members = match (a.from, a.target) {
                (None, Target::Position(i)) => Bitboard::single(s.battlefield()[position::<L>(i)]),
                (None, Target::Set(set)) => s.locked_in(set),
                (Some((from, true)), target) => {
                    let members = match target {
                        _ if a.lost => None,
                        Target::Position(i) => {
                            Some(Bitboard::single(s.battlefield()[position::<L>(i)]))
                        }
                        Target::Static(i) => Some(apply::static_members(s, t, i.into(), a.order)),
                        Target::Set(_) => unreachable!("a static ability locks in no set"),
                    };
                    if a.parts > 1 {
                        let carried = Begun {
                            from: from.into(),
                            order: a.order,
                            members,
                        };
                        applied =
                            applied.and(begun.get_or_insert_with(Default::default).push(carried));
                    }
                    let Some(members) = members else {
                        continue;
                    };
                    members
                }
                (Some((from, false)), _) => {
                    let from: u16 = from.into();
                    let carried = begun.as_ref().and_then(|begun| {
                        begun
                            .iter()
                            .find(|b| (b.from, b.order) == (from, a.order))
                            .copied()
                    });
                    match carried.and_then(|b| b.members) {
                        Some(members) => members,
                        None => continue,
                    }
                }
                (None, Target::Static(_)) => unreachable!("a resolved effect names no ability"),
            };
            let you = a
                .from
                .map(|(from, _)| s.rows()[position::<L>(from)].controller);
            applied = applied.and(apply::apply_to_members(
                s, t, members, a.source, a.change, you,
            ));
        }
        head = end;
    }
    applied
}

/// A battlefield position the layout's index names, as a `usize`.
#[inline]
fn position<L: Layout>(i: L::Index) -> usize {
    i.into()
}

/// Layer 7c's counters ([CR 613.4c]): "Effects and counters that modify power and/or
/// toughness (but don't set power and/or toughness to a specific number or value) are
/// applied." Additions commute, so they follow the layer's effects.
///
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
#[inline]
fn counters<L: Layout>(s: &mut Rederive<'_, L>) {
    for i in 0..s.battlefield().len() {
        let c = s.object(s.battlefield()[i]).counters();
        let d = &mut s.results()[i];
        let delta = c.plus_one as i8 - c.minus_one as i8;
        d.power = d.power.saturating_add(delta);
        d.toughness = d.toughness.saturating_add(delta);
    }
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
            part: 0,
            target,
            source: e.source,
            change,
            from: None,
            parts: 1,
            lost: false,
        });
        listed = listed.and(pushed);
    }
    listed
}

/// The rules the permanents' static abilities change for each player, by seat ([CR 613.11]): "Some
/// continuous effects affect game rules rather than objects. ... These effects are applied after
/// all other continuous effects have been applied." So control is settled: "each opponent" and
/// "you" in such an ability are read for the permanent's controller as layer 2 left it ([CR 109.5]:
/// "For a static ability, this is the current controller of the object it's on."). A face-down
/// permanent has no abilities ([CR 708.2]), which its copiable values answer for.
///
/// Out of line: only a recompute with such a permanent on the battlefield comes here.
///
/// [CR 613.11]: https://yawgatog.com/resources/magic-rules/#R61311
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
/// [CR 708.2]: https://yawgatog.com/resources/magic-rules/#R7082
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
        for (n, ability) in what.face(t).abilities.iter().enumerate() {
            let (AbilityKind::Static, Some(rule)) = (ability.kind(), ability.rule()) else {
                continue;
            };
            // A permanent that lost the ability changes no rule (CR 613.1f).
            if !s.has_ability_so_far(i, AbilityIx::new(n as u8)) {
                continue;
            }
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
