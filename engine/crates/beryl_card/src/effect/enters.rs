//! [`Effect::may_enter_permanent`]: whether carrying out an effect may put a permanent onto
//! the battlefield.

use super::{Effect, Found};
use crate::characteristics::Supertypes;
use crate::filter::Filter;

/// Whether `filter` requires the Basic supertype: a basic land never chooses as it enters
/// and is never an Aura.
fn requires_basic(filter: Filter) -> bool {
    match filter {
        Filter::Supertypes(s) => s.contains(Supertypes::BASIC),
        Filter::All(fs) => fs.iter().any(|&f| requires_basic(f)),
        Filter::Any
        | Filter::Types(_)
        | Filter::SubtypeWord(_)
        | Filter::ColorWords(_)
        | Filter::Whose(_)
        | Filter::Owned(_)
        | Filter::Tapped
        | Filter::AttackedThisTurn
        | Filter::WithCounter(_)
        | Filter::Token
        | Filter::Named(_)
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::ManaValueAtMostPower
        | Filter::Not(_)
        | Filter::AnyOf(_)
        | Filter::InZone(_) => false,
    }
}

impl Effect {
    /// Whether carrying this out may put a permanent onto the battlefield, so that the
    /// permanent may have a choice to make as it enters, such as the Aura's of what it will
    /// enchant (see CR 303.4f). The resolving permanent spell is not counted: it is not an
    /// instruction of the effect. A sequence or a branch does when an instruction inside it
    /// does.
    pub fn may_enter_permanent(&self) -> bool {
        self.any(&Effect::enters_itself)
    }

    /// Whether this instruction, not one inside it, may put a permanent onto the battlefield.
    /// One that does so only later, as a delayed trigger does, does not: that resolves apart.
    #[allow(clippy::too_many_lines)] // One exhaustive match, a line per instruction.
    pub fn enters_itself(&self) -> bool {
        match *self {
            Effect::ExileUntil { .. }
            | Effect::ReturnToBattlefield(_)
            | Effect::PutOntoBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::Meld(_)
            | Effect::CreateCopies { .. }
            // A subgame or a restarted game plays out whatever its cards do.
            | Effect::Subgame
            | Effect::RestartGame { .. } => true,
            // A search puts what it finds onto the battlefield only when it says so, and a
            // basic land never chooses as it enters.
            Effect::Search { filter, found, .. } => {
                matches!(found, Found::OntoBattlefield | Found::OntoBattlefieldTapped)
                    && !requires_basic(filter)
            }
            Effect::Sequence(_)
            | Effect::If { .. }
            | Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::ForEachPlayer { .. }
            | Effect::Choose { .. }
            | Effect::ChooseValue { .. }
            | Effect::ChooseAtRandom(_)
            | Effect::LookAt { .. }
            // A player takes the cards they dig for into their hand (`Taken`).
            | Effect::Dig { .. }
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
            | Effect::Transform(_)
            | Effect::Flip(_)
            | Effect::Reveal(_)
            | Effect::PutIntoLibrary { .. }
            | Effect::PutBack { .. }
            | Effect::PutAnyOnBottom { .. }
            | Effect::ShuffleInto { .. }
            | Effect::Fateseal { .. }
            | Effect::Wish { .. }
            | Effect::Draw { .. }
            | Effect::Discard { .. }
            | Effect::Mill { .. }
            | Effect::Scry { .. }
            | Effect::Explore(_)
            | Effect::ManifestDread(_)
            | Effect::FlipCoin(_)
            | Effect::DealDamage { .. }
            | Effect::PreventDamage { .. }
            | Effect::GainLife { .. }
            | Effect::LoseLife { .. }
            | Effect::CreateTokens { .. }
            | Effect::CreateAttackingTokens { .. }
            | Effect::BecomeCopy { .. }
            | Effect::Mutate(_)
            | Effect::AddCounters { .. }
            | Effect::RemoveCounters { .. }
            | Effect::RemoveAllCounters(_)
            | Effect::DoubleCounters { .. }
            | Effect::AddMana { .. }
            | Effect::AddManaOfKind { .. }
            | Effect::AddProduct { .. }
            | Effect::Modify { .. }
            | Effect::ChangeText { .. }
            | Effect::ExchangeControl(..)
            | Effect::Attach { .. }
            | Effect::ChangeRule { .. }
            | Effect::ChangeMana { .. }
            | Effect::GainFlashback(_)
            | Effect::PermitCast { .. }
            | Effect::MayCast { .. }
            | Effect::UntapUpTo { .. }
            | Effect::Delay { .. }
            | Effect::Reflexive { .. }
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_)
            | Effect::ExtraTurn(..)
            | Effect::AdditionalStep(_)
            | Effect::AdditionalPhases { .. }
            | Effect::EndTurn => false,
        }
    }
}
