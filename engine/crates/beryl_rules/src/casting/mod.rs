//! Casting spells and activating abilities as an explicit machine (architecture §7.4): announce,
//! face, alternative and additional costs, X, targets, total cost, payment plan, payment.
//!
//! Implements: CR 118.3, CR 601, CR 602.2, CR 602.5
//!
//! [CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012): "To cast a spell is to take
//! it from where it is (usually the hand), put it on the stack, and pay its costs, so that it
//! will eventually resolve and have its effect. ... If a player is unable to comply with the
//! requirements of a step listed below while performing that step, the casting of the spell
//! is illegal; the game returns to the moment before the casting of that spell was proposed".
//!
//! Spells that cannot be paid for or targeted are never offered, and every step offers only
//! answers that leave the rest completable, so the machine never has to rewind.

mod adventure;
mod costs;
mod delve;
mod graveyard;
pub(crate) mod land;
mod legal;
mod mana_payment;
pub(crate) mod parts;
pub mod payment;
mod spent;
mod stack;
pub mod start;
mod steps;
mod targets;
mod timing;

pub(crate) use adventure::finish_adventure;
pub use costs::cost_being_cast;
pub use legal::{payable_responses, pointless};
pub(crate) use legal::{priority_options, refusal};
pub(crate) use spent::pays as pays_from_spent;
pub(crate) use stack::{leave_stack, unstack};
pub(crate) use start::search_cast_options;
pub(crate) use steps::{begin_activate, begin_cast, cast_before, decision, perform, targeted};
pub use targets::target_takes;
pub(crate) use targets::{
    TargetSet, new_targets, player_matches, spells, still_legal, target_trigger,
};
pub(crate) use timing::sorcery_timing;
