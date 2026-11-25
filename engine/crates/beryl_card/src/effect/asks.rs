//! [`Effect::asks`]: whether carrying out an effect may ask a player to choose.

use super::{Effect, LibraryPosition, Objects};
use crate::continuous::Modification;

impl Effect {
    /// Whether carrying this out may ask a player to choose
    /// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a
    /// spell or ability offers any choices other than choices already made as part of
    /// casting the spell, activating the ability, or otherwise putting the spell or ability
    /// on the stack, the player announces these while applying the effect." The kind of mana
    /// a mana ability adds is chosen as it is activated, so [`Effect::AddMana`] asks nothing.
    #[allow(clippy::too_many_lines)] // One exhaustive match, a line per instruction.
    pub const fn asks(&self) -> bool {
        match *self {
            Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::Discard { .. }
            | Effect::Scry { .. }
            | Effect::Explore(_)
            // Over or under is chosen as it resolves.
            | Effect::Mutate(_)
            | Effect::UntapUpTo { .. }
            | Effect::ManifestDread(_)
            | Effect::CreateAttackingTokens { .. }
            | Effect::Choose { .. }
            // The two words are chosen as it resolves.
            | Effect::ChangeText { .. }
            | Effect::PutBack { .. }
            | Effect::PutAnyOnBottom { .. }
            | Effect::Dig { .. }
            | Effect::Fateseal { .. }
            | Effect::Wish { .. }
            | Effect::Search { .. }
            | Effect::MayCast { .. }
            // State-based actions can ask which legendary permanent to keep.
            | Effect::RestartGame { .. }
            | Effect::EndTurn
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
            Effect::ExileUntil { when_found, .. } => when_found.asks(),
            // The copies' new targets are chosen as they are made (CR 707.10c).
            Effect::CopySpell { new_targets, .. } => new_targets,
            // A source of the controller's choice is chosen as the shield begins.
            Effect::PreventDamage { from, .. } => from.is_some(),
            Effect::Modify { modifications, .. } => Modification::any_asks(modifications),
            Effect::LookAt { .. }
            | Effect::Counter(_)
            | Effect::CounterInto { .. }
            | Effect::Destroy { .. }
            | Effect::Regenerate(_)
            | Effect::Tap(_)
            | Effect::Untap(_)
            | Effect::PhaseOut(_)
            | Effect::SetPrepared { .. }
            | Effect::Exile(_)
            | Effect::ExileUntilThisLeaves(_)
            | Effect::ExileLinked(_)
            | Effect::Sacrifice(_)
            | Effect::ReturnToHand(_)
            | Effect::PutIntoGraveyard(_)
            | Effect::ReturnToBattlefield(_)
            | Effect::PutOntoBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::Transform(_)
            | Effect::Meld(_)
            | Effect::Flip(_)
            | Effect::Reveal(_)
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
            | Effect::BecomeCopy { .. }
            | Effect::AddCounters { .. }
            | Effect::RemoveCounters { .. }
            | Effect::RemoveAllCounters(_)
            | Effect::AddMana { .. }
            | Effect::AddManaOfKind { .. }
            | Effect::ExchangeControl(..)
            | Effect::Attach { .. }
            | Effect::Subgame
            | Effect::FlipCoin(_)
            | Effect::DealDamage { .. }
            | Effect::ChangeRule { .. }
            | Effect::Delay { .. }
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_)
            | Effect::AdditionalStep(_)
            | Effect::AdditionalPhases { .. }
            | Effect::ExtraTurn(..) => false,
        }
    }

    /// What the hidden pick of this effect, if it has one, picks among: an
    /// [`Effect::Choose`] with `hidden`, as each player's pick of a card from their hand in
    /// turn order before the cards are put onto the battlefield together ([CR 101.4a]): "If an
    /// effect has each player choose a card in a hidden zone, such as their hand or library,
    /// those cards may remain face down as they're chosen. However, each player must clearly
    /// indicate which face-down card they are choosing." A hidden pick is optional, and is
    /// asked whether or not any candidate passes, so that the other players do not learn from
    /// being asked or not. Its candidates are the cards of a hidden zone
    /// ([`crate::Malformed::Chosen`]), and an effect has at most one.
    ///
    /// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
    pub fn hidden_pick(&self) -> Option<Objects> {
        let found = core::cell::Cell::new(None);
        self.any(&|effect| {
            if let Some((among, _, true)) = effect.choice() {
                found.set(Some(among));
                true
            } else {
                false
            }
        });
        found.get()
    }

    /// The candidates of a pick, whether it is optional, and whether it is hidden.
    pub(super) const fn choice(&self) -> Option<(Objects, bool, bool)> {
        match *self {
            Effect::Choose {
                among,
                optional,
                hidden,
                ..
            } => Some((among, optional, hidden)),
            Effect::Sequence(_)
            | Effect::If { .. }
            | Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::ForEachPlayer { .. }
            | Effect::LookAt { .. }
            | Effect::Counter(_)
            | Effect::CounterInto { .. }
            | Effect::CopySpell { .. }
            | Effect::Destroy { .. }
            | Effect::Regenerate(_)
            | Effect::Tap(_)
            | Effect::Untap(_)
            | Effect::PhaseOut(_)
            | Effect::SetPrepared { .. }
            | Effect::Exile(_)
            | Effect::ExileUntilThisLeaves(_)
            | Effect::ExileLinked(_)
            | Effect::Sacrifice(_)
            | Effect::ReturnToHand(_)
            | Effect::PutIntoGraveyard(_)
            | Effect::ReturnToBattlefield(_)
            | Effect::PutOntoBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::Transform(_)
            | Effect::Meld(_)
            | Effect::Flip(_)
            | Effect::Reveal(_)
            | Effect::PutIntoLibrary { .. }
            | Effect::PutBack { .. }
            | Effect::PutAnyOnBottom { .. }
            | Effect::ShuffleInto { .. }
            | Effect::Dig { .. }
            | Effect::ExileUntil { .. }
            | Effect::Fateseal { .. }
            | Effect::Wish { .. }
            | Effect::Search { .. }
            | Effect::Draw { .. }
            | Effect::Discard { .. }
            | Effect::Mill { .. }
            | Effect::Scry { .. }
            | Effect::Explore(_)
            | Effect::ManifestDread(_)
            | Effect::Subgame
            | Effect::FlipCoin(_)
            | Effect::DealDamage { .. }
            | Effect::PreventDamage { .. }
            | Effect::GainLife { .. }
            | Effect::LoseLife { .. }
            | Effect::CreateTokens { .. }
            | Effect::CreateAttackingTokens { .. }
            | Effect::CreateCopies { .. }
            | Effect::BecomeCopy { .. }
            | Effect::Mutate(_)
            | Effect::AddCounters { .. }
            | Effect::RemoveCounters { .. }
            | Effect::RemoveAllCounters(_)
            | Effect::AddMana { .. }
            | Effect::AddManaOfKind { .. }
            | Effect::Modify { .. }
            | Effect::ChangeText { .. }
            | Effect::ExchangeControl(..)
            | Effect::Attach { .. }
            | Effect::ChangeRule { .. }
            | Effect::GainFlashback(_)
            | Effect::PermitCast { .. }
            | Effect::MayCast { .. }
            | Effect::UntapUpTo { .. }
            | Effect::Delay { .. }
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_)
            | Effect::RestartGame { .. }
            | Effect::EndTurn
            | Effect::AdditionalStep(_)
            | Effect::AdditionalPhases { .. }
            | Effect::ExtraTurn(..) => None,
        }
    }
}
