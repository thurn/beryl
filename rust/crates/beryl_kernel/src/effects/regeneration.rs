//! Regeneration shields in the Effects store ([CR 701.19a]): "If the effect of a resolving
//! spell or ability regenerates a permanent, it creates a replacement effect that protects
//! the permanent the next time it would be destroyed this turn."
//!
//! Each shield is one entry on the permanent with the payload [`Payload::Regeneration`],
//! lasting until end of turn. A permanent regenerated twice has two shields, and each
//! destruction uses up one of them. A shield from a static ability ([CR 701.19b]) replaces
//! every destruction and is used up by none; it would be a rule of the permanent's copiable
//! values rather than an entry here.
//!
//! [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
//! [CR 701.19b]: https://yawgatog.com/resources/magic-rules/#R70119b

use std::cell::Cell;

use crate::layout::Layout;
use crate::object::ObjectRef;
use crate::state::State;

use super::{Affects, ContinuousEffect, Kinds, Payload};

impl<L: Layout> State<L> {
    /// Whether a regeneration shield is on `object`. With no shield on any object, one AND,
    /// which is all that a destruction in a game with no shield pays.
    #[inline]
    pub fn is_shielded(&self, object: ObjectRef<L>) -> bool {
        let affected = Affects::object(object);
        (self.effects.of(Kinds::REGENERATION).iter())
            .any(|e| e.payload == Payload::Regeneration && e.affected == affected)
    }

    /// Whether a regeneration shield is on any permanent: one AND.
    #[inline]
    pub fn has_regeneration_shields(&self) -> bool {
        !self.effects.of(Kinds::REGENERATION).is_empty()
    }

    /// Uses up the earliest regeneration shield on `object`, as it replaces a destruction
    /// ([CR 701.19a]): "The next time [permanent] would be destroyed this turn, instead
    /// remove all damage marked on it and its controller taps it." Returns whether there was
    /// one to use up.
    ///
    /// [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
    pub fn use_regeneration_shield(&mut self, object: ObjectRef<L>) -> bool {
        let affected = Affects::object(object);
        let shield =
            |e: &ContinuousEffect<L>| e.payload == Payload::Regeneration && e.affected == affected;
        let Some(earliest) = self.effects.of(Kinds::REGENERATION).iter().position(shield) else {
            return false;
        };
        // `end` visits the entries in order, so counting them finds the earliest shield.
        let at = Cell::new(0);
        let done = self.effects.end(|_| {
            let i = at.replace(at.get() + 1);
            i != earliest
        });
        self.settle_ended(done);
        true
    }
}
