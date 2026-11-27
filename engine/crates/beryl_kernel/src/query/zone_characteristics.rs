//! Characteristics off the battlefield: the continuous effects that reach objects in every
//! zone, and the reads that apply them.
//!
//! Implements: none. Applies the layer changes selected for zones outside the battlefield.
//!
//! Layer results exist only for permanents ([`crate::state::Derived`], one row for each
//! battlefield position). Anywhere else an object's characteristics are computed when they
//! are read: its copiable values, then the continuous effects that reach other zones, in the
//! order they apply (see [CR 613.1]): "The values of an object's characteristics are determined
//! by starting with the actual object. ... Then all applicable continuous effects are
//! applied in a series of layers in the following order". Such effects are rare, so the
//! recompute of the layers lists them, at most [`ELSEWHERE`], beside the layer results, and a
//! game with none pays one test of the list's length for each read.
//!
//! An effect of this kind is a static ability of a permanent whose affected set is
//! [`beryl_card::Objects::Everywhere`]. The card table admits only those whose parts change
//! card types, subtypes and colors (layers 4 and 5, `beryl_card`'s `Malformed::Everywhere`),
//! which this module applies, or grant escape to cards in a graveyard, which the casting
//! rules ask of a card there ([`State::escape_from`]).
//!
//! [CR 613.1]: https://yawgatog.com/resources/magic-rules/#R6131

use beryl_card::{Colors, Filter, Objects, Supertypes, Types};

use crate::chosen::ChosenValue;

use crate::effects::LayerChange;
use crate::layout::{Layout, Queue};
use crate::reference::{CardIn, Graveyard};
use crate::state::State;
use crate::subtypes::GameSubtypes;
use crate::table::GameTable;

/// How many effects that reach other zones the list holds in a fixed layout. A fifth is
/// [`crate::fixed::Full`], and the game stalls into a layout whose list grows.
pub const ELSEWHERE: usize = 4;

/// One part of a continuous effect that reaches objects off the battlefield: the part at
/// `part` of the static ability at `ability` of the face of the permanent in `source`. The
/// change it makes is read from the face when it applies ([`State::reach_change`]), so each
/// entry is three bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Reach<L: Layout> {
    /// The permanent with the static ability, whose controller reads its filter
    /// (see [CR 109.5]): "For a static ability, this is the current controller of the object
    /// it's on."
    ///
    /// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
    pub source: L::Slot,
    /// The ability's index on the face of that permanent.
    pub ability: u8,
    /// The part's index among the ability's modifications.
    pub part: u8,
}

impl<L: Layout> crate::relayout::Relayout for Reach<L> {
    type In<M: Layout> = Reach<M>;

    fn relayout<M: Layout>(&self) -> Reach<M> {
        Reach {
            source: crate::relayout::slot(self.source),
            ability: self.ability,
            part: self.part,
        }
    }
}

/// The list of effects that reach other zones, in the order they apply.
pub(crate) type Reaching<L> = <L as Layout>::Queue<Reach<L>, ELSEWHERE>;

/// The characteristics of an object off the battlefield that continuous effects there may
/// change, as far as the effects applied so far have changed them.
#[derive(Clone, Copy, Debug)]
pub struct SoFar {
    pub types: Types,
    pub supertypes: Supertypes,
    pub subtypes: GameSubtypes,
    pub colors: Colors,
}

impl<L: Layout> State<L> {
    /// Whether some continuous effect reaches objects off the battlefield, as the layers last
    /// applied: without one, an object there has its copiable values, and the reads of
    /// [`crate::query`] take them with one test of this.
    #[inline]
    pub fn reaches_elsewhere(&self) -> bool {
        !self.elsewhere.is_empty()
    }

    /// The effects that reach objects off the battlefield, in the order they apply.
    #[inline]
    pub fn reaching(&self) -> &[Reach<L>] {
        self.elsewhere.as_slice()
    }

    /// The characteristics of the object in `slot`, which is not a permanent, after the
    /// effects that reach other zones: its copiable values, then each effect whose filter
    /// accepts it as the effects before left it, in the order they apply (see [CR 613.7]): "An
    /// effect with an earlier timestamp is applied before an effect with a later timestamp."
    ///
    /// Out of line: a game comes here only while a card such as one that colors every card
    /// is on the battlefield.
    ///
    /// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
    #[cold]
    #[inline(never)]
    pub(crate) fn elsewhere(&self, t: &GameTable, slot: L::Slot) -> SoFar {
        let what = self.copiable(slot);
        let mut so_far = SoFar {
            types: what.types(t),
            supertypes: what.supertypes(t),
            subtypes: what.subtypes(t),
            colors: what.colors(t),
        };
        for &reach in self.elsewhere.iter() {
            let (filter, change) = self.reach_change(t, reach);
            // Escape is an ability, read by `escapes`: no characteristic here.
            if change == LayerChange::GrantEscape {
                continue;
            }
            let you = self.controller(reach.source);
            // The filter is the source's text, read through the source's words.
            let words = self.words_of(reach.source);
            if crate::filter::passes_so_far(&filter, self, t, slot, you, &words, &so_far) {
                apply(t, &mut so_far, change);
            }
        }
        so_far
    }
}

impl<L: Layout> State<L> {
    /// The first effect that gives this graveyard card escape (see [CR 702.138a]):
    /// "Escape represents a static ability that functions while the card with escape is in
    /// a player's graveyard." The filter sees the card after any changes to its types
    /// or colors, since escape is granted in layer 6.
    ///
    /// [CR 702.138a]: https://yawgatog.com/resources/magic-rules/#R702138a
    #[cold]
    #[inline(never)]
    pub fn escape_from(&self, t: &GameTable, card: CardIn<Graveyard, L>) -> Option<Reach<L>> {
        let slot = card.slot();
        let mut so_far = None;
        self.elsewhere.iter().copied().find(|&reach| {
            let (filter, change) = self.reach_change(t, reach);
            if change != LayerChange::GrantEscape {
                return false;
            }
            let so_far = *so_far.get_or_insert_with(|| self.elsewhere(t, slot));
            let you = self.controller(reach.source);
            let words = self.words_of(reach.source);
            crate::filter::passes_so_far(&filter, self, t, slot, you, &words, &so_far)
        })
    }

    /// The filter of the effect `reach` names and the change its part makes: the part as the
    /// layers resolve it for the source's controller, the chosen color read from the source.
    fn reach_change(&self, t: &GameTable, reach: Reach<L>) -> (Filter, LayerChange) {
        let ability = &self.definition(reach.source).face(t).abilities[usize::from(reach.ability)];
        let Some(spec) = ability.layer() else {
            unreachable!("the layers list only static abilities with a continuous effect");
        };
        let Objects::Everywhere(filter) = spec.affected else {
            unreachable!("the layers list only abilities that reach every zone");
        };
        let modification = spec.modifications[usize::from(reach.part)];
        let you = self.controller(reach.source);
        let words = self.words_of(reach.source);
        let change = LayerChange::resolve(modification, you, &words, || {
            self.chosen_color(reach.source)
        })
        .expect("the card table admits only changes of types and colors, and escape, here");
        (filter, change)
    }

    /// The color chosen as the permanent in `slot` entered (see [CR 105.4]): "If a player is
    /// asked to choose a color, they must choose one of the five colors." None if no color
    /// was chosen for it, as for a permanent that entered without the replacement that asks.
    ///
    /// [CR 105.4]: https://yawgatog.com/resources/magic-rules/#R1054
    #[inline]
    pub fn chosen_color(&self, slot: L::Slot) -> Colors {
        match self.chosen(slot) {
            Some(ChosenValue::Color(colors)) => colors,
            _ => Colors::NONE,
        }
    }
}

/// Applies `change`, a change of layer 4 or 5, to characteristics off the battlefield, as the
/// layers apply it to a permanent.
fn apply(t: &GameTable, so_far: &mut SoFar, change: LayerChange) {
    match change {
        // see CR 613.1d: "Layer 4: Type-changing effects are applied. These include effects that
        // change an object's card type, subtype, and/or supertype."
        LayerChange::AddTypes(types) => so_far.types = so_far.types.union(types),
        // see CR 205.1a: "If an object's card type is removed, the subtypes correlated with that
        // card type will remain if they are also the subtypes of a card type the object
        // currently has; otherwise, they are also removed".
        LayerChange::RemoveTypes(types) => {
            so_far.types = so_far.types.minus(types);
            so_far.subtypes = so_far.subtypes.of_types(t.subtypes(), so_far.types);
        }
        LayerChange::SetTypes(types) => {
            so_far.types = types;
            so_far.subtypes = so_far.subtypes.of_types(t.subtypes(), types);
        }
        LayerChange::AddSubtype(subtype) => {
            so_far.subtypes = so_far.subtypes.with(t.subtypes(), subtype);
        }
        // see CR 205.1a: "when an effect sets one or more of an object's subtypes, the new
        // subtype(s) replaces any existing subtypes from the appropriate set".
        LayerChange::SetSubtype(subtype) => {
            let kept = (so_far.subtypes).without_types(t.subtypes(), subtype.card_type());
            so_far.subtypes = kept.with(t.subtypes(), subtype);
        }
        // see CR 613.1e: "Layer 5: Color-changing effects are applied."
        LayerChange::SetColors(colors) => so_far.colors = colors,
        LayerChange::AddColors(colors) => so_far.colors = so_far.colors.union(colors),
        LayerChange::Control(_)
        | LayerChange::SetLandTypes(_)
        | LayerChange::AddKeywords(_)
        | LayerChange::Grant(_)
        | LayerChange::LoseAllAbilities
        | LayerChange::LoseAbility(_)
        | LayerChange::DefinedPower(_)
        | LayerChange::SetBasePt(..)
        | LayerChange::ModifyPt(..)
        | LayerChange::SwitchPt
        | LayerChange::Read { .. }
        | LayerChange::DefinedPt(..)
        | LayerChange::AddProtection(_)
        | LayerChange::GrantEscape
        | LayerChange::Lend { .. } => {
            unreachable!("the card table admits only changes of types and colors here")
        }
    }
}

/// Whether the effects that reach other zones could hold `change`: a change of card types,
/// subtypes or colors, which [`State::elsewhere`] applies, or escape, which
/// [`State::escape_from`] reads. A land type's mana abilities (see CR 305.7) are none of these.
pub fn reaches(change: LayerChange) -> bool {
    matches!(
        change,
        LayerChange::AddTypes(_)
            | LayerChange::RemoveTypes(_)
            | LayerChange::SetTypes(_)
            | LayerChange::AddSubtype(_)
            | LayerChange::SetSubtype(_)
            | LayerChange::SetColors(_)
            | LayerChange::AddColors(_)
            | LayerChange::GrantEscape
    )
}
