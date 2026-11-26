//! The definition check: what a definition can say that no game could carry out, refused as
//! the definition compiles.
//!
//! Every type of the vocabulary is plain data, so a card file can write an effect that names
//! a target its ability does not have, or asks whether a cost was paid that its face does not
//! offer. The interpreter would do nothing with it, silently, or fail when the card is first
//! played. [`CardDef::check`] walks a definition for each such mistake ([`Malformed`]), and
//! [`card`](crate::card) runs it, so in a card's `static` one is a compile error in the
//! card's own file.
//!
//! The check walks a definition on its own instead of through [`CardDef::walk`], whose
//! visitor is a closure that a `const fn` cannot call. Both read an instruction's
//! expressions through [`Effect::operands`] and the instructions it holds through
//! `Effect::branches`, so a new vocabulary entry is described once for both.

use crate::ability::{Ability, Behavior};
use crate::continuous::{LayerSpec, Modification, Rewrite};
use crate::cost::{Chosen, Cost, CostAction, CostPart, Measure};
use crate::definition::{CardDef, Face, FaceUse, Shape};
use crate::effect::{Amount, Branches, Condition, CopyChange, Effect, Objects, Players};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::SpecIx;
use crate::target::TargetKind;
use crate::trigger::{Subject, Trigger, TriggerEvent};

mod instruction;
mod malformed;
mod modifications;
mod movement;
pub use malformed::Malformed;
use movement::{Moved, moved_by};

type Checked = Result<(), Malformed>;

/// Whether every one of `modifications` changes only card types, subtypes or colors (layers
/// 4 and 5), or grants escape, which works only from a graveyard: the changes an effect on
/// objects in every zone may make ([`Malformed::Everywhere`]).
const fn reaches_every_zone(modifications: &[Modification]) -> bool {
    let mut i = 0;
    while i < modifications.len() {
        if !matches!(
            modifications[i],
            Modification::AddTypes(_)
                | Modification::RemoveTypes(_)
                | Modification::SetTypes(_)
                | Modification::AddSubtypeWord(_)
                | Modification::SetSubtypeWord(_)
                | Modification::SetColorWords(_)
                | Modification::AddColorWords(_)
                | Modification::AddChosenColor
                | Modification::GrantEscape
        ) {
            return false;
        }
        i += 1;
    }
    true
}

/// Both, the first failure first. `?` is not for a `const fn`.
const fn both(first: Checked, second: Checked) -> Checked {
    match first {
        Ok(()) => second,
        Err(malformed) => Err(malformed),
    }
}

impl CardDef {
    /// Whether every face says only what a game can carry out.
    ///
    /// # Errors
    ///
    /// The first mistake found ([`Malformed`]).
    pub const fn check(&self) -> Checked {
        // A prototype card's definition supplies its prototyped face after its one printed
        // face ([`Face::prototyped`]).
        let prototype = matches!(self.shape, Shape::Single)
            && !self.faces.is_empty()
            && self.faces[0].has_prototype();
        let expected = self.shape.characteristics_count() + prototype as usize;
        if self.faces.len() != expected {
            return Err(match expected {
                1 => Malformed::ExpectedOneFace,
                2 => Malformed::ExpectedTwoFaces,
                _ => Malformed::ExpectedFusedFace,
            });
        }
        let mut i = 0;
        while i < self.faces.len() {
            let face = &self.faces[i];
            let expected_use = if prototype && i == 1 {
                FaceUse::Prototype
            } else if i == 2 {
                FaceUse::Fused
            } else {
                FaceUse::of(self.shape, i == 0)
            };
            if face.use_of_face as u8 != expected_use as u8 {
                return Err(Malformed::FaceUse);
            }
            if let Err(malformed) = both(
                both(
                    check_abilities(face, face.abilities),
                    check_abilities(face, face.grants),
                ),
                check_excepts(face),
            ) {
                return Err(malformed);
            }
            i += 1;
        }
        Ok(())
    }
}

/// Whether each copy exception of `face` that gives an ability names one the face lists in
/// [`Face::grants`] ([`CopyChange::Has`]).
const fn check_excepts(face: &Face) -> Checked {
    let mut i = 0;
    while i < face.excepts.len() {
        let changes = face.excepts[i].0;
        let mut j = 0;
        while j < changes.len() {
            if let CopyChange::Has(ix) = changes[j]
                && ix.index() >= face.grants.len()
            {
                return Err(Malformed::Copy);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

const fn check_abilities(face: &Face, abilities: &[Ability]) -> Checked {
    let mut i = 0;
    while i < abilities.len() {
        let scope = Scope {
            face,
            ability: &abilities[i],
            for_a_player: abilities[i].replaces_draws(),
            moved: Moved::NONE,
        };
        if let Err(malformed) = scope.ability() {
            return Err(malformed);
        }
        i += 1;
    }
    Ok(())
}

/// Whether a cost asks for no mana. [`crate::ManaCost::is_zero`] is not for a `const fn`.
const fn no_mana(cost: &Cost) -> bool {
    let mana = &cost.mana;
    let mut symbols = 0;
    let mut i = 0;
    while i < mana.symbols.len() {
        symbols += mana.symbols[i] as u16;
        i += 1;
    }
    mana.generic == 0 && mana.x == 0 && symbols == 0 && mana.hybrid.is_empty()
}

/// What the check asks of an ability's effect as a whole.
const fn whole(effect: &Effect) -> Checked {
    if rule_changes(effect) > 1 {
        Err(Malformed::Rules)
    } else {
        Ok(())
    }
}

/// How many of `effect`'s instructions start a rule change ([`Effect::ChangeRule`]).
const fn rule_changes(effect: &Effect) -> usize {
    match effect.branches() {
        Branches::Sequence(effects) => {
            let mut count = 0;
            let mut i = 0;
            while i < effects.len() {
                count += rule_changes(&effects[i]);
                i += 1;
            }
            count
        }
        Branches::If {
            then, otherwise, ..
        } => {
            rule_changes(then)
                + match otherwise {
                    Some(otherwise) => rule_changes(otherwise),
                    None => 0,
                }
        }
        Branches::May(effect)
        | Branches::Unless(effect, _)
        | Branches::ForEachPlayer(effect)
        | Branches::WhenFound(effect) => rule_changes(effect),
        Branches::Single => {
            if effect.started_rule().is_some() {
                1
            } else {
                0
            }
        }
    }
}

/// Where an expression is read: the ability it is part of, on its face.
#[derive(Clone, Copy)]
struct Scope<'a> {
    face: &'a Face,
    ability: &'a Ability,
    /// Within an [`Effect::ForEachPlayer`].
    for_a_player: bool,
    /// What the instructions before this one may have moved.
    moved: Moved,
}

impl Scope<'_> {
    const fn ability(self) -> Checked {
        let condition = match self.ability.condition() {
            Some(condition) => Scope {
                for_a_player: false,
                ..self
            }
            .condition(condition),
            None => Ok(()),
        };
        let layer = match self.ability.layer() {
            Some(layer) => self.layer(layer),
            None => Ok(()),
        };
        let body = match self.ability.body() {
            Behavior::Data(effect) => {
                let instructions = match self.effect(effect) {
                    Ok(_) => Ok(()),
                    Err(malformed) => Err(malformed),
                };
                both(instructions, whole(effect))
            }
            Behavior::Custom(_) => Ok(()),
        };
        let copy = match self.ability.replacement() {
            Some(replacement) => match replacement.rewrite {
                Rewrite::EnterAsCopy {
                    except: Some(ix), ..
                } if ix.index() >= self.face.excepts.len() => Err(Malformed::Copy),
                Rewrite::None
                | Rewrite::ToZone(_)
                | Rewrite::EntersTapped
                | Rewrite::EntersPrepared
                | Rewrite::EntersTappedUnlessFewLands(_)
                | Rewrite::EntersTappedUnlessPayLife(_)
                | Rewrite::EntersWithCounters(..)
                | Rewrite::Instead
                | Rewrite::Dredge(_)
                | Rewrite::Choose(_)
                | Rewrite::MaySkipTurnToUntap
                | Rewrite::Multiply(_)
                | Rewrite::EnterAsCopy { .. } => Ok(()),
            },
            None => Ok(()),
        };
        both(
            both(both(condition, self.ability_targets()), both(layer, body)),
            copy,
        )
    }

    /// Whether each target of an ability on the stack has no filter: what a filter says is
    /// about objects and players, and no rule reads it of an ability.
    const fn ability_targets(self) -> Checked {
        let targets = self.ability.targets();
        let mut i = 0;
        while i < targets.len() {
            if targets[i].required() > targets[i].count
                || (targets[i].kind.abilities().is_some()
                    && !matches!(targets[i].filter, Filter::Any))
            {
                return Err(Malformed::Target);
            }
            i += 1;
        }
        Ok(())
    }

    const fn layer(self, layer: &LayerSpec) -> Checked {
        let condition = match &layer.condition {
            Some(condition) => self.condition(condition),
            None => Ok(()),
        };
        let mut modifications = if layer.modifications.is_empty() {
            Err(Malformed::NoModification)
        } else {
            Ok(())
        };
        let mut i = 0;
        while i < layer.modifications.len() {
            let modification = &layer.modifications[i];
            let checked = match modification {
                Modification::LoseAbility(ability) => {
                    if matches!(layer.affected, Objects::This)
                        && ability.index() < self.face.abilities.len()
                    {
                        Ok(())
                    } else {
                        Err(Malformed::Lost)
                    }
                }
                modification @ (Modification::Control
                | Modification::AddTypes(_)
                | Modification::RemoveTypes(_)
                | Modification::SetTypes(_)
                | Modification::AddSubtypeWord(_)
                | Modification::SetSubtypeWord(_)
                | Modification::SetLandTypeWords(_)
                | Modification::AddKeywords(_)
                | Modification::Grant(_)
                | Modification::SetPower(_)
                | Modification::SetPt(..)
                | Modification::ModifyPt(..)
                | Modification::LoseAllAbilities
                | Modification::SetColorWords(_)
                | Modification::AddColorWords(_)
                | Modification::AddChosenColor
                | Modification::SetBasePt(..)
                | Modification::SwitchPt
                | Modification::AddProtection(_)
                | Modification::GrantEscape) => self.modification(modification),
            };
            modifications = both(modifications, checked);
            i += 1;
        }
        let affected = match &layer.affected {
            Objects::Everywhere(filter) => {
                if layer.condition.is_none() && reaches_every_zone(layer.modifications) {
                    self.filter(filter)
                } else {
                    Err(Malformed::Everywhere)
                }
            }
            affected @ (Objects::This
            | Objects::Target(_)
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
            | Objects::Paid(_)
            | Objects::Other(_)) => self.objects(affected),
        };
        both(affected, both(condition, modifications))
    }

    /// Instructions in the order written (CR 608.2c): each is read after what those before
    /// it moved, and what they have moved by the end is the answer.
    const fn effects(self, effects: &[Effect]) -> Result<Moved, Malformed> {
        let mut scope = self;
        let mut i = 0;
        while i < effects.len() {
            match scope.effect(&effects[i]) {
                Ok(moved) => scope = Scope { moved, ..scope },
                Err(malformed) => return Err(malformed),
            }
            i += 1;
        }
        Ok(scope.moved)
    }

    /// An instruction: itself, and then the instructions it holds. What it may have moved
    /// by its end, with what was moved before it.
    const fn effect(self, effect: &Effect) -> Result<Moved, Malformed> {
        if let Err(malformed) = self.instruction(effect) {
            return Err(malformed);
        }
        match effect.branches() {
            Branches::Sequence(effects) => self.effects(effects),
            Branches::If {
                condition,
                then,
                otherwise,
            } => {
                if let Err(malformed) = self.condition(condition) {
                    return Err(malformed);
                }
                let then = match self.effect(then) {
                    Ok(moved) => moved,
                    Err(malformed) => return Err(malformed),
                };
                match otherwise {
                    Some(otherwise) => match self.effect(otherwise) {
                        Ok(moved) => Ok(then.or(moved)),
                        Err(malformed) => Err(malformed),
                    },
                    None => Ok(then),
                }
            }
            Branches::May(effect) | Branches::WhenFound(effect) => self.effect(effect),
            Branches::Unless(effect, cost) => {
                if let Err(malformed) = self.resolution_cost(cost) {
                    return Err(malformed);
                }
                self.effect(effect)
            }
            // Read a second time after what one player's turn at it moved, as the next
            // player's is.
            Branches::ForEachPlayer(effect) => {
                let each = Scope {
                    for_a_player: true,
                    ..self
                };
                match each.effect(effect) {
                    Ok(moved) => Scope { moved, ..each }.effect(effect),
                    Err(malformed) => Err(malformed),
                }
            }
            Branches::Single => Ok(match moved_by(effect) {
                Some(objects) => self.moved.with(objects),
                None => self.moved,
            }),
        }
    }

    /// A cost paid as an effect resolves ([`Effect::Unless`]): mana alone, or the discard
    /// of one card from the player's hand that a filter accepts.
    const fn resolution_cost(self, cost: &Cost) -> Checked {
        match cost.parts() {
            [] => Ok(()),
            [
                CostPart::Chosen(
                    CostAction::Discard,
                    Chosen {
                        zone: Zone::Hand,
                        filter,
                        other: false,
                        measure: Measure::Count(1),
                    },
                ),
            ] if no_mana(cost) => self.filter(filter),
            _ => Err(Malformed::Cost),
        }
    }

    /// The kind of thing the ability's `target`th target is, if it has one.
    const fn target(self, target: SpecIx) -> Option<TargetKind> {
        let targets = self.ability.targets();
        if target.index() < targets.len() {
            Some(targets[target.index()].kind)
        } else {
            None
        }
    }

    /// Whether no instruction before has moved `objects`: what it was is then asked of it
    /// where it is.
    const fn unmoved(self, objects: &Objects) -> Checked {
        if self.moved.has(objects) {
            Err(Malformed::LastKnown)
        } else {
            Ok(())
        }
    }

    /// Whether its controller, its owner and its numbers can be read: of a target they are
    /// kept through a move (`beryl_rules`' `Known`), and of anything else only while no
    /// instruction before has moved it.
    const fn kept(self, objects: &Objects) -> Checked {
        match objects {
            Objects::Target(_) => Ok(()),
            Objects::This
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
            | Objects::Other(_) => self.unmoved(objects),
        }
    }

    const fn objects(self, objects: &Objects) -> Checked {
        match objects {
            Objects::This
            | Objects::AttachedTo
            | Objects::Those
            | Objects::Paid(_)
            | Objects::ExiledWithThis
            | Objects::PutOntoBattlefieldWithThis => Ok(()),
            Objects::TriggeredOn | Objects::TriggeredOnIn(_) => match self.ability.trigger() {
                Some(_) => Ok(()),
                None => Err(Malformed::TriggeredOn),
            },
            Objects::Carried => match self.ability.trigger() {
                Some(trigger) if trigger.delayed => Ok(()),
                _ => Err(Malformed::Carried),
            },
            Objects::Target(target) => match self.target(*target) {
                None | Some(TargetKind::Player) => Err(Malformed::Target),
                Some(_) => Ok(()),
            },
            Objects::All { filter, .. } => self.filter(filter),
            Objects::InZoneOf {
                zone,
                players,
                filter,
            } => match zone {
                Zone::Battlefield | Zone::Library | Zone::Hand | Zone::Graveyard => {
                    both(self.players(players), self.filter(filter))
                }
                Zone::Exile | Zone::Stack | Zone::Nowhere | Zone::Merged => Err(Malformed::ZoneOf),
            },
            Objects::CardsIn { players, .. } | Objects::LibraryTop { players, .. } => {
                self.players(players)
            }
            Objects::Everywhere(_) => Err(Malformed::Everywhere),
            Objects::Other(objects) | Objects::ExceptThose(objects) => self.objects(objects),
        }
    }

    const fn players(self, players: &Players) -> Checked {
        match players {
            Players::You
            | Players::EachOpponent
            | Players::EachPlayer
            | Players::Enchanted
            | Players::SubgameNonwinners => Ok(()),
            Players::Target(target) => match self.target(*target) {
                Some(TargetKind::Player | TargetKind::PermanentOrPlayer) => Ok(()),
                Some(
                    TargetKind::Permanent
                    | TargetKind::Spell
                    | TargetKind::SpellOrPermanent
                    | TargetKind::GraveyardCard
                    | TargetKind::Ability
                    | TargetKind::ActivatedAbility
                    | TargetKind::TriggeredAbility,
                )
                | None => Err(Malformed::Target),
            },
            Players::ControllerOf(objects) | Players::OwnerOf(objects) => {
                both(self.kept(objects), self.objects(objects))
            }
            Players::EachOther(players) => self.players(players),
            Players::ThatPlayer if self.for_a_player => Ok(()),
            Players::ThatPlayer => match self.ability.trigger() {
                Some(trigger) if trigger.event.is_cast() => Ok(()),
                _ => Err(Malformed::ThatPlayer),
            },
        }
    }

    const fn amount(self, amount: &Amount) -> Checked {
        match amount {
            Amount::Fixed(_) | Amount::X | Amount::ThatMany => Ok(()),
            Amount::Carried => match self.ability.trigger() {
                Some(trigger) if trigger.delayed => Ok(()),
                Some(Trigger {
                    event: TriggerEvent::DealsDamage,
                    ..
                }) => Ok(()),
                _ => Err(Malformed::Carried),
            },
            Amount::SpellsCastBefore => match self.ability.trigger() {
                Some(Trigger {
                    event: TriggerEvent::Casts,
                    subject: Subject::This,
                    ..
                }) => Ok(()),
                _ => Err(Malformed::CastBefore),
            },
            Amount::Count(objects) | Amount::CardTypesAmong(objects) => self.objects(objects),
            Amount::Times(_, amount)
            | Amount::Plus(_, amount)
            | Amount::Half { of: amount, .. } => self.amount(amount),
            Amount::Life(players)
            | Amount::ThoseOf(players)
            | Amount::SpellsCast(players)
            | Amount::CardsDrawn(players) => self.players(players),
            Amount::Most(players, amount) => both(self.players(players), self.amount(amount)),
            Amount::Power(objects)
            | Amount::Toughness(objects)
            | Amount::ManaValue(objects)
            | Amount::Counters(objects, _) => both(self.kept(objects), self.objects(objects)),
        }
    }

    const fn conditions(self, conditions: &[Condition]) -> Checked {
        let mut i = 0;
        while i < conditions.len() {
            if let Err(malformed) = self.condition(&conditions[i]) {
                return Err(malformed);
            }
            i += 1;
        }
        Ok(())
    }

    const fn condition(self, condition: &Condition) -> Checked {
        match condition {
            Condition::Did | Condition::WonFlip => Ok(()),
            Condition::DrewInDrawStep(players) => self.players(players),
            Condition::Paid(kind) => {
                let casting = self.face.casting;
                let mut i = 0;
                while i < casting.len() {
                    if let Some(paid) = casting[i].kind()
                        && paid.index() == kind.index()
                    {
                        return Ok(());
                    }
                    i += 1;
                }
                Err(Malformed::Paid)
            }
            Condition::Compare(left, _, right) => both(self.amount(left), self.amount(right)),
            Condition::Is(objects, filter) => both(
                self.unmoved(objects),
                both(self.objects(objects), self.filter(filter)),
            ),
            Condition::EnteredThisTurn(objects)
            | Condition::Token(objects)
            | Condition::OnBattlefield(objects)
            | Condition::Cast(objects) => both(self.unmoved(objects), self.objects(objects)),
            // What a resolution moved is read where it went (CR 701.17c).
            Condition::HasChosenName(objects) => self.objects(objects),
            Condition::MeldPair(partner) => self.filter(partner),
            Condition::Not(condition) => self.condition(condition),
            Condition::All(conditions) | Condition::Any(conditions) => self.conditions(conditions),
        }
    }

    const fn filters(self, filters: &[Filter]) -> Checked {
        let mut i = 0;
        while i < filters.len() {
            if let Err(malformed) = self.filter(&filters[i]) {
                return Err(malformed);
            }
            i += 1;
        }
        Ok(())
    }

    const fn filter(self, filter: &Filter) -> Checked {
        match filter {
            Filter::Whose(players) | Filter::Owned(players) => self.players(players),
            Filter::Not(filter) => self.filter(filter),
            Filter::All(filters) | Filter::AnyOf(filters) => self.filters(filters),
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::SubtypeWord(_)
            | Filter::ColorWords(_)
            | Filter::AttackedThisTurn
            | Filter::Tapped
            | Filter::Token
            | Filter::Named(_)
            | Filter::ManaValueAtMost(_)
            | Filter::PowerAtMost(_)
            | Filter::ManaValueAtMostX
            | Filter::ManaValueAtMostPower
            | Filter::InZone(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
