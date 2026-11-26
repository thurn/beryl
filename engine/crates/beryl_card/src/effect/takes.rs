//! Whether an instruction removes or counters what a target chose.

use super::{Effect, Objects};
use crate::index::SpecIx;
use crate::target::Takes;

impl Effect {
    /// Whether this takes what was chosen for the `target`th instance of the word "target"
    /// from where it is in the way `how` names, whatever else it does and whatever it asks
    /// first: a removal destroys, exiles, sacrifices or returns it, or puts it into a
    /// library, and a counter counters it. A player is told as they choose the target
    /// ([`crate::Ability::takes_target`]).
    #[allow(clippy::too_many_lines)] // One exhaustive match, a line per instruction.
    pub const fn takes(&self, target: SpecIx, how: Takes) -> bool {
        match *self {
            Effect::Destroy { objects, .. }
            | Effect::Exile(objects)
            | Effect::ExileUntilThisLeaves(objects)
            | Effect::ExileLinked(objects)
            | Effect::Sacrifice(objects)
            | Effect::ReturnToHand(objects)
            | Effect::PutIntoGraveyard(objects)
            | Effect::PutIntoLibrary { objects, .. } => {
                matches!(how, Takes::Removal) && is_target(objects, target)
            }
            Effect::Counter(objects)
            | Effect::CounterInto {
                spells: objects, ..
            } => matches!(how, Takes::Counter) && is_target(objects, target),
            Effect::Sequence(effects) => {
                let mut i = 0;
                while i < effects.len() {
                    if effects[i].takes(target, how) {
                        return true;
                    }
                    i += 1;
                }
                false
            }
            Effect::If {
                then, otherwise, ..
            } => {
                then.takes(target, how)
                    || match otherwise {
                        Some(otherwise) => otherwise.takes(target, how),
                        None => false,
                    }
            }
            Effect::May { effect, .. }
            | Effect::Unless { effect, .. }
            | Effect::ForEachPlayer { effect, .. } => effect.takes(target, how),
            Effect::Attach { .. }
            | Effect::Mutate(_)
            | Effect::CopySpell { .. }
            | Effect::ExchangeControl(..)
            | Effect::Regenerate(_)
            | Effect::Tap(_)
            | Effect::Untap(_)
            | Effect::PhaseOut(_)
            | Effect::SetPrepared { .. }
            | Effect::PreventDamage { .. }
            | Effect::Draw { .. }
            | Effect::Discard { .. }
            | Effect::Mill { .. }
            | Effect::Scry { .. }
            | Effect::PutBack { .. }
            | Effect::PutAnyOnBottom { .. }
            | Effect::ShuffleInto { .. }
            | Effect::Dig { .. }
            | Effect::ExileUntil { .. }
            | Effect::Fateseal { .. }
            | Effect::Wish { .. }
            | Effect::Search { .. }
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
            | Effect::Modify { .. }
            | Effect::ChangeText { .. }
            | Effect::Explore(_)
            | Effect::Choose { .. }
            | Effect::LookAt { .. }
            | Effect::ChooseValue { .. }
            | Effect::ManifestDread(_)
            | Effect::CreateAttackingTokens { .. }
            | Effect::ReturnToBattlefield(_)
            | Effect::PutOntoBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::Transform(_)
            | Effect::Meld(_)
            | Effect::Flip(_)
            | Effect::Reveal(_)
            | Effect::GainFlashback(_)
            | Effect::PermitCast { .. }
            | Effect::Subgame
            | Effect::FlipCoin(_)
            | Effect::DealDamage { .. }
            | Effect::ChangeRule { .. }
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
            | Effect::ExtraTurn(..) => false,
        }
    }
}

/// Whether `objects` is what was chosen for the `target`th instance of the word "target".
const fn is_target(objects: Objects, target: SpecIx) -> bool {
    matches!(objects, Objects::Target(n) if n.get() == target.get())
}
