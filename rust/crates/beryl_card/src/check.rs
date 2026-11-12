//! The definition check: what a definition can say that no game could carry out, refused as
//! the definition compiles.
//!
//! Every type of the vocabulary is plain data, so a card file can write an effect that names
//! a target its ability does not have, or asks whether a cost was paid that its face does not
//! offer. The interpreter would do nothing with it, silently, or fail when the card is first
//! played. [`CardDef::check`] walks a definition for each such mistake ([`Malformed`]), and
//! [`card`](crate::card) runs it, so in a card's `static` one is a compile error in the
//! card's own file.

use crate::ability::{Ability, AbilityKind, Behavior};
use crate::continuous::{Duration, LayerSpec, Modification};
use crate::cost::{Chosen, Cost, CostAction, CostPart, Measure};
use crate::definition::{CardDef, Face};
use crate::effect::{Amount, Condition, CopyException, Effect, Objects, Operands, Players};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::SpecIx;
use crate::prompt::Shape;
use crate::target::TargetKind;

/// What is wrong with a definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Malformed {
    /// [`Objects::Target`] or [`Players::Target`] of a target its ability does not have, or
    /// of the other kind: an object where the ability targets a player, or a player where it
    /// targets an object ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
    Target,
    /// [`Players::ThatPlayer`] outside an [`Effect::ForEachPlayer`]: there is no player the
    /// instruction is being done for.
    ThatPlayer,
    /// [`Condition::Paid`] of a kind of cost its face has no [`crate::CastingOption`] for:
    /// it could never have been paid.
    Paid,
    /// [`Modification::Grant`] of an ability past the end of its face's [`Face::grants`].
    Grant,
    /// [`CopyException::PowerToughness`] other than 1/1, which is all the engine's copy
    /// makes.
    Copy,
    /// [`Effect::AddMana`] outside a mana ability, whose activator chooses the kind
    /// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)): any other ability
    /// names the kind it adds ([`Effect::AddManaOfKind`]).
    AddMana,
    /// [`crate::TargetSpec::removal`] or [`crate::TargetSpec::counter`] declared of an
    /// ability whose effect is data, which says what it takes itself
    /// ([`Ability::takes_target`]).
    Declared,
    /// [`Effect::CounterInto`] a zone other than exile, which is the one place the engine
    /// puts a countered spell instead of its owner's graveyard.
    Countered,
    /// [`Effect::Delay`] of an ability its face does not have, or of one that is not a
    /// delayed triggered ability ([`crate::Trigger::next`]): nothing would wait for it.
    Delayed,
    /// [`Amount::Carried`] in an ability that is not a delayed triggered ability: no effect
    /// made it, so it carries nothing.
    Carried,
    /// [`Objects::TriggeredOn`] in an ability that is not triggered: no event was about
    /// anything.
    TriggeredOn,
    /// [`Effect::Choose`] with a prompt that is not a pick of one object, which is the one
    /// choice the interpreter asks.
    Chosen,
    /// [`Objects::InZoneOf`] a zone the players share: each has a library, a hand and a
    /// graveyard of their own, and no other
    /// ([CR 400.1](https://yawgatog.com/resources/magic-rules/#R4001)).
    ZoneOf,
    /// [`Effect::PermitCast`] for a duration other than this turn or for as long as the card
    /// stays where it is, which are the two a permission lasts.
    Permitted,
    /// [`Effect::Unless`] of a cost the engine cannot take as an effect resolves: one other
    /// than mana alone or the discard of one card from a hand.
    Cost,
    /// [`Effect::Fateseal`] of a number of cards other than one, which is all the engine
    /// fateseals.
    Fateseal,
}

impl Malformed {
    /// What is wrong, as the build failure says it.
    pub const fn says(self) -> &'static str {
        match self {
            Malformed::Target => {
                "an effect names a target its ability does not have, or one of the other kind"
            }
            Malformed::ThatPlayer => "\"that player\" is said outside a for-each-player effect",
            Malformed::Paid => "a condition asks about a cost its face cannot be cast with",
            Malformed::Grant => "a modification grants an ability its face does not list",
            Malformed::Copy => "a copy's exception is a power and toughness other than 1/1",
            Malformed::AddMana => "an effect adds mana outside a mana ability",
            Malformed::Declared => {
                "a target of effect data declares what it takes: the data says so itself"
            }
            Malformed::Countered => "a countered spell is put somewhere other than exile",
            Malformed::Cost => "an effect names a cost the engine cannot take as it resolves",
            Malformed::Fateseal => "an effect fateseals a number of cards other than one",
            Malformed::Delayed => {
                "an effect delays an ability that is not a delayed trigger of its face"
            }
            Malformed::Carried => {
                "an amount is what a delayed trigger carries, and its ability is not one"
            }
            Malformed::TriggeredOn => {
                "an effect names what its trigger event was about, and its ability has none"
            }
            Malformed::Chosen => "a choice among objects is not a pick of one of them",
            Malformed::ZoneOf => "a zone of a player's own is not a library, hand or graveyard",
            Malformed::Permitted => {
                "a permission to cast lasts neither this turn nor while the card stays put"
            }
        }
    }
}

type Checked = Result<(), Malformed>;

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
        let mut i = 0;
        while i < self.faces.len() {
            let face = &self.faces[i];
            if let Err(malformed) = both(
                check_abilities(face, face.abilities),
                check_abilities(face, face.grants),
            ) {
                return Err(malformed);
            }
            i += 1;
        }
        Ok(())
    }
}

const fn check_abilities(face: &Face, abilities: &[Ability]) -> Checked {
    let mut i = 0;
    while i < abilities.len() {
        let scope = Scope {
            face,
            ability: &abilities[i],
            for_a_player: false,
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
    mana.generic == 0 && mana.x == 0 && symbols == 0
}

/// Where an expression is read: the ability it is part of, on its face.
#[derive(Clone, Copy)]
struct Scope<'a> {
    face: &'a Face,
    ability: &'a Ability,
    /// Within an [`Effect::ForEachPlayer`].
    for_a_player: bool,
}

impl Scope<'_> {
    const fn ability(self) -> Checked {
        let condition = match self.ability.condition() {
            Some(condition) => self.condition(condition),
            None => Ok(()),
        };
        let layer = match self.ability.layer() {
            Some(layer) => self.layer(layer),
            None => Ok(()),
        };
        let body = match self.ability.body() {
            Behavior::Data(effect) => both(self.declares_nothing(), self.effect(effect)),
            Behavior::Custom(_) => Ok(()),
        };
        both(condition, both(layer, body))
    }

    const fn declares_nothing(self) -> Checked {
        let targets = self.ability.targets();
        let mut i = 0;
        while i < targets.len() {
            if targets[i].takes.is_some() {
                return Err(Malformed::Declared);
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
        both(
            self.objects(&layer.affected),
            both(condition, self.modification(&layer.modification)),
        )
    }

    const fn effects(self, effects: &[Effect]) -> Checked {
        let mut i = 0;
        while i < effects.len() {
            if let Err(malformed) = self.effect(&effects[i]) {
                return Err(malformed);
            }
            i += 1;
        }
        Ok(())
    }

    /// An instruction: itself, and then the instructions it holds.
    const fn effect(self, effect: &Effect) -> Checked {
        let held = match effect {
            Effect::Sequence(effects) => self.effects(effects),
            Effect::If {
                condition,
                then,
                otherwise,
            } => {
                let otherwise = match otherwise {
                    Some(otherwise) => self.effect(otherwise),
                    None => Ok(()),
                };
                both(
                    self.condition(condition),
                    both(self.effect(then), otherwise),
                )
            }
            Effect::May { effect, .. } => self.effect(effect),
            Effect::Unless { effect, cost, .. } => {
                both(self.resolution_cost(cost), self.effect(effect))
            }
            Effect::ForEachPlayer { effect, .. } => {
                let each = Scope {
                    for_a_player: true,
                    ..self
                };
                each.effect(effect)
            }
            _ => Ok(()),
        };
        both(self.instruction(effect), held)
    }

    /// One instruction without those it holds: what the check asks of its entry alone, and
    /// of each of its operands.
    const fn instruction(self, effect: &Effect) -> Checked {
        let entry = match effect {
            Effect::CounterInto { zone, .. } => match zone {
                Zone::Exile => Ok(()),
                _ => Err(Malformed::Countered),
            },
            Effect::CreateCopies { except, .. } => match except {
                CopyException::None | CopyException::PowerToughness(1, 1) => Ok(()),
                CopyException::PowerToughness(..) => Err(Malformed::Copy),
            },
            Effect::AddMana { .. } if !matches!(self.ability.kind(), AbilityKind::Mana) => {
                Err(Malformed::AddMana)
            }
            Effect::Modify { modification, .. } => self.modification(modification),
            // One object, picked: the one run of picks the interpreter asks.
            Effect::Choose { prompt, .. }
                if !(matches!(prompt.purpose.shape(), Shape::Pick) && prompt.most == 1) =>
            {
                Err(Malformed::Chosen)
            }
            Effect::PermitCast {
                duration: Duration::UntilYourNextTurn,
                ..
            } => Err(Malformed::Permitted),
            Effect::Fateseal { count, .. } => match count {
                Amount::Fixed(1) => Ok(()),
                _ => Err(Malformed::Fateseal),
            },
            Effect::Delay { ability, .. } => {
                let abilities = self.face.abilities;
                let delayed = ability.index() < abilities.len()
                    && match abilities[ability.index()].trigger() {
                        Some(trigger) => trigger.delayed,
                        None => false,
                    };
                if delayed {
                    Ok(())
                } else {
                    Err(Malformed::Delayed)
                }
            }
            _ => Ok(()),
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
        let amount = match operands.amount {
            Some(amount) => self.amount(amount),
            None => Ok(()),
        };
        let filter = match operands.filter {
            Some(filter) => self.filter(filter),
            None => Ok(()),
        };
        both(both(players, others), both(objects, both(amount, filter)))
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

    const fn modification(self, modification: &Modification) -> Checked {
        match modification {
            Modification::Grant(grant) if grant.index() >= self.face.grants.len() => {
                Err(Malformed::Grant)
            }
            Modification::SetPower(amount) => self.amount(amount),
            _ => Ok(()),
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

    const fn objects(self, objects: &Objects) -> Checked {
        match objects {
            Objects::This
            | Objects::AttachedTo
            | Objects::Those
            | Objects::Paid(_)
            | Objects::ExiledWithThis => Ok(()),
            Objects::TriggeredOn => match self.ability.trigger() {
                Some(_) => Ok(()),
                None => Err(Malformed::TriggeredOn),
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
                Zone::Library | Zone::Hand | Zone::Graveyard => {
                    both(self.players(players), self.filter(filter))
                }
                _ => Err(Malformed::ZoneOf),
            },
            Objects::CardsIn { players, .. } => self.players(players),
        }
    }

    const fn players(self, players: &Players) -> Checked {
        match players {
            Players::You | Players::EachOpponent | Players::EachPlayer => Ok(()),
            Players::Target(target) => match self.target(*target) {
                Some(TargetKind::Player) => Ok(()),
                _ => Err(Malformed::Target),
            },
            Players::ControllerOf(objects) | Players::OwnerOf(objects) => self.objects(objects),
            Players::EachOther(players) => self.players(players),
            Players::ThatPlayer if self.for_a_player => Ok(()),
            Players::ThatPlayer => Err(Malformed::ThatPlayer),
        }
    }

    const fn amount(self, amount: &Amount) -> Checked {
        match amount {
            Amount::Fixed(_) | Amount::X | Amount::ThatMany => Ok(()),
            Amount::Carried => match self.ability.trigger() {
                Some(trigger) if trigger.delayed => Ok(()),
                _ => Err(Malformed::Carried),
            },
            Amount::Count(objects) => self.objects(objects),
            Amount::Times(_, amount) => self.amount(amount),
            Amount::Life(players) | Amount::ThoseOf(players) => self.players(players),
            Amount::Most(players, amount) => both(self.players(players), self.amount(amount)),
            Amount::Power(objects)
            | Amount::Toughness(objects)
            | Amount::ManaValue(objects)
            | Amount::Counters(objects, _) => self.objects(objects),
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
            Condition::Did | Condition::WonFlip | Condition::Custom(_) => Ok(()),
            Condition::Paid(kind) => {
                let casting = self.face.casting;
                let mut i = 0;
                while i < casting.len() {
                    if casting[i].kind() as u8 == *kind as u8 {
                        return Ok(());
                    }
                    i += 1;
                }
                Err(Malformed::Paid)
            }
            Condition::Compare(left, _, right) => both(self.amount(left), self.amount(right)),
            Condition::Is(objects, filter) => both(self.objects(objects), self.filter(filter)),
            Condition::EnteredThisTurn(objects) | Condition::Token(objects) => {
                self.objects(objects)
            }
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
            Filter::Whose(players) => self.players(players),
            Filter::Not(filter) => self.filter(filter),
            Filter::All(filters) | Filter::AnyOf(filters) => self.filters(filters),
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::Subtype(_)
            | Filter::Colors(_)
            | Filter::Tapped
            | Filter::ManaValueAtMost(_)
            | Filter::ManaValueAtMostX => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
