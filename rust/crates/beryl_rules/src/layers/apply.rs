//! What one continuous effect does as its layer applies: what it applies to, found against
//! the results of the layers before it, and the change it makes to each.

use beryl_kernel::derive::Rederive;
use beryl_kernel::static_ability::{self, LayerResults, NoResults};

use beryl_card::Amount;

use crate::bitboard::Bitboard;
use crate::card::{AbilityIx, FaceIx, Objects};
use crate::filter::{self, Filter};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Derived, GrantRef, LayerChange};
use crate::table::GameTable;

/// Applies `change` to each permanent in `members`, in battlefield order. One change to
/// each member commutes with the others, so the order is nothing to the results.
pub(super) fn apply_to_members<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    members: Bitboard<L::Slot>,
    source: L::Card,
    change: LayerChange,
) {
    for i in 0..s.battlefield().len() {
        if members.contains(s.battlefield()[i]) {
            apply(s, t, i, source, change);
        }
    }
}

/// The permanents a static ability of the permanent at battlefield position `at` applies
/// to now, the ability at `n` of its face, or none if its "as long as" does not hold. Both
/// are read against the layer results so far ([CR 611.3a]): "A continuous effect generated
/// by a static ability isn't 'locked in'; it applies at any given moment to whatever its
/// text indicates." So "creatures you control" are the permanents that are creatures and
/// controlled by the ability's controller after layers 2 to 6 have applied, when the effect
/// applies in layer 7c ([CR 613.1]): "all applicable continuous effects are applied in a
/// series of layers".
///
/// "You" is the controller of the permanent with the ability as the layers so far left it
/// ([CR 109.5]): "For a static ability, this is the current controller of the object it's
/// on."
///
/// [CR 611.3a]: https://yawgatog.com/resources/magic-rules/#R6113a
/// [CR 613.1]: https://yawgatog.com/resources/magic-rules/#R6131
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
pub(super) fn static_members<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    at: usize,
    n: u8,
) -> Bitboard<L::Slot> {
    let slot = s.battlefield()[at];
    let ability = &s.copiable(slot).face(t).abilities[usize::from(n)];
    let Some(spec) = ability.layer() else {
        unreachable!("the recompute lists the static abilities that have a layer effect");
    };
    let so_far = SoFar {
        s,
        t,
        at,
        you: s.rows()[at].controller,
    };
    if spec
        .condition
        .as_ref()
        .is_some_and(|condition| !static_ability::holds(s, slot, condition, &so_far))
    {
        return Bitboard::EMPTY;
    }
    let members = match spec.affected {
        Objects::This => Bitboard::single(slot),
        Objects::AttachedTo => s
            .object(slot)
            .attached_to()
            .filter(|&a| s.battlefield_position(a).is_some())
            .map_or(Bitboard::EMPTY, Bitboard::single),
        Objects::All { filter, .. } => {
            let mut members = Bitboard::EMPTY;
            for i in 0..s.battlefield().len() {
                if so_far.passes_at(filter, i) {
                    members.insert(s.battlefield()[i]);
                }
            }
            members
        }
        Objects::Target(_)
        | Objects::Those
        | Objects::TriggeredOn
        | Objects::TriggeredOnIn(_)
        | Objects::ExiledWithThis
        | Objects::InZoneOf { .. }
        | Objects::CardsIn { .. }
        | Objects::Paid(_) => {
            unreachable!("the card table refuses a static ability on any other set")
        }
    };
    if !members.is_empty() {
        crate::coverage::hit(ability);
    }
    members
}

/// Whether a static ability's "as long as" holds, for one that reads only what the layers
/// start from ([`static_ability::reads_results`] says no): the same before and after they
/// run, so read as the recompute lists the ability.
pub(super) fn holds_from_base<L: Layout>(
    s: &Rederive<'_, L>,
    slot: L::Slot,
    condition: &beryl_card::Condition,
) -> bool {
    static_ability::holds(s, slot, condition, &NoResults)
}

/// The layer results so far, read for the static ability of the permanent at position `at`
/// whose controller is `you`.
struct SoFar<'r, 'a, L: Layout> {
    s: &'r Rederive<'a, L>,
    t: &'r GameTable,
    at: usize,
    you: Seat,
}

impl<L: Layout> SoFar<'_, '_, L> {
    fn passes_at(&self, filter: Filter, i: usize) -> bool {
        passes(self.s, self.t, filter, i, self.you)
    }
}

impl<L: Layout> LayerResults for SoFar<'_, '_, L> {
    fn is(&self, filter: Filter) -> bool {
        self.passes_at(filter, self.at)
    }

    fn count(&self, filter: Filter) -> usize {
        (0..self.s.battlefield().len())
            .filter(|&i| self.passes_at(filter, i))
            .count()
    }
}

/// Applies one change to the permanent at battlefield index `i`.
pub(super) fn apply<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    i: usize,
    source: L::Card,
    change: LayerChange,
) {
    match change {
        LayerChange::DefinedPower { face, ability } => {
            define_power(s, t, i, source, face, ability);
        }
        change => modify(t, s.results(), i, source, change),
    }
}

/// Sets the power of the permanent at battlefield index `i` to the amount the static ability
/// at `ability` of the face `face` of `source` defines it as ([CR 613.4a]): "Layer 7a:
/// Effects from characteristic-defining abilities that define power and/or toughness are
/// applied." Layers 2 to 6 are done for every permanent by now, so a count sees each one's
/// final types and controller.
///
/// [CR 613.4a]: https://yawgatog.com/resources/magic-rules/#R6134a
#[inline(never)]
fn define_power<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    i: usize,
    source: L::Card,
    face: FaceIx,
    ability: AbilityIx,
) {
    let said_by = &t.def(source).face(face).abilities[ability.index()];
    let Some(Ok(Some(amount))) = said_by.layer().map(static_ability::defined_power) else {
        unreachable!("the card table has only the defined powers the layers read");
    };
    let power = defined(s, t, amount, s.rows()[i].controller);
    s.results()[i].power = i8::try_from(power).unwrap_or(i8::MAX);
}

/// Applies one change that reads nothing but the permanent it changes to the permanent at
/// battlefield index `i`.
///
/// Out of line, as it was before it was generic over the layout: most recomputes apply
/// nothing, and inlined into the recompute it costs the rollout benchmark 0.07% of its
/// instructions.
#[inline(never)]
fn modify<L: Layout>(
    t: &GameTable,
    derived: &mut [Derived<L>],
    i: usize,
    source: L::Card,
    change: LayerChange,
) {
    let d = &mut derived[i];
    match change {
        // CR 613.1b: "Layer 2: Control-changing effects are applied."
        LayerChange::Control(player) => d.controller = player,
        // CR 613.1d: "Layer 4: Type-changing effects are applied. These include effects that
        // change an object's card type, subtype, and/or supertype."
        LayerChange::AddTypes(types) => d.types = d.types.union(types),
        LayerChange::RemoveTypes(types) => d.types = d.types.minus(types),
        // CR 205.1a: "Some effects set an object's card type. In most such cases, the new
        // card type(s) replaces any existing card types. ... If an object's card type is
        // removed, the subtypes correlated with that card type will remain if they are also
        // the subtypes of a card type the object currently has; otherwise, they are also
        // removed".
        LayerChange::SetTypes(types) => {
            d.types = types;
            d.subtypes = d.subtypes.of_types(t.subtypes(), types);
        }
        LayerChange::AddSubtype(subtype) => {
            d.subtypes = d.subtypes.with(t.subtypes(), subtype);
        }
        // CR 613.1f: "Layer 6: Ability-adding effects, keyword counters, ability-removing
        // effects, and effects that say an object can't have an ability are applied."
        LayerChange::AddKeywords(k) => d.keywords = d.keywords.union(k),
        LayerChange::Grant(grant) => {
            if let Some(free) = d.granted.iter_mut().find(|g| g.is_none()) {
                *free = Some(GrantRef {
                    card: source,
                    grant,
                });
            }
        }
        LayerChange::ModifyPt(p, t) => {
            d.power = d.power.saturating_add(p);
            d.toughness = d.toughness.saturating_add(t);
        }
        LayerChange::DefinedPower { .. } => unreachable!("applied by `define_power`"),
    }
}

/// The value of an amount a characteristic-defining ability defines, read as its layer
/// applies for the permanent's controller `you`: of the permanents as layers 2 to 6 left
/// them, so a count sees each one's final types and controller. The card table has only
/// amounts read this way (`beryl_kernel`'s `static_ability::defined_power`).
fn defined<L: Layout>(s: &Rederive<'_, L>, t: &GameTable, amount: &Amount, you: Seat) -> usize {
    match *amount {
        Amount::Fixed(n) => usize::from(n),
        Amount::Times(n, amount) => usize::from(n).saturating_mul(defined(s, t, amount, you)),
        Amount::Count(&Objects::All { filter, .. }) => (0..s.battlefield().len())
            .filter(|&i| passes(s, t, filter, i, you))
            .count(),
        _ => unreachable!("the card table refuses an amount the layers cannot read"),
    }
}

/// Whether the permanent at battlefield position `i` passes `filter`, read for `you`: what
/// [`Filter`]'s evaluation says of a permanent, of the layer results so far, which the state
/// does not yet hold. Whether it is tapped and its mana value are the object's, which no
/// layer changes.
fn passes<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    filter: Filter,
    i: usize,
    you: Seat,
) -> bool {
    let d = &s.rows()[i];
    match filter {
        Filter::Any => true,
        Filter::Types(types) => d.types.intersects(types),
        Filter::Supertypes(supertypes) => d.supertypes.intersects(supertypes),
        Filter::Subtype(subtype) => d.subtypes.contains(t.subtypes(), subtype),
        Filter::Colors(colors) => d.colors.intersects(colors),
        Filter::Whose(players) => filter::is_for(players, d.controller, you),
        Filter::Tapped => s.object(s.battlefield()[i]).is_tapped(),
        Filter::ManaValueAtMost(n) => s.mana_value(t, s.battlefield()[i]) <= n,
        Filter::Not(filter) => !passes(s, t, *filter, i, you),
        Filter::All(filters) => filters.iter().all(|&filter| passes(s, t, filter, i, you)),
        Filter::AnyOf(filters) => filters.iter().any(|&filter| passes(s, t, filter, i, you)),
        Filter::ManaValueAtMostX => {
            unreachable!("the card table refuses a filter the layers cannot read")
        }
    }
}
