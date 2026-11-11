//! Permissions to cast: the queue by-w8gv.7.4 replaces with entries of the Effects store
//! ([`crate::effects`]), so it gets no typed mutator (the crate's contract, "The legacy
//! accessor").

use crate::ids::Seat;
use crate::layout::{Layout, Small};
use crate::object::ObjectRef;

/// A permission to cast a card from a zone other than a hand
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin to
/// cast a spell only if a rule or effect allows that player to cast it". It names one
/// incarnation of the card, so it ends when the card leaves that zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CastPermission<L: Layout = Small> {
    pub object: ObjectRef<L>,
    pub player: Seat,
    /// Generic mana added to the cost, as in "A spell cast this way costs {2} more to cast."
    /// (Elite Spellbinder)
    pub extra: u8,
    /// The permission lasts until end of turn rather than while the card stays in its zone.
    pub this_turn: bool,
    /// "If that spell would be put into your graveyard, exile it instead." (Jace, Telepath
    /// Unbound)
    pub exile: bool,
    /// The card has flashback, its cost the card's mana cost (Snapcaster Mage)
    /// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "'Flashback
    /// [cost]' means 'You may cast this card from your graveyard if the resulting spell is an
    /// instant or sorcery spell by paying [cost] rather than paying its mana cost'". Paying it
    /// is an alternative cost, so the card's own can't be added.
    pub flashback: bool,
}
