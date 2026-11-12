//! Casting spells and activating abilities as an explicit machine (architecture §7.4): announce,
//! face, alternative and additional costs, X, targets, total cost, payment plan, payment.
//!
//! [CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012): "To cast a spell is to take
//! it from where it is (usually the hand), put it on the stack, and pay its costs, so that it
//! will eventually resolve and have its effect. ... If a player is unable to comply with the
//! requirements of a step listed below while performing that step, the casting of the spell
//! is illegal; the game returns to the moment before the casting of that spell was proposed".
//!
//! Spells that cannot be paid for or targeted are never offered, and every step offers only
//! answers that leave the rest completable, so the machine never has to rewind.

mod costs;
mod legal;
mod parts;
mod stack;
mod steps;
mod targets;
mod timing;

pub use costs::cost_being_cast;
pub use legal::pointless;
pub(crate) use legal::{priority_options, refusal};
pub(crate) use stack::{leave_stack, offer_options, unstack};
pub(crate) use steps::{begin_activate, begin_cast, decision, perform};
pub(crate) use targets::{spells, still_legal, target_trigger};
pub(crate) use timing::{entry_ability, sorcery_timing};
