//! Prevention shields in the Effects store ([CR 615.1]): "Such effects watch for a damage event
//! that would happen and completely or partially prevent the damage that would be dealt. They
//! act like “shields” around whatever they're affecting."
//!
//! Each shield is one entry on the player or the permanent it protects, with the payload
//! [`Payload::Prevent`], lasting until end of turn. It says how much of the next damage it
//! prevents ([`Shield::left`]) and, for a shield against a source of a player's choice, which
//! source ([`Shield::from`]). It is used up as it applies: the rules lower what is left of it
//! ([`State::prevent_with`]) and end it once nothing is.
//!
//! [CR 615.1]: https://yawgatog.com/resources/magic-rules/#R6151

use crate::layout::{Layout, Queue, Small};
use crate::object::ObjectRef;
use crate::relayout::{Relayout, slot};
use crate::state::State;
use beryl_card::{FaceAbility, FaceIx};

use super::{ContinuousEffect, Kinds, Payload, Subject};

/// A prevention shield ([`Payload::Prevent`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shield<L: Layout = Small> {
    /// How much damage it has left to prevent, or `None` for all the damage of the next event
    /// ([CR 615.8]): "These effects prevent the next instance of damage from that source,
    /// regardless of how much damage that is." A shield of an amount ([CR 615.7]) never has
    /// 0 left: it ends as its last damage is prevented.
    ///
    /// [CR 615.7]: https://yawgatog.com/resources/magic-rules/#R6157
    /// [CR 615.8]: https://yawgatog.com/resources/magic-rules/#R6158
    pub left: Option<u8>,
    /// The source it prevents damage from, if it was chosen for one; damage from any source
    /// otherwise.
    pub from: Option<ChosenSource<L>>,
}

/// The source of damage a player chose for a shield ([CR 609.7a]): "If an effect requires a
/// player to choose a source of damage, they may choose a permanent; a spell on the stack
/// (including a permanent spell); any object referred to by an object on the stack".
///
/// [CR 609.7a]: https://yawgatog.com/resources/magic-rules/#R6097a
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChosenSource<L: Layout = Small> {
    /// The source as it was chosen. Once it has changed zones it is a new object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which the shield
    /// does not prevent damage from.
    pub object: ObjectRef<L>,
    /// The face of the shield's card, and its ability (none for the face's spell), that made
    /// the shield: the filter it holds is what the source must still pass as it would deal
    /// damage ([CR 615.9]): "When the source would deal damage, the shield rechecks the
    /// source's properties. If the properties no longer match, the damage isn't prevented or
    /// replaced and the shield isn't used up."
    ///
    /// [CR 615.9]: https://yawgatog.com/resources/magic-rules/#R6159
    pub face: FaceIx,
    pub ability: Option<FaceAbility>,
}

impl<L: Layout> Relayout for Payload<L> {
    type In<M: Layout> = Payload<M>;

    fn relayout<M: Layout>(&self) -> Payload<M> {
        match *self {
            Payload::Layer(change) => Payload::Layer(change),
            Payload::Move(replacement) => Payload::Move(replacement),
            Payload::Rule(rule) => Payload::Rule(rule),
            Payload::Cast(terms) => Payload::Cast(terms),
            Payload::Cost(change) => Payload::Cost(change),
            Payload::Regeneration => Payload::Regeneration,
            Payload::Prevent(Shield { left, from }) => Payload::Prevent(Shield {
                left,
                from: from.map(|c| ChosenSource {
                    object: ObjectRef {
                        slot: slot(c.object.slot),
                        incarnation: c.object.incarnation,
                    },
                    face: c.face,
                    ability: c.ability,
                }),
            }),
        }
    }
}

/// A shield on a player or a permanent as the rules read it: where it is in the store, the
/// card whose spell or ability made it, and the shield.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShieldAt<L: Layout = Small> {
    /// Its place among the store's entries, which [`State::prevent_with`] takes back.
    pub at: u8,
    pub source: L::Card,
    /// When it began: what tells it from another shield as entries before it end.
    pub timestamp: u16,
    pub shield: Shield<L>,
}

impl<L: Layout> State<L> {
    /// Whether a prevention shield is on any player or permanent: one AND, which is all that
    /// damage in a game with no shield pays for them.
    #[inline]
    pub fn has_prevention_shields(&self) -> bool {
        !self.effects.of(Kinds::PREVENTION).is_empty()
    }

    /// The prevention shields on `recipient`, in the order they began.
    pub fn shields_on(&self, recipient: Subject<L>) -> impl Iterator<Item = ShieldAt<L>> + '_ {
        let affected = recipient.stored();
        (0u8..)
            .zip(self.effects.of(Kinds::PREVENTION))
            .filter_map(move |(at, e)| match e.payload {
                Payload::Prevent(shield) if e.affected == affected => Some(ShieldAt {
                    at,
                    source: e.source,
                    timestamp: e.timestamp,
                    shield,
                }),
                _ => None,
            })
    }

    /// Prevents up to `amount` damage with the shield at `at` ([`ShieldAt::at`]), and returns
    /// how much it prevented. A shield of an amount loses what it prevents ([CR 615.7]):
    /// "Preventing 1 damage reduces the remaining shield by 1." It ends once nothing is left
    /// of it, as a shield for the next event does after one ([CR 615.8]).
    ///
    /// [CR 615.7]: https://yawgatog.com/resources/magic-rules/#R6157
    /// [CR 615.8]: https://yawgatog.com/resources/magic-rules/#R6158
    pub fn prevent_with(&mut self, at: u8, amount: u8) -> u8 {
        let at = usize::from(at);
        let e = &mut self.effects.entries.as_mut_slice()[at];
        let Payload::Prevent(shield) = &mut e.payload else {
            panic!("no prevention shield at {at}");
        };
        let (prevented, left) = match shield.left {
            Some(left) => (amount.min(left), left - amount.min(left)),
            None => (amount, 0),
        };
        if left > 0 {
            shield.left = Some(left);
            return prevented;
        }
        // `end` visits the entries in order, so counting them finds this one.
        let seen = std::cell::Cell::new(0);
        let done = self.effects.end(|_: &ContinuousEffect<L>| {
            let i = seen.replace(seen.get() + 1);
            i != at
        });
        self.settle_ended(done);
        prevented
    }
}
