//! Checks that apply to one instruction and the operands it reads.

use super::{Checked, Malformed, Scope, both};
use crate::PutOnto;
use crate::ability::AbilityKind;
use crate::continuous::Duration;
use crate::effect::{Amount, Effect, Objects, Operands, Players, Recipients};
use crate::ids::Zone;
use crate::prompt::Shape;

impl Scope<'_> {
    /// One instruction without those it holds: what the check asks of its entry alone, and
    /// of each of its operands.
    #[allow(clippy::too_many_lines)] // Each instruction is classified in one match.
    pub(super) const fn instruction(self, effect: &Effect) -> Checked {
        let entry = match effect {
            Effect::DealDamage {
                to: Recipients::DividedTwo(spec),
                amount,
            } => {
                let targets = self.ability.targets();
                if spec.index() < targets.len()
                    && matches!(amount, Amount::Fixed(2))
                    && targets[spec.index()].required() == 1
                    && targets[spec.index()].count == 2
                    && matches!(
                        targets[spec.index()].kind,
                        crate::TargetKind::Permanent
                            | crate::TargetKind::Player
                            | crate::TargetKind::PermanentOrPlayer
                    )
                {
                    Ok(())
                } else {
                    Err(Malformed::Target)
                }
            }
            Effect::PreventDamage {
                to: Recipients::DividedTwo(_),
                ..
            } => Err(Malformed::Target),
            Effect::CounterInto { zone, .. } => match zone {
                Zone::Exile | Zone::Library => Ok(()),
                Zone::Hand
                | Zone::Battlefield
                | Zone::Graveyard
                | Zone::Stack
                | Zone::Nowhere
                | Zone::Merged => Err(Malformed::Countered),
            },
            Effect::CreateCopies {
                except: Some(ix), ..
            }
            | Effect::BecomeCopy {
                except: Some(ix), ..
            }
            | Effect::CopySpell {
                except: Some(ix), ..
            } if ix.index() >= self.face.excepts.len() => Err(Malformed::Copy),
            Effect::AddMana { .. } | Effect::AddProduct { .. }
                if !matches!(self.ability.kind(), AbilityKind::Mana) =>
            {
                Err(Malformed::AddMana)
            }
            Effect::RestartGame { .. } | Effect::Subgame
                if matches!(
                    self.ability.kind(),
                    AbilityKind::Replacement | AbilityKind::Mana
                ) =>
            {
                Err(Malformed::NewGame)
            }
            Effect::EndTurn if matches!(self.ability.kind(), AbilityKind::Replacement) => {
                Err(Malformed::EndTurn)
            }
            Effect::Modify {
                objects,
                modifications,
                ..
            } => self.resolved_modifications(objects, modifications),
            // One object, picked: the one run of picks the interpreter asks.
            Effect::Choose { prompt, .. }
                if !(matches!(prompt.purpose.shape(), Shape::Pick) && prompt.most == 1) =>
            {
                Err(Malformed::Chosen)
            }
            Effect::Choose {
                hidden: true,
                optional,
                among,
                ..
            } if !*optional
                || !matches!(
                    among,
                    Objects::InZoneOf {
                        zone: Zone::Hand | Zone::Library,
                        ..
                    }
                ) =>
            {
                Err(Malformed::Chosen)
            }
            Effect::MayCast {
                player: Players::EachPlayer | Players::EachOpponent | Players::EachOther(_),
                ..
            } => Err(Malformed::CastPlayer),
            Effect::ExileUntil {
                player: Players::EachPlayer | Players::EachOpponent | Players::EachOther(_),
                ..
            } => Err(Malformed::ExilePlayer),
            Effect::ExileUntil {
                matching: crate::ExileMatch::SharesTypeWithTriggeringSpell,
                ..
            } if !matches!(self.ability.trigger(), Some(trigger) if trigger.event.is_cast()) => {
                Err(Malformed::TriggeredOn)
            }
            Effect::MayCast {
                free: false,
                free_below: Some(_),
                ..
            } => Err(Malformed::CastLimit),
            Effect::PermitCast {
                duration: Duration::UntilYourNextTurn,
                ..
            } => Err(Malformed::Permitted),
            Effect::PutOntoBattlefield(PutOnto {
                under: Some(Players::EachPlayer | Players::EachOpponent | Players::EachOther(_)),
                ..
            }) => Err(Malformed::Under),
            Effect::Fateseal { count, .. } => match count {
                Amount::Fixed(1) => Ok(()),
                Amount::Fixed(_)
                | Amount::X
                | Amount::ThatMany
                | Amount::Carried
                | Amount::ThoseOf(_)
                | Amount::Count(_)
                | Amount::Times(..)
                | Amount::Half { .. }
                | Amount::Plus(..)
                | Amount::CardTypesAmong(_)
                | Amount::Power(_)
                | Amount::Toughness(_)
                | Amount::ManaValue(_)
                | Amount::Counters(..)
                | Amount::Life(_)
                | Amount::Most(..)
                | Amount::SpellsCast(_)
                | Amount::CardsDrawn(_)
                | Amount::SpellsCastBefore => Err(Malformed::Fateseal),
            },
            Effect::Delay {
                ability, object, ..
            } => {
                let abilities = self.face.abilities;
                let delayed = ability.index() < abilities.len()
                    && match abilities[ability.index()].trigger() {
                        Some(trigger) => {
                            trigger.delayed
                                && !trigger.exact_turn
                                && !matches!(trigger.event, crate::TriggerEvent::Reflexive)
                        }
                        None => false,
                    }
                    && !matches!(
                        object,
                        Some(
                            Objects::All { .. }
                                | Objects::InZoneOf { .. }
                                | Objects::CardsIn { .. }
                                | Objects::ExiledWithThis
                                | Objects::LibraryTop { .. }
                                | Objects::ExceptThose(_)
                        )
                    );
                if delayed {
                    Ok(())
                } else {
                    Err(Malformed::Delayed)
                }
            }
            // A reflexive ability is one of the face's own, written with `Trigger::REFLEXIVE`.
            Effect::Reflexive { condition, ability } => {
                let abilities = self.face.abilities;
                let reflexive = ability.index() < abilities.len()
                    && matches!(
                        abilities[ability.index()].trigger(),
                        Some(t) if t.delayed && matches!(t.event, crate::TriggerEvent::Reflexive)
                    );
                if !reflexive {
                    Err(Malformed::Delayed)
                } else if let Some(condition) = condition {
                    self.condition(condition)
                } else {
                    Ok(())
                }
            }
            Effect::ExtraTurn(_, Some(ability)) => {
                let abilities = self.face.abilities;
                if ability.index() < abilities.len()
                    && matches!(abilities[ability.index()].trigger(), Some(t) if t.delayed && t.exact_turn && matches!(t.event, crate::TriggerEvent::Step(_)))
                {
                    Ok(())
                } else {
                    Err(Malformed::Delayed)
                }
            }
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
            | Effect::DoubleCounters { .. }
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
            | Effect::GetEmblem { .. }
            | Effect::WinGame(_)
            | Effect::LoseGame(_)
            | Effect::ControlPlayer(_)
            | Effect::RestartGame { .. }
            | Effect::EndTurn
            | Effect::AdditionalStep(_)
            | Effect::AdditionalPhases { .. }
            | Effect::ExtraTurn(_, None) => Ok(()),
        };
        both(entry, self.operands(effect.operands()))
    }

    const fn operands(self, operands: Operands) -> Checked {
        let players = match operands.players {
            Some(players) => self.players(players),
            None => Ok(()),
        };
        let others = match operands.others {
            Some(players) => self.players(players),
            None => Ok(()),
        };
        let objects = match operands.objects {
            Some(objects) => self.objects(objects),
            None => Ok(()),
        };
        let to = match operands.to {
            Some(objects) => self.objects(objects),
            None => Ok(()),
        };
        let amount = match operands.amount {
            Some(amount) => self.amount(amount),
            None => Ok(()),
        };
        let filter = match operands.filter {
            Some(filter) => self.filter(filter),
            None => Ok(()),
        };
        both(
            both(players, others),
            both(both(objects, to), both(amount, filter)),
        )
    }
}
