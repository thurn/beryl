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
use crate::continuous::{LayerSpec, Modification};
use crate::definition::{CardDef, Face};
use crate::effect::{Amount, Condition, CopyException, Effect, Objects, Players};
use crate::filter::Filter;
use crate::index::SpecIx;
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
    /// [`Effect::AddMana`] outside a mana ability: an ability that adds mana is one
    /// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)), and the engine
    /// produces mana only as it resolves one.
    AddMana,
    /// [`crate::TargetSpec::removal`] or [`crate::TargetSpec::counter`] declared of an
    /// ability whose effect is data, which says what it takes itself
    /// ([`Ability::takes_target`]).
    Declared,
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

    const fn effect(self, effect: &Effect) -> Checked {
        match effect {
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
            Effect::May { player, effect } | Effect::Unless { effect, player, .. } => {
                both(self.players(player), self.effect(effect))
            }
            Effect::ForEachPlayer { players, effect } => {
                let each = Scope {
                    for_a_player: true,
                    ..self
                };
                both(self.players(players), each.effect(effect))
            }
            Effect::Counter(objects)
            | Effect::Destroy(objects)
            | Effect::Exile(objects)
            | Effect::ExileUntilThisLeaves(objects)
            | Effect::Sacrifice(objects)
            | Effect::ReturnToHand(objects)
            | Effect::PutIntoLibrary { objects, .. } => self.objects(objects),
            Effect::Draw { player, count }
            | Effect::Discard { player, count }
            | Effect::Mill { player, count }
            | Effect::Scry { player, count }
            | Effect::GainLife {
                player,
                amount: count,
            }
            | Effect::LoseLife {
                player,
                amount: count,
            }
            | Effect::CreateTokens { player, count, .. } => {
                both(self.players(player), self.amount(count))
            }
            Effect::CreateCopies {
                player,
                of,
                count,
                except,
            } => {
                let except = match except {
                    CopyException::None | CopyException::PowerToughness(1, 1) => Ok(()),
                    CopyException::PowerToughness(..) => Err(Malformed::Copy),
                };
                both(
                    both(self.players(player), self.objects(of)),
                    both(self.amount(count), except),
                )
            }
            Effect::AddCounters { objects, count, .. }
            | Effect::RemoveCounters { objects, count, .. } => {
                both(self.objects(objects), self.amount(count))
            }
            Effect::AddMana { player, .. } => {
                if matches!(self.ability.kind(), AbilityKind::Mana) {
                    self.players(player)
                } else {
                    Err(Malformed::AddMana)
                }
            }
            Effect::Modify {
                objects,
                modification,
                ..
            } => both(self.objects(objects), self.modification(modification)),
            Effect::WinGame(player) | Effect::LoseGame(player) => self.players(player),
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
            Objects::This | Objects::AttachedTo | Objects::Those | Objects::Paid(_) => Ok(()),
            Objects::Target(target) => match self.target(*target) {
                None | Some(TargetKind::Player) => Err(Malformed::Target),
                Some(_) => Ok(()),
            },
            Objects::All { filter, .. } => self.filter(filter),
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
            Players::ThatPlayer if self.for_a_player => Ok(()),
            Players::ThatPlayer => Err(Malformed::ThatPlayer),
        }
    }

    const fn amount(self, amount: &Amount) -> Checked {
        match amount {
            Amount::Fixed(_) | Amount::X | Amount::ThatMany => Ok(()),
            Amount::Count(objects) => self.objects(objects),
            Amount::Times(_, amount) => self.amount(amount),
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
            Condition::Did | Condition::Custom(_) => Ok(()),
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
            Condition::EnteredThisTurn(objects) => self.objects(objects),
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
mod tests {
    use super::*;
    use crate::cost::CastingKind;
    use crate::effect::build::{
        TARGET, TARGET_PLAYER, THAT_PLAYER, YOU, add_mana, destroy, draw, for_each_player, if_then,
    };
    use crate::mana::{ManaColor, ManaSet};
    use crate::target::TargetSpec;

    const A_CREATURE: TargetSpec = TargetSpec::one(TargetKind::Permanent, Filter::Any);
    const A_PLAYER: TargetSpec = TargetSpec::one(TargetKind::Player, Filter::Any);

    /// What the check says of a one-faced card with these abilities.
    fn checked(abilities: &'static [Ability]) -> Checked {
        let face: &'static Face = Box::leak(Box::new(Face::EMPTY.abilities(abilities)));
        CardDef {
            name: "Test",
            faces: std::slice::from_ref(face),
            tokens: &[],
        }
        .check()
    }

    #[test]
    fn a_target_the_ability_does_not_have_is_refused() {
        const NO_TARGET: &[Ability] = &[Ability::spell().effect(&destroy(TARGET))];
        const A_PLAYER_DESTROYED: &[Ability] = &[Ability::spell()
            .targeting(&[A_PLAYER])
            .effect(&destroy(TARGET))];
        const A_CREATURE_DRAWS: &[Ability] = &[Ability::spell()
            .targeting(&[A_CREATURE])
            .effect(&draw(TARGET_PLAYER, 1))];
        const WELL_FORMED: &[Ability] = &[Ability::spell()
            .targeting(&[A_CREATURE, A_PLAYER])
            .effect(&Effect::Sequence(&[
                destroy(TARGET),
                draw(Players::Target(SpecIx::new(1)), 1),
            ]))];
        assert_eq!(checked(NO_TARGET), Err(Malformed::Target));
        assert_eq!(checked(A_PLAYER_DESTROYED), Err(Malformed::Target));
        assert_eq!(checked(A_CREATURE_DRAWS), Err(Malformed::Target));
        assert_eq!(checked(WELL_FORMED), Ok(()));
    }

    #[test]
    fn that_player_outside_a_for_each_player_is_refused() {
        const OUTSIDE: &[Ability] = &[Ability::spell().effect(&draw(THAT_PLAYER, 1))];
        const INSIDE: &[Ability] =
            &[Ability::spell()
                .effect(&for_each_player(Players::EachPlayer, &draw(THAT_PLAYER, 1)))];
        assert_eq!(checked(OUTSIDE), Err(Malformed::ThatPlayer));
        assert_eq!(checked(INSIDE), Ok(()));
    }

    #[test]
    fn a_cost_the_face_cannot_be_cast_with_is_refused() {
        const KICKED: &[Ability] = &[Ability::spell().effect(&if_then(
            Condition::Paid(CastingKind::Kicker),
            &draw(YOU, 1),
        ))];
        // Offspring's own ability, on a face that does not list its cost.
        const OFFSPRING: &[Ability] = &[Ability::offspring()];
        assert_eq!(checked(KICKED), Err(Malformed::Paid));
        assert_eq!(checked(OFFSPRING), Err(Malformed::Paid));
    }

    #[test]
    fn a_grant_the_face_does_not_list_is_refused() {
        const GRANTS: &[Ability] = &[Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modification: Modification::grant(0),
        })];
        assert_eq!(checked(GRANTS), Err(Malformed::Grant));
    }

    #[test]
    fn a_copy_that_is_not_one_one_is_refused() {
        const COPY: &[Ability] = &[Ability::spell().effect(&Effect::CreateCopies {
            player: YOU,
            of: Objects::This,
            count: Amount::Fixed(1),
            except: CopyException::PowerToughness(2, 2),
        })];
        assert_eq!(checked(COPY), Err(Malformed::Copy));
    }

    #[test]
    fn mana_added_outside_a_mana_ability_is_refused() {
        const RITUAL: &[Ability] =
            &[Ability::spell().effect(&add_mana(YOU, ManaSet::one(ManaColor::Black)))];
        const LAND: &[Ability] = &[Ability::tap_for_mana(ManaSet::one(ManaColor::Black))];
        assert_eq!(checked(RITUAL), Err(Malformed::AddMana));
        assert_eq!(checked(LAND), Ok(()));
    }

    #[test]
    fn effect_data_declares_nothing_of_its_targets() {
        const DECLARED: &[Ability] = &[Ability::spell()
            .targeting(&[A_CREATURE.removal()])
            .effect(&destroy(TARGET))];
        assert_eq!(checked(DECLARED), Err(Malformed::Declared));
    }

    /// In a card's `static` this panic is the build failure.
    #[test]
    #[should_panic(expected = "an effect adds mana outside a mana ability")]
    fn a_card_is_checked_as_it_is_built() {
        const RITUAL: &[Ability] =
            &[Ability::spell().effect(&add_mana(YOU, ManaSet::one(ManaColor::Black)))];
        let face: &'static Face = Box::leak(Box::new(Face::EMPTY.abilities(RITUAL)));
        let _ = crate::card("Test Ritual", std::slice::from_ref(face));
    }
}
