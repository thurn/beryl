//! Read an instruction and visit the instructions it can run.

use super::{Condition, Effect, Objects};
use crate::continuous::{Modification, RuleChange};
use crate::cost::Cost;

/// The instructions an effect can run, with the condition or cost that controls them.
pub(crate) enum Branches<'a> {
    Sequence(&'static [Effect]),
    If {
        condition: &'a Condition,
        then: &'static Effect,
        otherwise: Option<&'static Effect>,
    },
    May(&'static Effect),
    Unless(&'static Effect, &'a Cost),
    ForEachPlayer(&'static Effect),
    WhenFound(&'static Effect),
    Single,
}

impl Effect {
    /// How it reaches other instructions, if it contains any.
    #[inline]
    pub(crate) const fn branches(&self) -> Branches<'_> {
        match self {
            Effect::Sequence(effects) => Branches::Sequence(effects),
            Effect::If {
                condition,
                then,
                otherwise,
            } => Branches::If {
                condition,
                then,
                otherwise: *otherwise,
            },
            Effect::May { effect, .. } => Branches::May(effect),
            Effect::Unless { effect, cost, .. } => Branches::Unless(effect, cost),
            Effect::ForEachPlayer { effect, .. } => Branches::ForEachPlayer(effect),
            Effect::ExileUntil { when_found, .. } => Branches::WhenFound(when_found),
            Effect::Choose { .. }
            | Effect::LookAt { .. }
            | Effect::Counter(_)
            | Effect::CounterInto { .. }
            | Effect::CopySpell { .. }
            | Effect::Destroy { .. }
            | Effect::Regenerate(_)
            | Effect::PhaseOut(_)
            | Effect::Exile(_)
            | Effect::ExileUntilThisLeaves(_)
            | Effect::ExileLinked(_)
            | Effect::Sacrifice(_)
            | Effect::ReturnToHand(_)
            | Effect::PutIntoGraveyard(_)
            | Effect::ReturnToBattlefield(_)
            | Effect::PutOntoBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::PutIntoLibrary { .. }
            | Effect::PutBack { .. }
            | Effect::ShuffleInto { .. }
            | Effect::Dig { .. }
            | Effect::Fateseal { .. }
            | Effect::Wish { .. }
            | Effect::Search { .. }
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
            | Effect::CreateCopies { .. }
            | Effect::BecomeCopy { .. }
            | Effect::Mutate(_)
            | Effect::AddCounters { .. }
            | Effect::RemoveCounters { .. }
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
            | Effect::EndTurn
            | Effect::ExtraTurn(_) => Branches::Single,
        }
    }

    /// The layer modifications this instruction starts.
    pub const fn modifications(&self) -> &'static [Modification] {
        match *self {
            Effect::Modify { modifications, .. } => modifications,
            Effect::Sequence(_)
            | Effect::If { .. }
            | Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::ForEachPlayer { .. }
            | Effect::Choose { .. }
            | Effect::LookAt { .. }
            | Effect::Counter(_)
            | Effect::CounterInto { .. }
            | Effect::CopySpell { .. }
            | Effect::Destroy { .. }
            | Effect::Regenerate(_)
            | Effect::PhaseOut(_)
            | Effect::Exile(_)
            | Effect::ExileUntilThisLeaves(_)
            | Effect::ExileLinked(_)
            | Effect::Sacrifice(_)
            | Effect::ReturnToHand(_)
            | Effect::PutIntoGraveyard(_)
            | Effect::ReturnToBattlefield(_)
            | Effect::PutOntoBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::PutIntoLibrary { .. }
            | Effect::PutBack { .. }
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
            | Effect::AddMana { .. }
            | Effect::AddManaOfKind { .. }
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
            | Effect::EndTurn
            | Effect::ExtraTurn(_) => &[],
        }
    }

    /// Whether `test` holds of this instruction or of one inside it, whichever of them a
    /// resolution comes to.
    pub fn any(&self, test: &impl Fn(&Effect) -> bool) -> bool {
        if test(self) {
            return true;
        }
        match self.branches() {
            Branches::Sequence(effects) => effects.iter().any(|effect| effect.any(test)),
            Branches::If {
                then, otherwise, ..
            } => then.any(test) || otherwise.is_some_and(|otherwise| otherwise.any(test)),
            Branches::May(effect)
            | Branches::Unless(effect, _)
            | Branches::ForEachPlayer(effect)
            | Branches::WhenFound(effect) => effect.any(test),
            Branches::Single => false,
        }
    }

    /// The objects its player must choose one of before it does anything else
    /// ([`Effect::Choose`], not optional): with none of them it does nothing at all, which a
    /// player about to pay for it can be told ([`crate::Ability::needs`]).
    pub const fn needs(&self) -> Option<Objects> {
        if let Branches::Sequence([first, ..]) = self.branches() {
            return first.needs();
        }
        match self.choice() {
            Some((among, false, _)) => Some(among),
            Some(_) | None => None,
        }
    }

    /// The rule change this starts as it resolves ([`Effect::ChangeRule`]), whichever of its
    /// instructions does: what its ability names to the game ([`crate::Ability::rule`]).
    pub const fn starts(&self) -> Option<&'static RuleChange> {
        match self.branches() {
            Branches::Sequence(effects) => {
                let mut i = 0;
                while i < effects.len() {
                    if let Some(rule) = effects[i].starts() {
                        return Some(rule);
                    }
                    i += 1;
                }
                None
            }
            Branches::If {
                then, otherwise, ..
            } => match (then.starts(), otherwise) {
                (Some(rule), _) => Some(rule),
                (None, Some(otherwise)) => otherwise.starts(),
                (None, None) => None,
            },
            Branches::May(effect)
            | Branches::Unless(effect, _)
            | Branches::ForEachPlayer(effect)
            | Branches::WhenFound(effect) => effect.starts(),
            Branches::Single => self.started_rule(),
        }
    }

    /// The rule change this instruction itself starts.
    pub(crate) const fn started_rule(&self) -> Option<&'static RuleChange> {
        match self {
            Effect::ChangeRule { rule, .. } => Some(rule),
            Effect::Sequence(_)
            | Effect::If { .. }
            | Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::ForEachPlayer { .. }
            | Effect::Choose { .. }
            | Effect::LookAt { .. }
            | Effect::Counter(_)
            | Effect::CounterInto { .. }
            | Effect::CopySpell { .. }
            | Effect::Destroy { .. }
            | Effect::Regenerate(_)
            | Effect::PhaseOut(_)
            | Effect::Exile(_)
            | Effect::ExileUntilThisLeaves(_)
            | Effect::ExileLinked(_)
            | Effect::Sacrifice(_)
            | Effect::ReturnToHand(_)
            | Effect::PutIntoGraveyard(_)
            | Effect::ReturnToBattlefield(_)
            | Effect::PutOntoBattlefield(_)
            | Effect::ReturnTransformed(_)
            | Effect::PutIntoLibrary { .. }
            | Effect::PutBack { .. }
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
            | Effect::AddMana { .. }
            | Effect::AddManaOfKind { .. }
            | Effect::Modify { .. }
            | Effect::ChangeText { .. }
            | Effect::ExchangeControl(..)
            | Effect::Attach { .. }
            | Effect::GainFlashback(_)
            | Effect::PermitCast { .. }
            | Effect::MayCast { .. }
            | Effect::UntapUpTo { .. }
            | Effect::Delay { .. }
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_)
            | Effect::EndTurn
            | Effect::ExtraTurn(_) => None,
        }
    }
}
