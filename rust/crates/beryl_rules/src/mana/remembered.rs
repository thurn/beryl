//! Mana that remembers ([CR 106.6]): "Some spells or abilities that produce mana restrict how
//! that mana can be spent, have an additional effect that affects the spell or ability that mana
//! is spent on, or create a delayed triggered ability (see rule 603.7a) that triggers when that
//! mana is spent. This doesn't affect the mana's type."
//!
//! A source whose mana ability makes restricted mana is a class of its own in a payment its
//! restriction accepts, and in no other ([`super::sources::Sources::of`]). Mana that remembers
//! in a pool pays only what its restriction accepts. Spent on a spell, its rider becomes a rule
//! change on that spell, which `beryl_kernel`'s [`State::effect_rules_on`] reads. Everything
//! here is read only in a game whose cards make such mana ([`Capability::RemembersMana`]).
//!
//! [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066

use beryl_card::{AbilityIx, Behavior, Effect, ManaRestriction, Players};

use crate::card::{Face, FaceIx, ManaColor};
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::rule_change;
use beryl_kernel::chosen::ChosenValue;
use beryl_kernel::mana_memory::{RememberedMana, RestrictionId};

use crate::state::{
    Continuous, Derived, Duration, ObjectRef, StackEntry, StackItem, State, Subject,
};
use crate::table::{Capability, GameTable, is_tap_for_mana};

use super::sources::{allowed, can_tap};

/// What a payment is for, as mana whose restriction reads it sees it: the spell it casts, if
/// it casts one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Paying<L: Layout = Small> {
    /// The spell's card and the face it is cast as.
    spell: Option<(L::Card, FaceIx)>,
    /// The spell on the stack, which a rider changes: none while the rules check whether a
    /// card could be cast at all.
    object: Option<ObjectRef<L>>,
}

impl<L: Layout> Paying<L> {
    /// A payment for no spell: an ability's cost, or one an effect asks for. Restricted mana
    /// pays none of it.
    pub(crate) const NOTHING: Paying<L> = Paying {
        spell: None,
        object: None,
    };

    /// A payment for casting `face` of `card`, which is not yet on the stack.
    pub(crate) fn card(card: L::Card, face: FaceIx) -> Paying<L> {
        Paying {
            spell: Some((card, face)),
            object: None,
        }
    }

    /// A payment for casting the card in `slot` as its face `face`, not yet on the stack: in a
    /// game whose cards make no restricted mana, one for nothing, which reads nothing.
    #[inline]
    pub(crate) fn casting(s: &State<L>, t: &GameTable, slot: L::Slot, face: FaceIx) -> Paying<L> {
        if !t.can(Capability::RemembersMana) {
            return Paying::NOTHING;
        }
        let (card, face) = s.copiable(slot).turned(face).record(t);
        Paying::card(card, face)
    }

    /// A payment for the spell or ability `entry` being cast: in a game whose cards make no
    /// restricted mana, one for nothing, which reads nothing.
    #[inline]
    pub(crate) fn entry(t: &GameTable, entry: &StackEntry<L>) -> Paying<L> {
        if !t.can(Capability::RemembersMana) {
            return Paying::NOTHING;
        }
        match entry.item {
            StackItem::Spell { card, face, .. } => Paying {
                spell: Some((card, face)),
                object: entry.spell(),
            },
            StackItem::Activated { .. } | StackItem::Triggered { .. } => Paying::NOTHING,
        }
    }

    /// Whether restricted mana may pay any of it: a spell, in a game whose cards make such
    /// mana. One AND in any other game.
    #[inline]
    pub(crate) fn reads_restrictions(&self, t: &GameTable) -> bool {
        self.spell.is_some() && t.can(Capability::RemembersMana)
    }

    /// Whether mana under the restriction the game numbered `restriction` may be spent on it,
    /// whose source had chosen the creature type `chosen_type`
    /// ([`RememberedMana::chosen_type`]).
    pub(crate) fn accepts(
        &self,
        t: &GameTable,
        restriction: RestrictionId,
        chosen_type: u8,
    ) -> bool {
        let restriction: &ManaRestriction = t.restriction(restriction);
        let Some((card, face)) = self.spell else {
            return false;
        };
        let spell: &Face = t.def(card).face(face);
        rule_change::accepts(restriction.cast, spell)
            && (!restriction.of_chosen_type
                || t.face_subtypes(card, face).has_creature_type(chosen_type))
    }
}

/// A permanent's mana ability whose mana is restricted, which the solver taps only for a
/// payment its restriction accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Restricted<L: Layout = Small> {
    pub(super) slot: L::Slot,
    /// The kinds of mana it makes, a bit per [`ManaColor`].
    pub(super) kinds: u8,
    /// Its restriction, by the game's number.
    pub(super) restriction: RestrictionId,
    /// The creature type the permanent chose as it entered, by the game's number.
    pub(super) chosen_type: u8,
}

/// The creature type the permanent in `slot` chose as it entered, by the game's number, as a
/// restriction to "the chosen type" reads it.
fn chosen_type<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    match s.chosen(slot) {
        Some(ChosenValue::CreatureType(Some(n))) => n,
        _ => RememberedMana::<L>::NO_TYPE,
    }
}

/// The restricted mana ability of the permanent in `slot`, whose derived characteristics are
/// `d`, if `player` may tap it for mana now, with its place on the face.
fn restricted_ability<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    d: &Derived<L>,
) -> Option<(AbilityIx, &'static beryl_card::Ability)> {
    if !can_tap(s, player, slot, d) {
        return None;
    }
    let abilities = s.definition(slot).face(t).abilities;
    (0u8..).zip(abilities).find_map(|(i, a)| {
        let index = AbilityIx::new(i);
        (is_tap_for_mana(a)
            && a.spend().is_some()
            && s.has_ability(slot, index)
            && allowed(s, t, a, player, &s.words_of(slot)))
        .then_some((index, a))
    })
}

/// The sources of restricted mana a [`Small`] payment considers. One past it is [`Full`].
pub(crate) const RESTRICTED: usize = 4;

/// The permanents `player` controls with a restricted mana ability they may tap now, whatever
/// it would pay for: none in a game whose cards make no restricted mana.
pub(crate) fn restricted_sources<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Result<L::Queue<Restricted<L>, RESTRICTED>, Full> {
    let mut out: L::Queue<Restricted<L>, RESTRICTED> = Default::default();
    if !t.can(Capability::RemembersMana) {
        return Ok(out);
    }
    for (slot, d) in super::sources::controlled(s, player) {
        if let Some((_, a)) = restricted_ability(s, t, player, slot, d) {
            debug_assert_eq!(
                a.produces().amount,
                1,
                "a restricted source makes one mana: more would float as mana that remembers"
            );
            out.push(Restricted {
                slot,
                kinds: a.produces().colors,
                restriction: (a.spend())
                    .and_then(|r| t.restriction_id(r))
                    .expect("numbered"),
                chosen_type: chosen_type(s, slot),
            })?;
        }
    }
    Ok(out)
}

/// The mana that remembers in `player`'s pool that `paying` accepts: each entry's place in
/// the pool and its kind, as a [`ManaColor`] index.
pub(crate) fn accepted_entries<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    paying: Paying<L>,
) -> L::Queue<(u8, u8), 4> {
    let mut out: L::Queue<(u8, u8), 4> = Default::default();
    if !paying.reads_restrictions(t) {
        return out;
    }
    let entries = s.player(player).remembered_mana().iter();
    for (i, mana) in (0u8..).zip(entries) {
        if paying.accepts(t, mana.restriction, mana.chosen_type)
            && !out.push_if_room((
                i,
                u8::try_from(mana.color.index()).expect("one of six mana kinds"),
            ))
        {
            break;
        }
    }
    out
}

/// Whether the restricted mana ability of the permanent in `slot` makes `color`, so that
/// tapping it for that mana uses it: for a source whose unrestricted abilities make none.
pub(super) fn makes<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    color: ManaColor,
) -> Option<(AbilityIx, &'static beryl_card::Ability)> {
    if !t.can(Capability::RemembersMana) {
        return None;
    }
    let player = s.controller(slot);
    restricted_ability(s, t, player, slot, s.derived(slot))
        .filter(|(_, a)| a.produces().includes(color))
}

/// Adds the mana a restricted mana ability of the permanent `source` makes, of the kind
/// `color`, as mana that remembers its restriction and what the restriction reads of the
/// source now (CR 106.6), its costs paid. The ability's whole effect is to add the mana.
pub(super) fn add<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    source: ObjectRef<L>,
    ability: &'static beryl_card::Ability,
    color: ManaColor,
) -> Result<(), Full> {
    crate::coverage::hit(ability);
    debug_assert!(
        matches!(
            ability.body(),
            Behavior::Data(&Effect::AddMana {
                player: Players::You,
                ..
            })
        ),
        "a restricted mana ability only adds mana"
    );
    let restriction = ability.spend().expect("a restricted mana ability");
    let mana = RememberedMana {
        color,
        restriction: t.restriction_id(restriction).expect("numbered"),
        chosen_type: chosen_type(s, source.slot),
        source: s.definition(source.slot).record(t).0,
    };
    let player = s.controller(source.slot);
    let added = s.add_remembered_mana(player, Some(source), mana)?;
    sink.note_from(Some(source), added);
    crate::layers::refresh(s, t, sink)
}

/// Spends the mana that remembers at `index` in `player`'s pool on what `paying` is for. Its
/// rider, if it has one, becomes a rule change on the spell, for as long as the spell stays
/// on the stack (CR 106.6), whose card is the source of the mana.
pub(super) fn spend<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    player: Seat,
    index: usize,
    paying: Paying<L>,
) -> Result<ManaColor, Full> {
    let mana = s.spend_remembered_mana(player, index);
    debug_assert!(
        paying.accepts(t, mana.restriction, mana.chosen_type),
        "mana spent on what its restriction refuses"
    );
    if let (Some(rider), Some(spell)) = (t.restriction(mana.restriction).rider, paying.object) {
        let id = t.rule_id(rider).expect("a rider the game numbered");
        let change = Continuous::Rule(Subject::Object(spell), id);
        s.start_effect(mana.source, change, Duration::Indefinite)?;
    }
    Ok(mana.color)
}
