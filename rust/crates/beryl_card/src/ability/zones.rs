//! Where an ability works ([CR 113.6]): "Abilities of an instant or sorcery spell usually function
//! only while that object is on the stack. Abilities of all other objects usually function only
//! while that object is on the battlefield."
//!
//! [CR 113.6]: https://yawgatog.com/resources/magic-rules/#R1136

use super::{Ability, AbilityKind};
use crate::ids::ZoneSet;

/// Where an ability of `kind` works unless it says otherwise ([CR 113.6]): "Abilities of an
/// instant or sorcery spell usually function only while that object is on the stack. Abilities of
/// all other objects usually function only while that object is on the battlefield."
///
/// [CR 113.6]: https://yawgatog.com/resources/magic-rules/#R1136
pub(super) const fn usual_zones(kind: AbilityKind) -> ZoneSet {
    match kind {
        AbilityKind::Spell => ZoneSet::STACK,
        _ => ZoneSet::BATTLEFIELD,
    }
}

/// The builders and readers of where an ability works.
impl Ability {
    /// It works only in a graveyard ([CR 113.6b]): "An ability that states which zones it
    /// functions in functions only from those zones." As "When this card is put into your
    /// graveyard from anywhere" or "Whenever you draw a card, if this card is in your
    /// graveyard" say.
    ///
    /// [CR 113.6b]: https://yawgatog.com/resources/magic-rules/#R1136b
    #[must_use]
    pub const fn in_graveyard(self) -> Ability {
        Ability {
            zones: ZoneSet::GRAVEYARD,
            ..self
        }
    }

    /// It works only on the stack, as a spell's "When you cast this spell" does
    /// ([CR 113.6k]): "A trigger condition that can't trigger from the battlefield functions in
    /// all zones it can trigger from."
    ///
    /// [CR 113.6k]: https://yawgatog.com/resources/magic-rules/#R1136k
    #[must_use]
    pub const fn on_the_stack(self) -> Ability {
        Ability {
            zones: ZoneSet::STACK,
            ..self
        }
    }

    /// It works in every zone, as a characteristic-defining ability does ([CR 113.6a]):
    /// "Characteristic-defining abilities function everywhere, even outside the game and
    /// before the game begins."
    ///
    /// [CR 113.6a]: https://yawgatog.com/resources/magic-rules/#R1136a
    #[must_use]
    pub const fn everywhere(self) -> Ability {
        Ability {
            zones: ZoneSet::ALL,
            ..self
        }
    }

    /// The zones it works in (CR 113.6).
    #[inline]
    pub const fn zones(&self) -> ZoneSet {
        self.zones
    }

    /// Whether it works in the zones its kind does and no others (CR 113.6): a spell's on
    /// the stack, any other on the battlefield.
    pub const fn works_where_its_kind_does(&self) -> bool {
        let usual = usual_zones(self.kind);
        self.zones.includes(usual) && usual.includes(self.zones)
    }
}
