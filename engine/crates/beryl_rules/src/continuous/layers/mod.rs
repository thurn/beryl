//! Derived characteristics: continuous effects applied in layers, recomputed from base when
//! dirty (see CR 613).
//!
//! Implements: CR 611.3, CR 612.3, CR 613.1, CR 613.2a, CR 613.3, CR 613.6

mod apply;
mod copied;
mod dependency;
pub(crate) mod entering;
mod granted;
mod merged;
mod other_zones;
mod player_rules;
mod text;
mod together;

use player_rules::player_rules;
use text::reader;

pub(crate) use entering::{entered, lost_entering};
pub(crate) use together::order_may_matter;

use beryl_card::MODIFY_PARTS;
use beryl_kernel::derive::Rederive;

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, Colors, FaceIx, GrantIx, Layer, LayerSpec, Modification, Objects, Words,
};
use crate::decision::{Outcome, StallReason};
use crate::fixed::Full;
use crate::ids::CardId;
use crate::layout::{Layout, Queue, index};
use crate::replacement::event::{Listens, Replaces, Sink};
use crate::state::{
    AbilityMask, Derived, Flag, GrantRef, LayerChange, LockedIn, Permanents, Rules, State, Zone,
    read_layer,
};
use crate::static_ability::{self, Inputs};
use crate::table::Capability;
use crate::table::GameTable;
use granted::{GRANTED, Granted, GrantedTo, PASSES, granted_ability, granted_layer_effects, slice};

/// What one continuous effect changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Target<L: Layout> {
    /// The permanent at this battlefield position. The layout's index names every position
    /// of its battlefield, however many permanents that holds.
    Position(L::Index),
    /// Each permanent the effect locked in as it began that is still the object it was
    /// (see [CR 611.2c]): "the set of objects it affects is determined when that continuous effect
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
    layer: Layer,
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
}

impl<L: Layout> Apply<L> {
    /// Where it comes among the effects (see [CR 613.7]): "Within a layer or sublayer, determining
    /// which order effects are applied in is usually done using a timestamp system." Static
    /// abilities of permanents that share a timestamp apply in the order of the permanents they
    /// change. An effect on a set needs no such order: it has a timestamp of its own, and one
    /// change to each member commutes.
    ///
    /// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
    fn key(&self) -> (Layer, u16, u8, u8, u16) {
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
            layer: Layer::PtModifying,
            timestamp: 0,
            order: 0,
            part: 0,
            target: Target::Position(L::Index::default()),
            source: L::Card::FIRST,
            change: LayerChange::ModifyPt(0, 0),
            from: None,
            parts: 1,
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
        return recompute(s, t, &mut sink.work.dependency_trials);
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
/// pass. A face-down permanent has none of either, whichever card it is (see [CR 708.2a]): it has "no
/// text, no name, no subtypes, and no mana cost". So it counts for nothing
/// ([`crate::query::Copiable::may_listen`]), and masking or determinizing its hidden identity
/// leaves both unchanged.
///
/// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
pub(crate) fn listening_of_all<L: Layout>(s: &State<L>, t: &GameTable) -> (Listens, Replaces) {
    let merges = t.can(Capability::Merges);
    s.battlefield()
        .iter()
        .fold((Listens::NONE, Replaces::NONE), |(l, r), &slot| {
            let what = s.definition(slot);
            let (l, r) = (l | what.may_listen(t), r | what.may_replace(t));
            if merges {
                merged::masks(s, t, slot, (l, r))
            } else {
                (l, r)
            }
        })
}

/// [`listening_of_all`], leaving out the permanents that `rows`, the layer results, say have
/// lost every ability.
pub(crate) fn listening<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rows: &[Derived<L>],
) -> (Listens, Replaces) {
    let merges = t.can(Capability::Merges);
    s.battlefield().iter().zip(rows).fold(
        (Listens::NONE, Replaces::NONE),
        |(l, r), (&slot, row)| {
            // A permanent that lost all its abilities listens to nothing and replaces
            // nothing (CR 613.1f); one that lost some keeps its card's masks, which say what
            // it may do and are read again ability by ability.
            if { row.lost } == L::AbilityMask::ALL {
                return (l, r);
            }
            let what = s.definition(slot);
            let (l, r) = (l | what.may_listen(t), r | what.may_replace(t));
            if merges {
                merged::masks(s, t, slot, (l, r))
            } else {
                (l, r)
            }
        },
    )
}

/// Recomputes every permanent's characteristics from base ([CR 613.1]): "The values of an object's
/// characteristics are determined by starting with the actual object. For a card, that means the
/// values of the characteristics printed on that card. For a token or a copy of a spell or card,
/// that means the values of the characteristics defined by the effect that created it. Then all
/// applicable continuous effects are applied in a series of layers in the following order".
///
/// Within a layer, effects apply in timestamp order (see [CR 613.7]): "An effect with an earlier
/// timestamp is applied before an effect with a later timestamp." A characteristic-defining ability
/// defines power alone and sits in layer 7a with no other effect, so "apply effects from
/// characteristic-defining abilities first" (CR 613.3) holds by the layer order. An effect
/// that depends on another in its layer waits for it (see [CR 613.8], [`dependency`]).
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
/// In a game where a card grants a static ability with a continuous effect, the layers run
/// again while a run finds such abilities granted that it did not apply, at most three times
/// ([`granted`]).
///
/// [`Full`] if the list of effects had no place for one: the results are stored all the
/// same, lacking that effect, for a caller that ends the game.
///
/// [CR 613.1]: https://yawgatog.com/resources/magic-rules/#R6131
/// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
/// [CR 613.8]: https://yawgatog.com/resources/magic-rules/#R6138
fn recompute<L: Layout>(s: &mut State<L>, t: &GameTable, trials: &mut u64) -> Result<(), Full> {
    let Some(mut s) = s.rederive() else {
        return Ok(());
    };
    let mut granted: Granted<L> = None;
    let mut passes = 1;
    let (listed, applied) = loop {
        let (listed, applied) = apply_layers(&mut s, t, granted.as_ref(), trials);
        if passes == PASSES || !t.can(Capability::GrantsLayerEffect) {
            break (listed, applied);
        }
        match granted_layer_effects(&s, t) {
            Ok(found) if slice::<L>(found.as_ref()) != slice::<L>(granted.as_ref()) => {
                granted = found;
            }
            Ok(_) => break (listed, applied),
            Err(full) => break (listed.and(Err(full)), applied),
        }
        passes += 1;
    };
    let Applied {
        removed,
        land_types,
        protection,
    } = applied;
    let (listening, mut replacing) = if removed {
        listening(&s, t, s.rows())
    } else {
        listening_of_all(&s, t)
    };
    if protection {
        replacing |= Replaces::PROTECTION;
    }
    // A permanent that may change a rule for players said so in the mask just read for
    // the replacements: with none on the battlefield, one AND.
    let rules = if replacing.changes_player_rules() {
        player_rules(&s, t)
    } else {
        [Rules::NONE; _]
    };
    // What the static abilities read that no layer writes, so that a mutator of one of
    // them marks the results stale: in a game with no card that reads any, nothing. A
    // permanent that lost its abilities keeps its inputs, which costs a recompute too many
    // while it stays so and saves reading the results.
    let inputs = if t.can(Capability::LayerInputs) {
        (s.battlefield().iter()).fold(Inputs::NONE, |inputs, &slot| {
            let inputs = inputs.union(s.definition(slot).layer_inputs(t));
            merged::inputs(&s, t, slot, inputs)
        })
    } else {
        Inputs::NONE
    };
    s.finish(listening, replacing, rules, inputs, removed, land_types);
    listed
}

/// What kinds of effect one run of the layers applied, which the results stored with the
/// layer results are read from.
#[derive(Clone, Copy, Default)]
struct Applied {
    /// An effect that removes abilities.
    removed: bool,
    /// An effect that changes a land's land types, or a merged permanent with the abilities
    /// of cards under its top card: either makes a permanent's mana other than its card's
    /// ([`State::makes_card_mana`]).
    land_types: bool,
    /// An effect that gives protection ([`Replaces::PROTECTION`]).
    protection: bool,
}

/// One run of the layers over every permanent, from its copiable values to layer 7d, with
/// the static abilities of `granted` listed with those of the permanents' faces. Returns
/// [`Full`] if the list had no place for an effect, and what kinds of effect applied.
#[allow(
    clippy::disallowed_methods,
    reason = "the layers read a permanent's printed face up, by each ability's place on it"
)]
fn apply_layers<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    granted: Option<&<L as Layout>::Queue<GrantedTo<L>, 8>>,
    trials: &mut u64,
) -> (Result<(), Full>, Applied) {
    // Made at the first effect that applies: most recomputes have none, and the list is big.
    let mut list: Option<Applying<L>> = None;
    let mut listed = Ok(());
    let mut merged = false;
    let mut omitted = false;
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        // Layer 1: the copiable values (CR 613.2a). A copy's are those of what it copies,
        // with the exceptions it was made with (see CR 707.9b).
        let what = s.definition(slot);
        let f = what.face(t);
        // One test of the flag, read before the write; a copy's values are found out of line.
        let copy = s.object(slot).flags().has(Flag::Copy);
        s.results()[i] = Derived {
            power: f.power.unwrap_or(0),
            toughness: f.toughness.unwrap_or(0),
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
        if copy {
            let (omits_ability, is_merged, full) = copied::list(s, t, &mut list, i);
            omitted |= omits_ability;
            merged |= is_merged;
            listed = listed.and(full);
            continue;
        }
        for (ability, n) in f.abilities.iter().zip(0..) {
            let Some(spec) = ability.layer() else {
                continue;
            };
            let (source, face) = what.record(t);
            let on = Listed {
                at: i,
                order: n,
                source,
                face,
            };
            listed = listed.and(list_static(s, &mut list, on, ability, spec));
        }
    }
    for &GrantedTo { at, grant } in slice::<L>(granted) {
        let ability = granted_ability(t, grant);
        let Some(spec) = ability.layer() else {
            unreachable!("only granted abilities with a continuous effect are kept");
        };
        let on = Listed {
            at: usize::from(at),
            order: GRANTED | grant.grant.get(),
            source: grant.card,
            face: FaceIx::FRONT,
        };
        listed = listed.and(list_static(s, &mut list, on, ability, spec));
    }
    if t.can(Capability::DesignatedAbilities) {
        match crate::objects::designation::withhold(s, t) {
            Ok(missing) => omitted |= missing,
            Err(full) => listed = Err(full),
        }
    }
    // Layer 3 for the type lines, after every permanent's layer 1 values: nothing listed
    // above reads them (`apply::holds_from_base`).
    let text = if s.changes_text() {
        let (text, full) = text::type_lines(s, t);
        listed = listed.and(full);
        text
    } else {
        text::Changed::default()
    };
    listed = listed.and(resolved_effects(s, &mut list));
    let mut applied = Applied {
        removed: text.removed || omitted,
        land_types: text.land_types || merged,
        protection: false,
    };
    match &mut list {
        Some(list) => {
            list.as_mut_slice().sort_unstable_by_key(Apply::key);
            // The counters of layer 7c apply after its effects and before those of 7d.
            let split = (list.as_slice()).partition_point(|a| a.layer <= Layer::PtModifying);
            let (to_7c, after) = list.as_slice().split_at(split);
            let mut begun = None;
            let first = apply_in_order(s, t, to_7c, &mut begun, trials);
            apply::counters(s);
            let second = apply_in_order(s, t, after, &mut begun, trials);
            listed = listed.and(first).and(second);
            for a in list.iter() {
                applied.removed |= a.change.removes_abilities();
                applied.land_types |= matches!(
                    a.change,
                    LayerChange::SetLandTypes(_) | LayerChange::AddSubtype(_)
                );
                applied.protection |= matches!(a.change, LayerChange::AddProtection(_));
            }
        }
        None => apply::counters(s),
    }
    let applying = list.as_ref().map_or(&[][..], |l| l.as_slice());
    listed = listed.and(other_zones::reach(s, t, applying));
    (listed, applied)
}

/// Where a static ability being listed is: the battlefield position of its permanent, its
/// [`Apply::order`], and the card and face it is printed on.
#[derive(Clone, Copy)]
struct Listed<L: Layout> {
    at: usize,
    order: u8,
    source: L::Card,
    face: FaceIx,
}

/// The static ability `a`, a part of whose effect it is: an ability of its permanent's face,
/// one granted to the permanent ([`GRANTED`]), or one of a card under it ([`merged::MERGED`]).
#[allow(
    clippy::disallowed_methods,
    reason = "a granted ability is printed on its granting card's front face"
)]
fn static_ability_of<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    from: L::Index,
    a: &Apply<L>,
) -> &'static Ability {
    if a.order & GRANTED != 0 {
        &t.def(a.source).front().grants[usize::from(a.order & !GRANTED)]
    } else if a.order & merged::MERGED != 0 {
        merged::ability(s, t, s.battlefield()[position::<L>(from)], a)
    } else {
        &s.definition(s.battlefield()[position::<L>(from)])
            .face(t)
            .abilities[usize::from(a.order)]
    }
}

/// Lists the static ability `on` names, of the permanent at its battlefield position, whose
/// continuous effect is `spec`, among the effects to apply, if it applies now: one entry
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
    list: &mut Option<Applying<L>>,
    on: Listed<L>,
    ability: &'static Ability,
    spec: &'static LayerSpec,
) -> Result<(), Full> {
    let i = on.at;
    let slot = s.battlefield()[i];
    let o = s.object(slot);
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
            let target = if spec.affected == Objects::This {
                Some(i)
            } else {
                o.attached_to().and_then(|a| s.battlefield_position(a))
            };
            match (applies, target) {
                (true, Some(target)) => {
                    crate::coverage::hit(ability);
                    Target::Position(index::<L>(target))
                }
                _ => return Ok(()),
            }
        }
        Objects::This
        | Objects::Target(_)
        | Objects::AttachedTo
        | Objects::Those
        | Objects::TriggeredOn
        | Objects::TriggeredOnIn(_)
        | Objects::ExiledWithThis
        | Objects::LibraryTop { .. }
        | Objects::ExceptThose(_)
        | Objects::PutOntoBattlefieldWithThis
        | Objects::Carried
        | Objects::All { .. }
        | Objects::InZoneOf { .. }
        | Objects::CardsIn { .. }
        | Objects::Everywhere(_)
        | Objects::Paid(_)
        | Objects::Other(_) => Target::Static(index::<L>(i)),
    };
    let you = s.base_controller(slot);
    let words = text::words_for(s, slot, on.order);
    let parts = u8::try_from(spec.modifications.len())
        .expect("a layer spec has at most MODIFY_PARTS parts");
    let mut changes = [LayerChange::SwitchPt; MODIFY_PARTS];
    for (&modification, part) in spec.modifications.iter().zip(0..parts) {
        // No static ability of the cards changes control. One that did would name
        // its source's controller as layer 2 has it, not as the permanent entered.
        debug_assert_ne!(modification, Modification::Control);
        // "The chosen color" is the one chosen for this permanent. An amount read as its
        // layer applies is named by the ability that says it. The card table refuses a
        // granted ability with one (`Unapplied::GrantedAmount`).
        changes[usize::from(part)] =
            LayerChange::resolve(modification, you, &words, || s.chosen_color(slot))
                .unwrap_or_else(|| {
                    debug_assert_eq!(on.order & GRANTED, 0);
                    LayerChange::Read {
                        layer: read_layer(modification),
                        face: on.face,
                        ability: AbilityIx::new(on.order & !merged::MERGED),
                        part,
                    }
                });
    }
    let changes = &changes[..usize::from(parts)];
    // The part in the earliest layer, where the effect begins: the first said of those.
    let first = (0..changes.len())
        .min_by_key(|&part| (changes[part].layer(), part))
        .unwrap_or(0);
    let mut listed = Ok(());
    for (&change, part) in changes.iter().zip(0..parts) {
        let pushed = list.get_or_insert_with(Default::default).push(Apply {
            layer: change.layer(),
            timestamp: o.timestamp(),
            order: on.order,
            part,
            target,
            source: on.source,
            change,
            from: Some((index::<L>(i), usize::from(part) == first)),
            parts,
        });
        listed = listed.and(pushed);
    }
    listed
}

/// Applies `list`, effects sorted by layer and timestamp: all of a layer's before any of the
/// next one's, so each layer applies to what the layers before it made of every permanent,
/// and an effect that finds what it applies to as it applies sees those results.
///
/// Within a layer, effects apply in timestamp order unless one depends on another
/// ([`dependency`]). A static ability begins in its first layer if its permanent still has
/// it as the ability comes up ([`Begun`]). One whose effect has parts in later layers
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
    list: &[Apply<L>],
    begun: &mut Option<L::Queue<Begun<L>, 32>>,
    trials: &mut u64,
) -> Result<(), Full> {
    let mut applied = Ok(());
    let mut head = 0;
    while head < list.len() {
        let layer = list[head].layer;
        let end = head + list[head..].partition_point(|a| a.layer == layer);
        let segment = &list[head..end];
        if segment.len() > 1 && dependency::may_depend(s, t, segment) {
            applied = applied.and(dependency::apply_ordered(s, t, segment, begun, trials));
        } else {
            for a in segment {
                applied = applied.and(apply_one(s, t, a, begun));
            }
        }
        head = end;
    }
    applied
}

/// Applies one effect of [`apply_in_order`]: finds what it applies to now
/// ([`members`]) and changes each. The first part of a static ability with more parts
/// records the set it began on, or that it did not begin, for the later parts.
fn apply_one<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    a: &Apply<L>,
    begun: &mut Option<L::Queue<Begun<L>, 32>>,
) -> Result<(), Full> {
    let members = members(s, t, a, begun.as_ref());
    let mut applied = Ok(());
    if let Some((from, true)) = a.from
        && a.parts > 1
    {
        let carried = Begun {
            from: from.into(),
            order: a.order,
            members,
        };
        applied = begun.get_or_insert_with(Default::default).push(carried);
    }
    let Some(members) = members else {
        return applied;
    };
    let by = reader(s, a);
    applied.and(apply::apply_to_members(
        s, t, members, a.source, a.change, by,
    ))
}

/// The permanents `a` applies to now, or none if it does not apply. A static ability's
/// first part begins only if its permanent still has the ability ([CR 613.6]): "If an effect
/// starts to apply in one layer and/or sublayer, it will continue to be applied to the same
/// set of objects in each other applicable layer and/or sublayer, even if the ability
/// generating the effect is removed during this process." A later part applies to the set
/// the first found, which `begun` holds.
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
fn members<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    a: &Apply<L>,
    begun: Option<&L::Queue<Begun<L>, 32>>,
) -> Option<Bitboard<L::Slot>> {
    match (a.from, a.target) {
        (None, Target::Position(i)) => Some(Bitboard::single(s.battlefield()[position::<L>(i)])),
        (None, Target::Set(set)) => Some(s.locked_in(set)),
        (Some((from, true)), target) => {
            if !has_so_far(s, from, a) {
                return None;
            }
            Some(match target {
                Target::Position(i) => Bitboard::single(s.battlefield()[position::<L>(i)]),
                Target::Static(i) => {
                    let words = reader(s, a).map_or(Words::NONE, |by| by.words);
                    apply::static_members(s, t, i.into(), static_ability_of(s, t, from, a), words)
                }
                Target::Set(_) => unreachable!("a static ability locks in no set"),
            })
        }
        (Some((from, false)), _) => {
            let from: u16 = from.into();
            begun?
                .iter()
                .find(|b| (b.from, b.order) == (from, a.order))
                .and_then(|b| b.members)
        }
        (None, Target::Static(_)) => unreachable!("a resolved effect names no ability"),
    }
}

/// Whether the permanent at `from` still has the static ability `a` is a part of as its
/// effect comes up. An ability of its face, if no effect so far removed it. A granted one was
/// found granted by the run of the layers before ([`Granted`]), and in that run's layers 1
/// to 5 it is present, as no layer 6 effect has yet applied to take it away; from layer 6 on,
/// if the permanent has been granted it again in this run.
fn has_so_far<L: Layout>(s: &Rederive<'_, L>, from: L::Index, a: &Apply<L>) -> bool {
    let at = position::<L>(from);
    if a.order & merged::MERGED != 0 {
        return merged::has_so_far(s, at);
    }
    if a.order & GRANTED == 0 {
        return s.has_ability_so_far(at, AbilityIx::new(a.order));
    }
    a.layer < Layer::Ability || { s.rows()[at].granted }.contains(&Some(GrantRef {
        card: a.source,
        grant: GrantIx::new(a.order & !GRANTED),
    }))
}

/// A battlefield position the layout's index names, as a `usize`.
#[inline]
fn position<L: Layout>(i: L::Index) -> usize {
    i.into()
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
            // (see [CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137)); the members
            // are read as it applies.
            Permanents::Set(set) => Target::Set(set),
        };
        let pushed = list.get_or_insert_with(Default::default).push(Apply {
            layer: change.layer(),
            timestamp: e.timestamp,
            order: 0,
            part: 0,
            target,
            source: e.source,
            change,
            from: None,
            parts: 1,
        });
        listed = listed.and(pushed);
    }
    listed
}
