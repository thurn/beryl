//! [`Effect::asks`]: whether carrying out an effect may ask a player to choose.

use super::{Effect, LibraryPosition};
use crate::continuous::Modification;

impl Effect {
    /// Whether carrying this out may ask a player to choose
    /// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a
    /// spell or ability offers any choices other than choices already made as part of
    /// casting the spell, activating the ability, or otherwise putting the spell or ability
    /// on the stack, the player announces these while applying the effect." The kind of mana
    /// a mana ability adds is chosen as it is activated, so [`Effect::AddMana`] asks nothing, and
    /// whether to cast a card an [`Effect::MayCast`] offers is the engine's to ask once the
    /// effect has finished.
    pub const fn asks(&self) -> bool {
        match *self {
            Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::Discard { .. }
            | Effect::Scry { .. }
            | Effect::Explore(_)
            | Effect::UntapUpTo { .. }
            | Effect::ManifestDread(_)
            | Effect::CreateAttackingTokens { .. }
            | Effect::Choose { .. }
            // The two words are chosen as it resolves.
            | Effect::ChangeText { .. }
            | Effect::PutBack { .. }
            | Effect::Dig { .. }
            | Effect::Fateseal { .. }
            | Effect::Search { .. }
            | Effect::PutIntoLibrary {
                position: LibraryPosition::TopOrBottom,
                ..
            } => true,
            Effect::Sequence(effects) => {
                let mut i = 0;
                while i < effects.len() {
                    if effects[i].asks() {
                        return true;
                    }
                    i += 1;
                }
                false
            }
            Effect::If {
                then, otherwise, ..
            } => {
                then.asks()
                    || match otherwise {
                        Some(otherwise) => otherwise.asks(),
                        None => false,
                    }
            }
            Effect::ForEachPlayer { effect, .. } => effect.asks(),
            // A source of the controller's choice is chosen as the shield begins.
            Effect::PreventDamage { from, .. } => from.is_some(),
            Effect::Modify { modifications, .. } => Modification::any_asks(modifications),
            Effect::LookAt { .. }
            | Effect::Counter(_)
            | Effect::CounterInto { .. }
            | Effect::Destroy { .. }
            | Effect::Regenerate(_)
            | Effect::Exile(_)
            | Effect::ExileUntilThisLeaves(_)
            | Effect::ExileLinked(_)
            | Effect::Sacrifice(_)
            | Effect::ReturnToHand(_)
            | Effect::PutIntoGraveyard(_)
            | Effect::ReturnToBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::PutIntoLibrary { .. }
            | Effect::GainFlashback(_)
            | Effect::PermitCast { .. }
            | Effect::ShuffleInto { .. }
            | Effect::Draw { .. }
            | Effect::Mill { .. }
            | Effect::GainLife { .. }
            | Effect::LoseLife { .. }
            | Effect::CreateTokens { .. }
            | Effect::CreateCopies { .. }
            | Effect::AddCounters { .. }
            | Effect::RemoveCounters { .. }
            | Effect::AddMana { .. }
            | Effect::AddManaOfKind { .. }
            | Effect::ExchangeControl(..)
            | Effect::Attach { .. }
            | Effect::FlipCoin(_)
            | Effect::DealDamage { .. }
            | Effect::ChangeRule { .. }
            | Effect::MayCast { .. }
            | Effect::Delay { .. }
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_) => false,
        }
    }
}
