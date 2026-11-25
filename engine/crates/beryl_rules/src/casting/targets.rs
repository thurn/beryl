//! Choosing targets (CR 601.2c), what the target being announced is chosen for, and whether
//! targets are still legal (CR 115.3, see CR 608.2b).
//!
//! Implements: CR 115.3, CR 115.5

use crate::PlayerSet;
use crate::bitboard::Bitboard;
use crate::card::{Abilities, Ability, Takes, TargetKind, TargetSpec, Words};
use crate::continuous::rule_change;
use crate::filter::{self, Evaluate, Filter};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::replacement::event::Sink;
use crate::replacement::protection;
use crate::state::{
    AbilityTarget, CastKind, CastStep, Casting, Pending, StackEntry, StackItem, State, Target,
    TargetPlace, Zone, placed,
};
use crate::table::{Capability, GameTable};

use super::steps::{ability_event, advance, specs, top};

/// The objects, players and abilities on the stack a target spec can choose. A spec of most
/// kinds chooses only one of them, and the others are empty; "any target" chooses from
/// objects and players.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TargetSet<L: Layout = Small> {
    pub objects: Bitboard<L::Slot>,
    pub players: PlayerSet,
    /// The abilities, a bit for each place on the stack, counted from the bottom. A stack
    /// deeper than 64 entries needs the large layout, whose abilities above the 64th place
    /// are never candidates.
    pub abilities: u64,
}

/// The spell or ability whose targets are chosen or checked, which is an illegal target for
/// itself ([CR 115.5]): "A spell or ability on the stack is an illegal target for itself."
/// `spell` is the spell, if it is one; `on_stack` says whether its entry is the top of the
/// stack, as it is while its targets are chosen, where an ability target can name only the
/// entries below it. Before it is cast it is not yet on the stack, but it will have left the
/// zone it is in when its targets are chosen. During resolution its entry is held separately.
///
/// [CR 115.5]: https://yawgatog.com/resources/magic-rules/#R1155
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Itself<L: Layout = Small> {
    pub spell: Option<L::Slot>,
    pub on_stack: bool,
}

/// Nothing on the stack yet: an ability being activated, or a copy being made.
impl<L: Layout> Default for Itself<L> {
    fn default() -> Self {
        Itself {
            spell: None,
            on_stack: false,
        }
    }
}

impl<L: Layout> Itself<L> {
    /// What `entry`, the top of the stack, chooses targets for.
    fn top(entry: &StackEntry<L>) -> Itself<L> {
        Itself {
            spell: entry.spell().map(|r| r.slot),
            on_stack: true,
        }
    }

    /// What the resolving `entry` checks its targets for.
    fn resolving(entry: &StackEntry<L>) -> Itself<L> {
        Itself {
            spell: entry.spell().map(|r| r.slot),
            on_stack: false,
        }
    }
}

/// Whether a target spec can choose `player`: a spec for "target player" or "target opponent"
/// asks its filter, and one for "any target" may choose every player, since its filter is
/// for the permanents.
pub(crate) fn player_matches(spec: &TargetSpec, player: Seat, you: Seat) -> bool {
    match spec.kind {
        TargetKind::Player => match spec.filter {
            Filter::Any => true,
            Filter::Whose(players) => filter::is_for(players, player, you),
            Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::SubtypeWord(_)
            | Filter::ColorWords(_)
            | Filter::Owned(_)
            | Filter::AttackedThisTurn
            | Filter::Tapped
            | Filter::Token
            | Filter::ManaValueAtMost(_)
            | Filter::PowerAtMost(_)
            | Filter::ManaValueAtMostX
            | Filter::ManaValueAtMostPower
            | Filter::Not(_)
            | Filter::All(_)
            | Filter::AnyOf(_)
            | Filter::InZone(_) => unreachable!("a player filter is Any or Whose"),
        },
        TargetKind::PermanentOrPlayer => true,
        TargetKind::Permanent
        | TargetKind::Spell
        | TargetKind::SpellOrPermanent
        | TargetKind::GraveyardCard
        | TargetKind::Ability
        | TargetKind::ActivatedAbility
        | TargetKind::TriggeredAbility => false,
    }
}

/// What a spec that targets abilities of `kinds` chooses from, for `itself`: out of line, so
/// that a game without such a spec runs the object and player paths as before.
#[cold]
#[inline(never)]
fn ability_domain<L: Layout>(s: &State<L>, kind: TargetKind, itself: Itself<L>) -> TargetSet<L> {
    let kinds = kind.abilities().expect("an ability");
    let below = s.stack().len() - usize::from(itself.on_stack);
    TargetSet {
        objects: Bitboard::EMPTY,
        players: PlayerSet::NOBODY,
        abilities: abilities(s, kinds, below),
    }
}

/// The activated and triggered abilities of `kinds` on the stack below `below`, by place
/// ([`TargetSet::abilities`]). Mana abilities never go on the stack (see CR 605.3b).
fn abilities<L: Layout>(s: &State<L>, kinds: Abilities, below: usize) -> u64 {
    let mut places = 0;
    for (place, entry) in s.stack().iter().enumerate().take(below.min(64)) {
        let triggered = match entry.item {
            StackItem::Spell { .. } => continue,
            StackItem::Activated { .. } => false,
            StackItem::Triggered { .. } => true,
        };
        if kinds.include(triggered) {
            places |= 1 << place;
        }
    }
    places
}

/// The spells that have entries in the stack list, as objects.
pub(crate) fn spells<L: Layout>(s: &State<L>) -> Bitboard<L::Slot> {
    s.stack()
        .iter()
        .filter_map(StackEntry::spell)
        .map(|r| r.slot)
        .collect()
}

/// The objects, players or abilities a target spec can choose now, for a spell or ability
/// controlled by `you`, which is `itself`. `x` is the value of X of the spell or ability.
/// `source` is an ability's source, which "another target" can't choose.
pub(super) fn candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Itself<L>,
    x: u8,
    source: Option<L::Slot>,
) -> TargetSet<L> {
    candidates_through(
        s,
        t,
        spec,
        you,
        itself,
        x,
        source,
        printed_on(s, itself, source),
    )
}

/// [`candidates`] with the spec's words read through `words`: for a spell or ability on the
/// stack, those it reads its text through ([`State::entry_words`]).
#[allow(clippy::too_many_arguments)]
fn candidates_through<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Itself<L>,
    x: u8,
    source: Option<L::Slot>,
    words: Words,
) -> TargetSet<L> {
    let TargetSet {
        objects,
        players,
        abilities,
    } = domain(s, t, spec, you, itself, source);
    TargetSet {
        objects: match compared_with(s, t, spec, x, source) {
            Some(x) if !objects.is_empty() => spec.filter.select_x(s, t, objects, you, &words, x),
            Some(_) | None => Bitboard::EMPTY,
        },
        players,
        abilities,
    }
}

/// The number a target filter compares mana values with: X, or for a filter that compares
/// with its source's power ([`Filter::ManaValueAtMostPower`]), that power, read as the
/// filter is. None where no object can pass, as below a negative power.
#[inline]
fn compared_with<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    x: u8,
    source: Option<L::Slot>,
) -> Option<u8> {
    if t.can(Capability::ReadsSourcePower) && spec.filter.reads_source_power() {
        source_power(s, t, source)
    } else {
        Some(x)
    }
}

/// The power of an ability's source, its last known power once it has left the battlefield
/// (see [CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "if it's no longer in
/// that zone ... the effect uses the object's last known information." None for a negative
/// power or no source.
#[cold]
#[inline(never)]
fn source_power<L: Layout>(s: &State<L>, t: &GameTable, source: Option<L::Slot>) -> Option<u8> {
    let slot = source?;
    let power = if s.object(slot).on_battlefield() {
        s.power(t, slot)
    } else {
        (s.last_known_rows().iter().rev())
            .find(|row| row.object.slot == slot)
            .map(|row| row.derived.power)?
    };
    u8::try_from(power).ok()
}

/// What [`candidates`] chooses from: the players and abilities a spec can choose, and the
/// objects its filter is still to pass on. A permanent with protection from the spell, or from
/// the ability's source, is none of them ([`protection::targetable_by`]).
///
/// A resolving spell's object stays in the stack zone while its entry is held separately
/// (see [CR 608.2n]): "As the final part of an instant or sorcery spell's resolution, the
/// spell is put into its owner's graveyard." Use zone membership so a copy can choose a
/// resolving spell as a new target.
///
/// [CR 608.2n]: https://yawgatog.com/resources/magic-rules/#R6082n
fn domain<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Itself<L>,
    source: Option<L::Slot>,
) -> TargetSet<L> {
    let players = match spec.kind {
        TargetKind::Player | TargetKind::PermanentOrPlayer => {
            // See CR 702.16j: a player with protection from everything "can't be targeted by
            // spells or abilities".
            s.players().filter(|p| {
                player_matches(spec, p, you) && !rule_change::protected_from_everything(s, t, p)
            })
        }
        TargetKind::Permanent
        | TargetKind::Spell
        | TargetKind::SpellOrPermanent
        | TargetKind::GraveyardCard
        | TargetKind::Ability
        | TargetKind::ActivatedAbility
        | TargetKind::TriggeredAbility => PlayerSet::NOBODY,
    };
    let mut within = match spec.kind {
        TargetKind::Player => Bitboard::EMPTY,
        TargetKind::Permanent | TargetKind::PermanentOrPlayer => s.in_zone(Zone::Battlefield),
        TargetKind::Spell => s.in_zone(Zone::Stack),
        TargetKind::SpellOrPermanent => s.in_zone(Zone::Stack) | s.in_zone(Zone::Battlefield),
        TargetKind::GraveyardCard => s.in_zone(Zone::Graveyard),
        TargetKind::Ability | TargetKind::ActivatedAbility | TargetKind::TriggeredAbility => {
            return ability_domain(s, spec.kind, itself);
        }
    };
    if let Some(itself) = itself.spell {
        within.remove(itself);
    }
    if let Some(source) = source.filter(|_| spec.other) {
        within.remove(source);
    }
    TargetSet {
        objects: protection::targetable_by(s, t, within, source.or(itself.spell)),
        players,
        abilities: 0,
    }
}

/// Whether [`candidates`] holds at least `spec.count` targets, found without testing the
/// filter on more objects than that takes.
fn enough_candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Itself<L>,
    x: u8,
    source: Option<L::Slot>,
) -> bool {
    let TargetSet {
        objects,
        players,
        abilities,
    } = domain(s, t, spec, you, itself, source);
    let found = players.count() + abilities.count_ones();
    let Some(need) = usize::from(spec.required())
        .checked_sub(usize::try_from(found).expect("at most the players and stack abilities"))
    else {
        return true;
    };
    let words = printed_on(s, itself, source);
    let Some(x) = compared_with(s, t, spec, x, source) else {
        return false;
    };
    objects.at_least(need, |slot| {
        spec.filter.matches_x(s, t, slot, you, &words, x)
    })
}

/// The text changes of the object the target spec is printed on: the ability's source, or
/// the spell being cast ([`State::words_of`]).
fn printed_on<L: Layout>(s: &State<L>, itself: Itself<L>, source: Option<L::Slot>) -> Words {
    source
        .or(itself.spell)
        .map_or(Words::NONE, |slot| s.words_of(slot))
}

impl<L: Layout> TargetSet<L> {
    pub(super) fn count(self) -> u32 {
        self.objects.count() + self.players.count() + self.abilities.count_ones()
    }

    /// Takes out `target`, already chosen for the same instance of the word "target", which
    /// can't be chosen again ([CR 115.3]).
    ///
    /// [CR 115.3]: https://yawgatog.com/resources/magic-rules/#R1153
    fn remove(&mut self, target: Target<L>) {
        if let Some(r) = target.as_object() {
            self.objects.remove(r.slot);
        }
        if let Some(player) = target.as_player() {
            self.players.remove(player);
        }
        if let Some(AbilityTarget::At(place)) = target.as_ability() {
            self.abilities &= !(1 << place);
        }
    }
}

/// Whether every required target can be chosen
/// ([CR 115.3](https://yawgatog.com/resources/magic-rules/#R1153)): "The same target can't be
/// chosen multiple times for any one instance of the word "target" on a spell or ability."
pub(super) fn targetable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    ability: Option<&Ability>,
    you: Seat,
    itself: Itself<L>,
    x: u8,
    source: Option<L::Slot>,
) -> bool {
    ability.is_none_or(|a| {
        a.targets().iter().all(|spec| {
            (spec.optional && spec.minimum == 0)
                || enough_candidates(s, t, spec, you, itself, x, source)
        })
    })
}

/// The slot of an entry's ability source; a spell has none.
pub(super) fn source_of<L: Layout>(entry: &StackEntry<L>) -> Option<L::Slot> {
    entry.source().map(|r| r.slot)
}

/// Whether a chosen target is still legal as its spell or ability resolves
/// (see [CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "A target that's no
/// longer in the zone it was in when it was targeted is illegal. Other changes to the game
/// state may cause a target to no longer be legal". A zone change gives the object a new
/// incarnation, so a stale reference is illegal, and an ability that has left the stack is
/// gone ([`AbilityTarget::Gone`]). A player who gains protection from everything "can't be
/// targeted by spells or abilities" (see [CR 702.16j]).
///
/// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
pub(crate) fn still_legal<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
    target: Target<L>,
) -> bool {
    let spec = &ability.targets()[target.spec().index()];
    let you = entry.controller();
    let itself = Itself::resolving(entry);
    let Some(r) = target.as_object() else {
        if let Some(player) = target.as_player() {
            return player_matches(spec, player, you)
                && !rule_change::protected_from_everything(s, t, player);
        }
        return ability_still_legal(s, t, entry, spec, target);
    };
    if !s.is_current(r) {
        return false;
    }
    let words = s.entry_words(entry);
    (candidates_through(s, t, spec, you, itself, entry.x, source_of(entry), words).objects)
        .contains(r.slot)
}

/// [`still_legal`] for an ability target: out of line, as [`ability_domain`] is.
#[cold]
#[inline(never)]
fn ability_still_legal<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    spec: &TargetSpec,
    target: Target<L>,
) -> bool {
    let Some(AbilityTarget::At(place)) = target.as_ability() else {
        return false;
    };
    let itself = Itself::resolving(entry);
    let you = entry.controller();
    let set = candidates(s, t, spec, you, itself, entry.x, source_of(entry));
    place < 64 && set.abilities & (1 << place) != 0
}

/// The new targets a copy of a spell may have in place of its target at `place`, for its
/// controller to choose among as the copy is made, before it is on the stack
/// (see [CR 707.10c]): "If the player chooses to change some or all of the targets, the new targets
/// must be legal. Once the player has decided what the copy's targets will be, the copy is put
/// onto the stack with those targets." The candidates of that target's spec now, less the
/// targets `entry`, the copy's entry, has for the same spec: the same target can't be chosen
/// twice for one instance of the word "target" ([CR 115.3]). The target at `place` itself is
/// among them only if it is still legal; the caller offers to keep it either way.
///
/// [CR 707.10c]: https://yawgatog.com/resources/magic-rules/#R70710c
/// [CR 115.3]: https://yawgatog.com/resources/magic-rules/#R1153
pub(crate) fn new_targets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
    place: TargetPlace,
) -> TargetSet<L> {
    let spec_ix = entry.targets.as_slice()[place.index()].spec();
    let spec = &ability.targets()[spec_ix.index()];
    // Not yet on the stack, the copy is no candidate, and every entry there is below it.
    let itself = Itself::default();
    let mut set = candidates(s, t, spec, entry.controller(), itself, entry.x, None);
    for (other, target) in placed(entry.targets.as_slice()) {
        if other != place && target.spec() == spec_ix {
            set.remove(target);
        }
    }
    set
}

/// Chooses the targets of the triggered ability just put on the stack, if it has any.
///
/// See [CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d): "The remainder of the
/// process for putting a triggered ability on the stack is identical to the process for
/// casting a spell listed in rules 601.2c-d. If a choice is required when the triggered
/// ability goes on the stack but no legal choices can be made for it, ... the ability is
/// simply removed from the stack."
pub(crate) fn target_trigger<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let entry = top(s);
    let Some(ability) = entry.ability_in(t) else {
        return Ok(());
    };
    if ability.targets().is_empty() {
        // The trigger is already on the stack, with no choices to announce first
        // (see [CR 603.3](https://yawgatog.com/resources/magic-rules/#R6033)):
        // "its controller puts it on the stack as an object that's not a card".
        sink.note(ability_event(entry));
        return Ok(());
    }
    if !targetable(
        s,
        t,
        Some(ability),
        entry.controller(),
        Itself::top(entry),
        entry.x,
        source_of(entry),
    ) {
        s.take_top_of_stack();
        s.settle_captured_words();
        sink.departed(|| s.stack().len());
        return Ok(());
    }
    let c = Casting::new(entry.controller(), CastKind::Trigger, CastStep::TargetCount);
    advance(s, t, sink, c)
}

/// The objects or players the current target spec can still choose: not one already chosen
/// for it.
pub(super) fn target_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
) -> TargetSet<L> {
    let entry = top(s);
    let spec = &specs(t, entry)[c.spec.index()];
    let itself = Itself::top(entry);
    let mut set = candidates(s, t, spec, c.player, itself, entry.x, source_of(entry));
    for &target in entry.targets.iter().filter(|x| x.spec() == c.spec) {
        set.remove(target);
    }
    set
}

/// Whether the effect whose target is being announced
/// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "The player announces
/// their choice of an appropriate object or player for each target the spell requires") would
/// take the object in `slot` from its controller, were it chosen
/// ([`Ability::takes_target`](crate::card::Ability::takes_target)): a permanent removed from
/// the battlefield, or a spell countered
/// (see [CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a): "To counter a spell or
/// ability means to cancel it, removing it from the stack. It doesn't resolve and none of
/// its effects occur. A countered spell is put into its owner's graveyard."). This describes
/// the choice: it neither runs the effect nor says whether the object is a legal target.
pub fn target_takes<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let Pending::Casting(c) = *s.pending() else {
        return false;
    };
    let announcing = matches!(c.step, CastStep::TargetCount | CastStep::Target { .. });
    let ability = s.stack().last().and_then(|entry| entry.ability_in(t));
    let how = match s.object(slot).zone() {
        Zone::Battlefield => Takes::Removal,
        Zone::Stack => Takes::Counter,
        Zone::Library
        | Zone::Hand
        | Zone::Graveyard
        | Zone::Exile
        | Zone::Nowhere
        | Zone::Merged => return false,
    };
    announcing && ability.is_some_and(|ability| ability.takes_target(c.spec, how))
}
