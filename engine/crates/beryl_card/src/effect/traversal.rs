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
            | Effect::ChooseAtRandom(_)
            | Effect::LookAt { .. }
            | Effect::ChooseValue { .. }
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
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_)
            | Effect::RestartGame { .. }
            | Effect::EndTurn
            | Effect::AdditionalStep(_)
            | Effect::AdditionalPhases { .. }
            | Effect::ExtraTurn(..) => Branches::Single,
        }
    }

    /// Whether any branch can suspend resolution to play a subgame.
    pub(crate) const fn starts_subgame(&self) -> bool {
        match self.branches() {
            Branches::Sequence(effects) => {
                let mut index = 0;
                while index < effects.len() {
                    if effects[index].starts_subgame() {
                        return true;
                    }
                    index += 1;
                }
                false
            }
            Branches::If {
                then, otherwise, ..
            } => {
                then.starts_subgame()
                    || match otherwise {
                        Some(effect) => effect.starts_subgame(),
                        None => false,
                    }
            }
            Branches::May(effect)
            | Branches::Unless(effect, _)
            | Branches::ForEachPlayer(effect)
            | Branches::WhenFound(effect) => effect.starts_subgame(),
            Branches::Single => matches!(self, Effect::Subgame),
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
            | Effect::ChooseAtRandom(_)
            | Effect::LookAt { .. }
            | Effect::ChooseValue { .. }
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
            | Effect::AddProduct { .. }
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
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_)
            | Effect::RestartGame { .. }
            | Effect::EndTurn
            | Effect::AdditionalStep(_)
            | Effect::AdditionalPhases { .. }
            | Effect::ExtraTurn(..) => &[],
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
            | Effect::ChooseAtRandom(_)
            | Effect::LookAt { .. }
            | Effect::ChooseValue { .. }
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
            | Effect::AddProduct { .. }
            | Effect::Modify { .. }
            | Effect::ChangeText { .. }
            | Effect::ExchangeControl(..)
            | Effect::Attach { .. }
            | Effect::ChangeMana { .. }
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

    /// The `part`-th [`Effect::ChangeMana`] instruction inside this effect in pre-order traversal.
    pub fn mana_change_at(&self, part: u8) -> Option<&'static crate::ManaChange> {
        let mut seen = 0u8;
        self.find_mana_change(&mut |change| {
            let found = seen == part;
            seen = seen.saturating_add(1);
            found.then_some(change)
        })
    }

    /// The pre-order index of `target` (by pointer identity) among the [`Effect::ChangeMana`]
    /// instructions inside this effect.
    pub fn mana_change_part(&self, target: &'static crate::ManaChange) -> Option<u8> {
        let mut seen = 0u8;
        self.find_mana_change(&mut |change| {
            let index = seen;
            seen = seen.saturating_add(1);
            core::ptr::eq(change, target).then_some(index)
        })
    }

    fn find_mana_change<T>(
        &self,
        visit: &mut impl FnMut(&'static crate::ManaChange) -> Option<T>,
    ) -> Option<T> {
        if let Effect::ChangeMana { change, .. } = *self
            && let Some(found) = visit(change)
        {
            return Some(found);
        }
        match self.branches() {
            Branches::Sequence(effects) => {
                for effect in effects {
                    if let Some(found) = effect.find_mana_change(visit) {
                        return Some(found);
                    }
                }
                None
            }
            Branches::If {
                then, otherwise, ..
            } => then
                .find_mana_change(visit)
                .or_else(|| otherwise.and_then(|e| e.find_mana_change(visit))),
            Branches::May(effect)
            | Branches::Unless(effect, _)
            | Branches::ForEachPlayer(effect)
            | Branches::WhenFound(effect) => effect.find_mana_change(visit),
            Branches::Single => None,
        }
    }
}
