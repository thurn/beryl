//! Tracks references that an earlier instruction in the resolution could have moved.

use crate::PutOnto;
use crate::effect::{Effect, Objects};

/// The objects an instruction sets out to move from where they are, if it names them: a new object
/// once it has ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object that
/// moves from one zone to another becomes a new object with no memory of, or relation to, its
/// previous existence."
pub(super) const fn moved_by(effect: &Effect) -> Option<&Objects> {
    match effect {
        Effect::Counter(objects)
        | Effect::CounterInto {
            spells: objects, ..
        }
        | Effect::Destroy { objects, .. }
        | Effect::Exile(objects)
        | Effect::ExileUntilThisLeaves(objects)
        | Effect::ExileLinked(objects)
        | Effect::Sacrifice(objects)
        | Effect::ReturnToHand(objects)
        | Effect::PutIntoGraveyard(objects)
        | Effect::ReturnToBattlefield(objects)
        | Effect::PutOntoBattlefield(PutOnto { objects, .. })
        | Effect::ReturnTransformed(objects)
        | Effect::PutIntoLibrary { objects, .. }
        | Effect::ShuffleInto { cards: objects, .. } => Some(objects),
        // It exiles this object, and the permanent it melds into is another object.
        Effect::Meld(_) => Some(&Objects::This),
        Effect::Sequence(_)
        | Effect::If { .. }
        | Effect::May { .. }
        | Effect::Unless { .. }
        | Effect::ForEachPlayer { .. }
        | Effect::Choose { .. }
        | Effect::Transform(_)
        | Effect::Flip(_)
        | Effect::Reveal(_)
        | Effect::LookAt { .. }
        | Effect::ChooseValue { .. }
        | Effect::CopySpell { .. }
        | Effect::Regenerate(_)
        | Effect::Tap(_)
        | Effect::Untap(_)
        | Effect::PhaseOut(_)
        | Effect::SetPrepared { .. }
        | Effect::PutBack { .. }
        | Effect::PutAnyOnBottom { .. }
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

/// The objects named one at a time that the instructions before the one being read may have
/// moved: a target by its place, the ability's source, what that is attached to, what the
/// ability triggered on and what a delayed ability carries. A set named by what is in it ([`Objects::All`] and the like) is
/// read anew each time, and [`Objects::Those`] follows what moved.
#[derive(Clone, Copy)]
pub(super) struct Moved(u32);

impl Moved {
    pub(super) const NONE: Moved = Moved(0);
    const THIS: u32 = 1 << 31;
    const ATTACHED_TO: u32 = 1 << 30;
    const TRIGGERED_ON: u32 = 1 << 29;
    const CARRIED: u32 = 1 << 28;

    /// The bit of `objects`, if it is such a name. A target past the bits is not followed.
    const fn bit(objects: &Objects) -> u32 {
        match objects {
            Objects::Target(target) if target.index() < 28 => 1 << target.index(),
            Objects::This => Moved::THIS,
            Objects::AttachedTo => Moved::ATTACHED_TO,
            Objects::TriggeredOn | Objects::TriggeredOnIn(_) => Moved::TRIGGERED_ON,
            Objects::Carried => Moved::CARRIED,
            Objects::Target(_)
            | Objects::Those
            | Objects::ExiledWithThis
            | Objects::LibraryTop { .. }
            | Objects::ExceptThose(_)
            | Objects::PutOntoBattlefieldWithThis
            | Objects::All { .. }
            | Objects::InZoneOf { .. }
            | Objects::CardsIn { .. }
            | Objects::Everywhere(_)
            | Objects::Paid(_)
            | Objects::Other(_) => 0,
        }
    }

    /// These and `objects`. A source that has left is attached to nothing
    /// ([`Objects::AttachedTo`]).
    pub(super) const fn with(self, objects: &Objects) -> Moved {
        let attached = match objects {
            Objects::This => Moved::ATTACHED_TO,
            Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::ExiledWithThis
            | Objects::LibraryTop { .. }
            | Objects::ExceptThose(_)
            | Objects::PutOntoBattlefieldWithThis
            | Objects::Carried
            | Objects::All { .. }
            | Objects::InZoneOf { .. }
            | Objects::CardsIn { .. }
            | Objects::Everywhere(_)
            | Objects::Paid(_)
            | Objects::Other(_) => 0,
        };
        Moved(self.0 | Moved::bit(objects) | attached)
    }

    /// What either of two ways through an effect may have moved.
    pub(super) const fn or(self, other: Moved) -> Moved {
        Moved(self.0 | other.0)
    }

    pub(super) const fn has(self, objects: &Objects) -> bool {
        self.0 & Moved::bit(objects) != 0
    }
}
