//! What one continuous effect does as its layer applies: what it applies to, found against
//! the results of the layers before it, and the change it makes to each.
//!
//! Implements: CR 613.4

use beryl_kernel::derive::Rederive;
use beryl_kernel::static_ability::{self, LayerResults, NoResults};

use beryl_card::{Amount, Modification, Words};

use crate::bitboard::Bitboard;
use crate::card::Objects;
use crate::card::{BasicLandTypes, Colors, Keywords, Types};
use crate::filter::{self, Filter};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{AbilityMask, Characteristics, Derived, GrantRef, LayerChange, Zone};
use crate::table::GameTable;

/// Who reads a static ability as its layer applies: the controller of its permanent as the
/// layers so far left it ("you", see [CR 109.5]: "For a static ability, this is the current
/// controller of the object it's on"), and the text changes its color words and subtypes are
/// read through ([`beryl_kernel::State::words_of`]).
///
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
#[derive(Clone, Copy)]
pub(super) struct Reader {
    pub(super) you: Seat,
    pub(super) words: Words,
}

/// Applies `change` to each permanent in `members`, in battlefield order. One change to
/// each member commutes with the others, so the order is nothing to the results.
///
/// `by` reads the static ability the change is part of, for an amount the change reads ([`LayerChange::Read`]):
/// read once, unless it is the mana value of each permanent it changes
/// ([`static_ability::reads_each`]).
///
/// [`Full`] if a removal of abilities had no place in a member's mask.
pub(super) fn apply_to_members<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    members: Bitboard<L::Slot>,
    source: L::Card,
    change: LayerChange,
    by: Option<Reader>,
) -> Result<(), Full> {
    if members.is_empty() {
        return Ok(());
    }
    let each = reads_each::<L>(t, source, change);
    let change = settle(s, t, source, change, by);
    let mut applied = Ok(());
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        if members.contains(slot) {
            let change = if each {
                read(s, t, source, change, by, Some(slot))
            } else {
                change
            };
            applied = applied.and(apply(s, t, i, source, change));
        }
    }
    applied
}

/// Whether `change` is a part of a static ability whose amounts read each permanent it
/// changes ([`static_ability::reads_each`]).
fn reads_each<L: Layout>(t: &GameTable, source: L::Card, change: LayerChange) -> bool {
    let LayerChange::Read {
        face,
        ability,
        part,
        ..
    } = change
    else {
        return false;
    };
    t.def(source).face(face).abilities[ability.index()]
        .layer()
        .is_some_and(|spec| static_ability::reads_each(spec, part))
}

/// `change` with any amount it reads ([`LayerChange::Read`]) read now by `by`, which reads
/// the static ability the change is part of: what it does to each permanent it applies to.
pub(super) fn settle<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    source: L::Card,
    change: LayerChange,
    by: Option<Reader>,
) -> LayerChange {
    match change {
        // One that reads each permanent is read as it changes each: what it does depends on
        // nothing the layers change, a permanent's mana value.
        LayerChange::Read { .. } if reads_each::<L>(t, source, change) => change,
        LayerChange::Read { .. } => read(s, t, source, change, by, None),
        change @ (LayerChange::Control(_)
        | LayerChange::AddTypes(_)
        | LayerChange::RemoveTypes(_)
        | LayerChange::SetTypes(_)
        | LayerChange::AddSubtype(_)
        | LayerChange::SetLandTypes(_)
        | LayerChange::AddKeywords(_)
        | LayerChange::Grant(_)
        | LayerChange::SetColors(_)
        | LayerChange::AddColors(_)
        | LayerChange::LoseAllAbilities
        | LayerChange::LoseAbility(_)
        | LayerChange::DefinedPower(_)
        | LayerChange::SetBasePt(..)
        | LayerChange::ModifyPt(..)
        | LayerChange::SwitchPt
        | LayerChange::SetSubtype(_)
        | LayerChange::DefinedPt(..)
        | LayerChange::AddProtection(_)
        | LayerChange::GrantEscape) => change,
    }
}

/// The change `change`, a [`LayerChange::Read`] naming a part of a static ability of
/// `source`, makes, its amounts read now by `by`, which reads the ability, and of `member`, the permanent it changes, for one that reads each
/// ([`static_ability::reads_each`]): a characteristic-defining power ([CR 613.4a]): "Layer 7a: Effects from
/// characteristic-defining abilities that define power and/or toughness are applied", or a
/// base power and toughness ([CR 613.4b]): "Layer 7b: Effects that set power and/or
/// toughness to a specific number or value are applied." The layers before are done for
/// every permanent by now, so a count sees each one's final types and controller.
///
/// [CR 613.4a]: https://yawgatog.com/resources/magic-rules/#R6134a
/// [CR 613.4b]: https://yawgatog.com/resources/magic-rules/#R6134b
#[inline(never)]
fn read<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    source: L::Card,
    change: LayerChange,
    by: Option<Reader>,
    member: Option<L::Slot>,
) -> LayerChange {
    let LayerChange::Read {
        face,
        ability,
        part,
        ..
    } = change
    else {
        unreachable!("only a part whose amounts the layers read is read");
    };
    let said_by = &t.def(source).face(face).abilities[ability.index()];
    let (Some(spec), Some(by)) = (said_by.layer(), by) else {
        unreachable!("a static ability's part names its ability and its controller");
    };
    let number = |amount| i8::try_from(defined(s, t, amount, by, member)).unwrap_or(i8::MAX);
    match spec.modifications[usize::from(part)] {
        Modification::SetPower(power) => LayerChange::DefinedPower(number(power)),
        Modification::SetPt(power, toughness) => {
            LayerChange::DefinedPt(number(power), number(toughness))
        }
        Modification::SetBasePt(power, toughness) => {
            LayerChange::SetBasePt(number(power), number(toughness))
        }
        Modification::Control
        | Modification::AddTypes(_)
        | Modification::RemoveTypes(_)
        | Modification::SetTypes(_)
        | Modification::AddSubtypeWord(_)
        | Modification::SetSubtypeWord(_)
        | Modification::SetLandTypeWords(_)
        | Modification::AddKeywords(_)
        | Modification::Grant(_)
        | Modification::ModifyPt(..)
        | Modification::LoseAllAbilities
        | Modification::LoseAbility(_)
        | Modification::SetColorWords(_)
        | Modification::AddColorWords(_)
        | Modification::AddChosenColor
        | Modification::SwitchPt
        | Modification::AddProtection(_)
        | Modification::GrantEscape => {
            unreachable!("the layers read the amounts of a part that has some")
        }
    }
}

/// The permanents `ability`, a static ability of the permanent at battlefield position `at`,
/// applies to now, or none if its "as long as" does not hold. Both
/// are read against the layer results so far ([CR 611.3a]): "A continuous effect generated
/// by a static ability isn't 'locked in'; it applies at any given moment to whatever its
/// text indicates." So "creatures you control" are the permanents that are creatures and
/// controlled by the ability's controller after layers 2 to 6 have applied, when the effect
/// applies in layer 7c ([CR 613.1]): "all applicable continuous effects are applied in a
/// series of layers".
///
/// "You" is the controller of the permanent with the ability as the layers so far left it
/// (see [CR 109.5]): "For a static ability, this is the current controller of the object it's
/// on."
///
/// [CR 611.3a]: https://yawgatog.com/resources/magic-rules/#R6113a
/// [CR 613.1]: https://yawgatog.com/resources/magic-rules/#R6131
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
pub(super) fn static_members<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    at: usize,
    ability: &'static beryl_card::Ability,
    words: Words,
) -> Bitboard<L::Slot> {
    let slot = s.battlefield()[at];
    let Some(spec) = ability.layer() else {
        unreachable!("the recompute lists the static abilities that have a layer effect");
    };
    let so_far = SoFar {
        s,
        t,
        at,
        by: Reader {
            you: s.rows()[at].controller,
            words,
        },
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
        Objects::All { filter, .. } | Objects::Everywhere(filter) => so_far.select(filter),
        // "Each other": never the permanent with the ability, whatever it has become.
        Objects::Other(&Objects::All { filter, .. }) => {
            let mut members = so_far.select(filter);
            members.remove(slot);
            members
        }
        Objects::Other(_)
        | Objects::Target(_)
        | Objects::Those
        | Objects::TriggeredOn
        | Objects::TriggeredOnIn(_)
        | Objects::Carried
        | Objects::ExiledWithThis
        | Objects::LibraryTop { .. }
        | Objects::ExceptThose(_)
        | Objects::PutOntoBattlefieldWithThis
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
/// by `by`.
struct SoFar<'r, 'a, L: Layout> {
    s: &'r Rederive<'a, L>,
    t: &'r GameTable,
    at: usize,
    by: Reader,
}

impl<L: Layout> SoFar<'_, '_, L> {
    fn passes_at(&self, filter: Filter, i: usize) -> bool {
        passes(self.s, self.t, filter, i, self.by)
    }

    /// The permanents that pass `filter`.
    fn select(&self, filter: Filter) -> Bitboard<L::Slot> {
        let mut members = Bitboard::EMPTY;
        for i in 0..self.s.battlefield().len() {
            if self.passes_at(filter, i) {
                members.insert(self.s.battlefield()[i]);
            }
        }
        members
    }
}

impl<L: Layout> LayerResults for SoFar<'_, '_, L> {
    fn is(&self, filter: Filter) -> bool {
        self.passes_at(filter, self.at)
    }

    fn own_hand(&self) -> u32 {
        self.s.hand_size(self.by.you)
    }

    fn count(&self, filter: Filter) -> usize {
        (0..self.s.battlefield().len())
            .filter(|&i| self.passes_at(filter, i))
            .count()
    }
}

/// Applies one change to the permanent at battlefield index `i`. [`Full`] if it removes an
/// ability the permanent's mask has no place for ([`AbilityMask`]): all the abilities of a
/// face with more than the mask is wide, or one past its width.
pub(super) fn apply<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    i: usize,
    source: L::Card,
    change: LayerChange,
) -> Result<(), Full> {
    match change {
        LayerChange::LoseAllAbilities | LayerChange::SetLandTypes(_)
            if s.definition(s.battlefield()[i]).face(t).abilities.len() > L::AbilityMask::WIDTH =>
        {
            return Err(Full);
        }
        LayerChange::LoseAbility(ability) => {
            let d = &mut s.results()[i];
            d.lost = d.lost.with(ability).ok_or(Full)?;
            return Ok(());
        }
        LayerChange::Control(_)
        | LayerChange::AddTypes(_)
        | LayerChange::RemoveTypes(_)
        | LayerChange::SetTypes(_)
        | LayerChange::AddSubtype(_)
        | LayerChange::SetLandTypes(_)
        | LayerChange::AddKeywords(_)
        | LayerChange::Grant(_)
        | LayerChange::SetColors(_)
        | LayerChange::AddColors(_)
        | LayerChange::LoseAllAbilities
        | LayerChange::DefinedPower(_)
        | LayerChange::SetBasePt(..)
        | LayerChange::ModifyPt(..)
        | LayerChange::SwitchPt
        | LayerChange::Read { .. }
        | LayerChange::SetSubtype(_)
        | LayerChange::DefinedPt(..)
        | LayerChange::AddProtection(_)
        | LayerChange::GrantEscape => {}
    }
    modify(t, s.results(), i, source, change);
    Ok(())
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
        // see CR 205.1a: "Some effects set an object's card type. In most such cases, the new
        // card type(s) replaces any existing card types. ... If an object's card type is
        // removed, the subtypes correlated with that card type will remain if they are also
        // the subtypes of a card type the object currently has; otherwise, they are also
        // removed".
        LayerChange::RemoveTypes(types) => {
            d.types = d.types.minus(types);
            d.subtypes = d.subtypes.of_types(t.subtypes(), d.types);
        }
        LayerChange::SetTypes(types) => {
            d.types = types;
            d.subtypes = d.subtypes.of_types(t.subtypes(), types);
        }
        // see CR 305.7: "If a land gains one or more land types in addition to its own, it keeps
        // its land types and rules text, and it gains the new land types and mana
        // abilities." A basic land type it already has gives it nothing more.
        LayerChange::AddSubtype(subtype) => {
            if let Some(basic) = BasicLandTypes::of_subtype(subtype)
                && d.types.intersects(Types::LAND)
                && !d.subtypes.contains(t.subtypes(), subtype)
            {
                d.keywords = d.keywords.union(basic.mana());
            }
            d.subtypes = d.subtypes.with(t.subtypes(), subtype);
        }
        // see CR 205.1a: "when an effect sets one or more of an object's subtypes, the new
        // subtype(s) replaces any existing subtypes from the appropriate set (creature types,
        // land types, artifact types, enchantment types, planeswalker types, or spell types)."
        LayerChange::SetSubtype(subtype) => {
            let kept = d.subtypes.without_types(t.subtypes(), subtype.card_type());
            d.subtypes = kept.with(t.subtypes(), subtype);
        }
        // see CR 305.7: "If an effect sets a land's subtype to one or more of the basic land
        // types, the land no longer has its old land type. It loses all abilities generated
        // from its rules text, its old land types, and any copiable effects affecting that
        // land, and it gains the appropriate mana ability for each new basic land type." Its
        // other subtypes, a land creature's creature types, stay. In layer 4 no effect has
        // granted it anything yet, so all it has is its rules text's and what a copy effect's
        // exception gave it in layer 1, kept with the granted ones (`State::copy_grants`).
        LayerChange::SetLandTypes(types) => {
            d.granted = [None; 4];
            let mut subtypes = d
                .subtypes
                .of_types(t.subtypes(), d.types.minus(Types::LAND));
            for subtype in types.subtypes() {
                subtypes = subtypes.with(t.subtypes(), subtype);
            }
            d.subtypes = subtypes;
            d.lost = L::AbilityMask::ALL;
            d.keywords = types.mana();
        }
        // CR 613.1f: "Layer 6: Ability-adding effects, keyword counters, ability-removing
        // effects, and effects that say an object can't have an ability are applied."
        LayerChange::AddKeywords(k) => d.keywords = d.keywords.union(k),
        LayerChange::AddProtection(colors) => d.protection = d.protection.union(colors),
        // see CR 702.138a: escape "functions while the card with escape is in a player's
        // graveyard", so a permanent that has it gets nothing a rule reads.
        LayerChange::GrantEscape => {}
        LayerChange::Grant(grant) => {
            let mut granted = d.granted;
            if let Some(free) = granted.iter_mut().find(|g| g.is_none()) {
                *free = Some(GrantRef {
                    card: source,
                    grant,
                });
            }
            d.granted = granted;
        }
        // CR 613.1e: "Layer 5: Color-changing effects are applied."
        LayerChange::SetColors(colors) => d.colors = colors,
        LayerChange::AddColors(colors) => d.colors = d.colors.union(colors),
        // CR 613.1f: an ability-removing effect removes the abilities it has now: its
        // face's and those granted by effects applied before it in timestamp order. An
        // effect applied after it still adds its own.
        LayerChange::LoseAllAbilities => {
            d.lost = L::AbilityMask::ALL;
            d.keywords = Keywords::NONE;
            d.protection = Colors::NONE;
            d.granted = [None; 4];
        }
        LayerChange::LoseAbility(_) => unreachable!("applied by `apply`"),
        // CR 613.4a: "Layer 7a: Effects from characteristic-defining abilities that define
        // power and/or toughness are applied."
        LayerChange::DefinedPower(power) => d.power = power,
        // The same in layer 7a, and CR 613.4b: "Layer 7b: Effects that set power and/or
        // toughness to a specific number or value are applied."
        LayerChange::DefinedPt(power, toughness) | LayerChange::SetBasePt(power, toughness) => {
            d.power = power;
            d.toughness = toughness;
        }
        LayerChange::ModifyPt(p, t) => {
            d.power = d.power.saturating_add(p);
            d.toughness = d.toughness.saturating_add(t);
        }
        // CR 613.4d: "Layer 7d: Effects that switch a creature's power and toughness are
        // applied."
        LayerChange::SwitchPt => {
            let (power, toughness) = (d.power, d.toughness);
            d.power = toughness;
            d.toughness = power;
        }
        LayerChange::Read { .. } => unreachable!("settled by `apply_to_members`"),
    }
}

/// The value of an amount a static ability's part reads as its layer applies, for the
/// reader `by` of the ability: of the permanents as layers 2 to 6
/// left them, so a count sees each one's final types and controller. "Its mana value" is
/// that of `member`, the permanent the part changes (see [CR 202.3]): "The mana value of an
/// object is a number equal to the total amount of mana in its mana cost, regardless of
/// color." No layer changes it. The card table has only amounts read this way
/// (`beryl_kernel`'s `static_ability::read_amounts`).
///
/// [CR 202.3]: https://yawgatog.com/resources/magic-rules/#R2023
fn defined<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    amount: &Amount,
    by: Reader,
    member: Option<L::Slot>,
) -> usize {
    match *amount {
        Amount::Fixed(n) => usize::from(n),
        Amount::Times(n, amount) => {
            usize::from(n).saturating_mul(defined(s, t, amount, by, member))
        }
        Amount::ManaValue(Objects::Those) => {
            let Some(member) = member else {
                unreachable!("a part that reads each permanent is read for each");
            };
            usize::from(s.mana_value(t, member))
        }
        Amount::Plus(n, amount) => defined(s, t, amount, by, member).saturating_add(usize::from(n)),
        Amount::Half { of, round_up } => {
            let whole = defined(s, t, of, by, member);
            if round_up {
                whole.div_ceil(2)
            } else {
                whole / 2
            }
        }
        Amount::Count(&Objects::All { filter, .. }) => (0..s.battlefield().len())
            .filter(|&i| passes(s, t, filter, i, by))
            .count(),
        Amount::CardTypesAmong(_) => s.card_types_in_graveyards(t),
        Amount::X
        | Amount::ThatMany
        | Amount::Carried
        | Amount::ThoseOf(_)
        | Amount::Count(_)
        | Amount::Power(_)
        | Amount::Toughness(_)
        | Amount::ManaValue(_)
        | Amount::Counters(..)
        | Amount::Life(_)
        | Amount::Most(..)
        | Amount::SpellsCast(_)
        | Amount::CardsDrawn(_)
        | Amount::SpellsCastBefore => {
            unreachable!("the card table refuses an amount the layers cannot read")
        }
    }
}

/// Whether the permanent at battlefield position `i` passes `filter`, read by `by`: what
/// [`Filter`]'s evaluation says of a permanent, of the layer results so far, which the state
/// does not yet hold. Whether it is tapped and its mana value are the object's, which no
/// layer changes.
fn passes<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    filter: Filter,
    i: usize,
    by: Reader,
) -> bool {
    let d = &s.rows()[i];
    match filter {
        Filter::Any => true,
        Filter::Types(types) => d.types.intersects(types),
        Filter::Supertypes(supertypes) => d.supertypes.intersects(supertypes),
        Filter::SubtypeWord(word) => d.subtypes.contains(t.subtypes(), word.read(&by.words)),
        Filter::ColorWords(colors) => d.colors.intersects(colors.read(&by.words)),
        Filter::Whose(players) => filter::is_for(players, d.controller, by.you),
        Filter::Owned(players) => {
            filter::is_for(players, s.object(s.battlefield()[i]).owner(), by.you)
        }
        Filter::Tapped => s.object(s.battlefield()[i]).is_tapped(),
        Filter::Token => s.object(s.battlefield()[i]).is_token(),
        Filter::ManaValueAtMost(n) => s.mana_value(t, s.battlefield()[i]) <= n,
        // The layers apply to permanents, which are on the battlefield.
        Filter::InZone(zone) => zone == Zone::Battlefield,
        Filter::Not(filter) => !passes(s, t, *filter, i, by),
        Filter::All(filters) => filters.iter().all(|&filter| passes(s, t, filter, i, by)),
        Filter::AnyOf(filters) => filters.iter().any(|&filter| passes(s, t, filter, i, by)),
        Filter::ManaValueAtMostX | Filter::PowerAtMost(_) | Filter::AttackedThisTurn => {
            unreachable!("the card table refuses a filter the layers cannot read")
        }
    }
}

/// The characteristics `filter` reads of a permanent's layer results ([`passes`]): whether it
/// is tapped, is a token and its mana value are the object's, which no layer changes.
pub(super) fn filter_reads(filter: Filter) -> Characteristics {
    match filter {
        Filter::Any
        | Filter::Owned(_)
        | Filter::Tapped
        | Filter::AttackedThisTurn
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::InZone(_) => Characteristics::NONE,
        Filter::Types(_) => Characteristics::TYPES,
        Filter::Supertypes(_) => Characteristics::SUPERTYPES,
        Filter::SubtypeWord(_) => Characteristics::SUBTYPES,
        Filter::ColorWords(_) => Characteristics::COLORS,
        Filter::Whose(_) => Characteristics::CONTROLLER,
        // The card table refuses it in a layer effect ([`passes`]), but it would read power.
        Filter::PowerAtMost(_) => Characteristics::PT,
        Filter::Not(filter) => filter_reads(*filter),
        Filter::All(filters) | Filter::AnyOf(filters) => filters
            .iter()
            .fold(Characteristics::NONE, |r, &f| r.union(filter_reads(f))),
    }
}

/// The characteristics finding whether a static ability with the continuous effect `spec`
/// exists, and what it applies to, read of the layer results: whether its permanent has lost
/// it, which only a removal of abilities changes; what its filter reads, for a set "each
/// other" or not; and, for an "as long as" that reads the results, what filters read. The
/// test of what may depend on what ([`super::dependency`]) and of whether the order of
/// permanents entering together matters ([`super::together`]) both ask it, so the two never
/// disagree.
pub(super) fn existence_reads(spec: &beryl_card::LayerSpec) -> Characteristics {
    let mut reads = Characteristics::REMOVES_ABILITIES;
    if let Objects::All { filter, .. }
    | Objects::Everywhere(filter)
    | Objects::Other(&Objects::All { filter, .. }) = spec.affected
    {
        reads = reads.union(filter_reads(filter));
    }
    if (spec.condition.as_ref()).is_some_and(static_ability::reads_results) {
        reads = reads.union(Characteristics::FILTERED);
    }
    reads
}

/// The characteristics a static ability's change reads to get its amounts as the layer applies.
pub(super) fn amount_reads(spec: &beryl_card::LayerSpec, change: LayerChange) -> Characteristics {
    fn of(amount: &Amount) -> Characteristics {
        match *amount {
            // A number, a permanent's mana value, which no layer changes, or the cards in
            // graveyards, which have no layer results.
            Amount::Fixed(_) | Amount::ManaValue(Objects::Those) | Amount::CardTypesAmong(_) => {
                Characteristics::NONE
            }
            Amount::Times(_, amount)
            | Amount::Plus(_, amount)
            | Amount::Half { of: amount, .. } => of(amount),
            Amount::Count(&Objects::All { filter, .. }) => filter_reads(filter),
            Amount::X
            | Amount::ThatMany
            | Amount::Carried
            | Amount::ThoseOf(_)
            | Amount::Count(_)
            | Amount::Power(_)
            | Amount::Toughness(_)
            | Amount::ManaValue(_)
            | Amount::Counters(..)
            | Amount::Life(_)
            | Amount::Most(..)
            | Amount::SpellsCast(_)
            | Amount::CardsDrawn(_)
            | Amount::SpellsCastBefore => Characteristics::FILTERED,
        }
    }
    let part = match change {
        LayerChange::Read { part, .. } => part,
        LayerChange::Control(_)
        | LayerChange::AddTypes(_)
        | LayerChange::RemoveTypes(_)
        | LayerChange::SetTypes(_)
        | LayerChange::AddSubtype(_)
        | LayerChange::SetLandTypes(_)
        | LayerChange::AddKeywords(_)
        | LayerChange::Grant(_)
        | LayerChange::SetColors(_)
        | LayerChange::AddColors(_)
        | LayerChange::LoseAllAbilities
        | LayerChange::LoseAbility(_)
        | LayerChange::DefinedPower(_)
        | LayerChange::SetBasePt(..)
        | LayerChange::ModifyPt(..)
        | LayerChange::SwitchPt
        | LayerChange::SetSubtype(_)
        | LayerChange::DefinedPt(..)
        | LayerChange::AddProtection(_)
        | LayerChange::GrantEscape => return Characteristics::NONE,
    };
    match static_ability::amounts_of(spec, part) {
        (power, None) => of(power),
        (power, Some(toughness)) => of(power).union(of(toughness)),
    }
}

/// Layer 7c's counters ([CR 613.4c]): "Effects and counters that modify power and/or
/// toughness (but don't set power and/or toughness to a specific number or value) are
/// applied." Additions commute, so they follow the layer's effects.
///
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
#[inline]
pub(super) fn counters<L: Layout>(s: &mut Rederive<'_, L>) {
    for i in 0..s.battlefield().len() {
        let c = s.object(s.battlefield()[i]).counters();
        let d = &mut s.results()[i];
        let delta = i16::from(c.plus_one) - i16::from(c.minus_one);
        let adjust = |value| {
            let adjusted = i16::from(value) + delta;
            i8::try_from(adjusted.clamp(i16::from(i8::MIN), i16::from(i8::MAX)))
                .expect("clamped to i8")
        };
        d.power = adjust(d.power);
        d.toughness = adjust(d.toughness);
    }
}
