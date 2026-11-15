//! Mana that remembers: mana a player's pool holds with a restriction on what it may be spent
//! on and a rider it gives the spell it pays for ([CR 106.6]): "Some spells or abilities that
//! produce mana restrict how that mana can be spent, have an additional effect that affects the
//! spell or ability that mana is spent on, or create a delayed triggered ability (see rule
//! 603.7a) that triggers when that mana is spent. This doesn't affect the mana's type."
//!
//! A card says the restriction as data (`beryl_card`'s [`ManaRestriction`]), and a game numbers
//! those its cards name ([`RestrictionId`]) as its table is built, as it numbers rule changes. A
//! pool entry ([`RememberedMana`]) keeps the restriction's number, the mana's kind and what the
//! restriction reads of the mana's source, captured as the mana is made: the source may be gone
//! by the time the mana is spent. A game whose cards make no such mana
//! ([`crate::table::Capability::RemembersMana`]) never has an entry, and its pools are six
//! counts.
//!
//! [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066

use beryl_card::{Ability, CardDef, ManaColor, ManaRestriction};

use crate::chosen::ANOTHER;
use crate::event::Event;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::object::ObjectRef;
use crate::state::State;
use crate::table::GameTable;

/// The entries of mana that remembers one player's pool holds in [`Small`]. A fifth is
/// [`Full`], which stalls the game into [`crate::Large`], whose pool grows.
pub const REMEMBERED_MANA: usize = 4;

/// The number of a mana restriction in a game: its place among those the game's cards name, in
/// order of first appearance ([`crate::GameTable::restriction`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RestrictionId(u8);

impl RestrictionId {
    #[inline]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// One mana a pool holds that remembers what it may be spent on (CR 106.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RememberedMana<L: Layout = Small> {
    /// Its kind: the restriction doesn't change it (CR 106.6).
    pub color: ManaColor,
    /// What it may be spent on, and what it does to the spell it pays for.
    pub restriction: RestrictionId,
    /// The creature type its source had chosen as it made the mana, by the game's number of
    /// it ([`crate::chosen::ChosenValue::number`]), or [`ANOTHER`] for a type no card of the
    /// game names or a source that had chosen none: read only by a restriction to spells of
    /// the chosen type.
    pub chosen_type: u8,
    /// The card of the permanent that made it: the source of the rider's effect.
    pub source: L::Card,
}

/// The value a fixed-capacity queue fills its unused room with, never read.
impl<L: Layout> Default for RememberedMana<L> {
    fn default() -> Self {
        RememberedMana {
            color: ManaColor::Colorless,
            restriction: RestrictionId::default(),
            chosen_type: ANOTHER,
            source: L::Card::default(),
        }
    }
}

impl<L: Layout> RememberedMana<L> {
    /// The value a restriction to "the chosen type" reads when the source chose no creature
    /// type: one no spell has but by having every creature type.
    pub const NO_TYPE: u8 = ANOTHER;
}

/// The mana restrictions the cards of one game name, numbered in order of first appearance. A
/// restriction is the one written at its address, as a rule change is.
pub(crate) fn number(defs: &[&'static CardDef]) -> Vec<&'static ManaRestriction> {
    let mut list: Vec<&'static ManaRestriction> = Vec::new();
    let named = defs
        .iter()
        .flat_map(|def| def.faces)
        .flat_map(|face| face.abilities.iter().chain(face.grants))
        .filter_map(Ability::spend);
    for restriction in named {
        if !list.iter().any(|&r| std::ptr::eq(r, restriction)) {
            list.push(restriction);
        }
    }
    assert!(
        u8::try_from(list.len()).is_ok(),
        "a game numbers its mana restrictions in a byte"
    );
    list
}

impl GameTable {
    /// The mana restriction the game numbered `id`.
    #[inline]
    pub fn restriction(&self, id: RestrictionId) -> &'static ManaRestriction {
        self.restrictions[id.index()]
    }

    /// The number of `restriction` in the game, if a card of it names it.
    #[inline]
    pub fn restriction_id(&self, restriction: &'static ManaRestriction) -> Option<RestrictionId> {
        let at = (self.restrictions.iter()).position(|&r| std::ptr::eq(r, restriction))?;
        Some(RestrictionId(u8::try_from(at).expect("numbered in a byte")))
    }
}

impl<L: Layout> State<L> {
    /// Adds one mana that remembers to a player's mana pool, made by `source` (CR 106.4, CR
    /// 106.6): "When an effect instructs a player to add mana, that mana goes into a player's
    /// mana pool."
    ///
    /// # Errors
    ///
    /// [`Full`] when the pool holds as many entries as [`Small`] keeps.
    #[inline]
    pub fn add_remembered_mana(
        &mut self,
        player: Seat,
        source: Option<ObjectRef<L>>,
        mana: RememberedMana<L>,
    ) -> Result<Event<L>, Full> {
        self.player_mut(player).remembered.push(mana)?;
        Ok(Event::Mana {
            player,
            source,
            color: mana.color,
            amount: 1,
            restricted: true,
        })
    }

    /// Spends the mana that remembers at `index` in a player's pool (CR 106.4), returning it:
    /// the entries after it move up one.
    #[inline]
    pub fn spend_remembered_mana(&mut self, player: Seat, index: usize) -> RememberedMana<L> {
        self.player_mut(player).remembered.remove(index)
    }
}
