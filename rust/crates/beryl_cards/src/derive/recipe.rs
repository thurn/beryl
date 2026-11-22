//! Starting positions and game actions that exercise a card's abilities. Recipes change
//! only their own starting positions, so ordinary openings keep their original setup.

use beryl_card::{
    Ability, AbilityKind, Amount, Behavior, CardDef, Comparison, Condition, CounterKind, Effect,
    Filter, Governed, Modification, Objects, Players, Replaced, Rewrite, Seat, Subject, TargetKind,
    Trigger, TriggerEvent,
};
use beryl_engine::{TurnStep, Zone};
use beryl_scenario::{P0, P1};

use super::position::Place;

mod opening;

use opening::{Opening, open_recipe};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Recipe {
    PlaySource,
    Attack(Seat),
    AttackWithout(Filter),
    Step(Seat, TurnStep),
    Cast(Seat, Filter, Zone),
    PlayLand(Seat, Filter),
    Draw(Seat),
    DrawFromShortLibrary(Seat),
    Discard(Seat, Filter),
    GainLife(Seat),
    LoseLife(Seat, u8),
    ExileCreature,
    Escape,
    PermitFromGraveyard(Option<u8>),
    PermitFromExile,
    DamageCreature,
    DamagePlayer,
    Destroy(Seat),
    RemoveSource,
    TargetSource,
    CreateTokens(Seat),
    ReturnCreature,
    Mutate,
    RemoveLastCounter(CounterKind),
    WithoutPermanent(Filter),
    ObserveState,
    EmptyHand,
}

fn players(whose: Players) -> &'static [Seat] {
    match whose {
        Players::You => &[P0],
        Players::EachOpponent => &[P1],
        // Contextual player references are checked by the engine when the event occurs.
        _ => &Seat::BOTH,
    }
}

fn on(event: Trigger, recipe: impl Fn(Seat) -> Recipe) -> Vec<Recipe> {
    players(event.whose).iter().copied().map(recipe).collect()
}

/// Exhaustive so a new trigger event must say how to produce it.
fn trigger(event: Trigger) -> Vec<Recipe> {
    let filter = match event.subject {
        Subject::This => Filter::Any,
        Subject::A(filter) => filter,
    };
    match event.event {
        TriggerEvent::Enters | TriggerEvent::Casts if matches!(event.subject, Subject::This) => {
            vec![Recipe::PlaySource]
        }
        TriggerEvent::Enters => {
            let mut recipes = on(event, |player| Recipe::PlayLand(player, filter));
            recipes.extend(on(event, |player| {
                Recipe::Cast(player, filter, Zone::Battlefield)
            }));
            recipes
        }
        TriggerEvent::Dies | TriggerEvent::Leaves if matches!(event.subject, Subject::This) => {
            vec![Recipe::RemoveSource, Recipe::Destroy(P1)]
        }
        TriggerEvent::Dies | TriggerEvent::Leaves => on(event, |player| {
            Recipe::Destroy(if player == P0 { P1 } else { P0 })
        }),
        TriggerEvent::Attacks | TriggerEvent::PlayerAttacks => on(event, Recipe::Attack),
        // For combat damage the trigger's player is the recipient.
        TriggerEvent::DealsCombatDamageToPlayer => on(event, |recipient| {
            Recipe::Attack(if recipient == P0 { P1 } else { P0 })
        }),
        TriggerEvent::Step(step) => on(event, |player| Recipe::Step(player, step)),
        TriggerEvent::MainPhase => on(event, |player| {
            Recipe::Step(player, TurnStep::PostcombatMain)
        }),
        TriggerEvent::CreatesTokens => on(event, Recipe::CreateTokens),
        TriggerEvent::Casts => on(event, |player| Recipe::Cast(player, filter, Zone::Stack)),
        TriggerEvent::Discards => on(event, |player| Recipe::Discard(player, filter)),
        TriggerEvent::Draws => on(event, Recipe::Draw),
        TriggerEvent::BecomesTarget => vec![Recipe::TargetSource],
        TriggerEvent::Mutates => vec![Recipe::Mutate],
        TriggerEvent::DealsDamage => vec![Recipe::Attack(P0)],
        // Chapter scenarios advance turns or add lore directly before making their observations.
        TriggerEvent::TurnedFaceUp | TriggerEvent::Chapter { .. } => Vec::new(),
        TriggerEvent::State(condition) => vec![match *condition {
            Condition::Compare(Amount::Counters(Objects::This, kind), _, Amount::Fixed(0)) => {
                Recipe::RemoveLastCounter(kind)
            }
            Condition::Compare(
                Amount::Count(Objects::All {
                    zone: Zone::Battlefield,
                    filter,
                }),
                _,
                Amount::Fixed(0),
            ) => Recipe::WithoutPermanent(*filter),
            _ => Recipe::ObserveState,
        }],
    }
}

/// Exhaustive for the same reason as `trigger`. There is no event for `Replaced::None`.
fn replacement(event: Replaced) -> Vec<Recipe> {
    match event {
        Replaced::None => vec![],
        Replaced::ThisEnters => vec![Recipe::PlaySource],
        Replaced::Draw(whose, _) => players(whose).iter().copied().map(Recipe::Draw).collect(),
        Replaced::PutInto(filter, zone) => match zone {
            Zone::Graveyard => vec![
                Recipe::Destroy(P0),
                Recipe::Discard(P0, *filter),
                Recipe::Discard(P1, *filter),
            ],
            Zone::Library => vec![Recipe::GainLife(P0), Recipe::GainLife(P1)],
            Zone::Hand => vec![Recipe::RemoveSource, Recipe::Draw(P0)],
            Zone::Battlefield => vec![Recipe::Cast(P0, *filter, zone), Recipe::ReturnCreature],
            Zone::Exile => vec![Recipe::ExileCreature],
            Zone::Stack => vec![Recipe::Cast(P0, *filter, zone)],
            Zone::Nowhere | Zone::Merged => vec![],
        },
        Replaced::Enters { not_cast: true, .. } => vec![Recipe::ReturnCreature],
        Replaced::Enters {
            objects,
            not_cast: false,
        } => vec![
            Recipe::Cast(P0, *objects, Zone::Battlefield),
            Recipe::PlayLand(P0, *objects),
            Recipe::ReturnCreature,
        ],
        Replaced::Damage => vec![
            Recipe::DamagePlayer,
            Recipe::DamageCreature,
            Recipe::Attack(P0),
            Recipe::Attack(P1),
        ],
    }
}

fn recipes(def: &'static CardDef, place: Place) -> Vec<Recipe> {
    let (face, zone) = match place {
        Place::Battlefield(face) => (def.face(face), Zone::Battlefield),
        Place::Hand => (def.front(), Zone::Hand),
        Place::Graveyard => (def.front(), Zone::Graveyard),
        Place::Exile => (def.front(), Zone::Exile),
        Place::Mana(..) => return vec![],
    };
    let mut recipes = Vec::new();
    for ability in face.abilities {
        if let Some(triggered) = ability.trigger()
            && (ability.zones().contains(zone) && !triggered.delayed
                || place == Place::Hand
                    && (triggered.event == TriggerEvent::Mutates
                        || matches!(triggered.event, TriggerEvent::Dies | TriggerEvent::Leaves)))
        {
            recipes.extend(trigger(*triggered));
        }
        if let Some(replaced) = ability.replacement() {
            let works_here = if matches!(replaced.rewrite, Rewrite::Dredge(_)) {
                zone == Zone::Graveyard
            } else {
                ability.zones().contains(zone)
            };
            if works_here {
                recipes.extend(replacement(replaced.replaces));
                if ability.condition().is_some()
                    && let Replaced::Draw(whose, _) = replaced.replaces
                {
                    recipes.extend(
                        players(whose)
                            .iter()
                            .copied()
                            .map(Recipe::DrawFromShortLibrary),
                    );
                }
            }
        }
        if let Some(recipe) = casting_permission(ability, place, face.abilities) {
            recipes.push(recipe);
        }
        if !ability.zones().contains(zone) {
            continue;
        }
        if let Some(rule) = ability.rule() {
            match rule.what {
                // Either rule is checked during declarations. The multi-block rule is
                // checked after the first block, even with no other attacker to block.
                Governed::Block(_) | Governed::BlockAnyNumber(_) => {
                    recipes.push(Recipe::Attack(P1));
                }
                Governed::Attack {
                    unless_defender_controls: Some(filter),
                    ..
                } => {
                    recipes.push(Recipe::AttackWithout(filter));
                }
                Governed::Attack { .. } => recipes.push(Recipe::Attack(P0)),
                _ => {}
            }
        }
        if let Some(layer) = ability.layer() {
            if layer.modifications.contains(&Modification::GrantEscape) {
                recipes.push(Recipe::Escape);
            }
            match layer.condition {
                Some(Condition::Compare(Amount::Life(whose), _, Amount::Fixed(n))) => {
                    recipes.extend(
                        players(whose)
                            .iter()
                            .copied()
                            .map(|p| Recipe::LoseLife(p, n)),
                    );
                }
                Some(Condition::Compare(
                    Amount::Count(Objects::InZoneOf {
                        zone: Zone::Hand,
                        players: Players::You,
                        filter: Filter::Any,
                    }),
                    Comparison::Equal,
                    Amount::Fixed(0),
                )) if matches!(place, Place::Battlefield(_)) => {
                    recipes.push(Recipe::EmptyHand);
                }
                _ => {}
            }
        }
    }
    let mut unique = Vec::new();
    for recipe in recipes {
        if !unique.contains(&recipe) {
            unique.push(recipe);
        }
    }
    unique
}

/// Permissions are created by playing the granting ability. An object set up in a zone
/// without that action has no permission to cast it from there.
fn casting_permission(ability: &Ability, place: Place, abilities: &[Ability]) -> Option<Recipe> {
    let Behavior::Data(effect) = ability.body() else {
        return None;
    };
    if !effect.any(&|e| matches!(e, Effect::GainFlashback(_) | Effect::PermitCast { .. })) {
        return None;
    }
    let exiles = effect.any(&|e| {
        matches!(
            e,
            Effect::Exile(_) | Effect::ExileLinked(_) | Effect::ExileUntilThisLeaves(_)
        )
    });
    let targets_graveyard = ability
        .targets()
        .iter()
        .any(|t| t.kind == TargetKind::GraveyardCard);
    match place {
        Place::Hand if exiles => Some(Recipe::PermitFromExile),
        Place::Hand
            if targets_graveyard
                && matches!(ability.kind(), AbilityKind::Spell | AbilityKind::Triggered) =>
        {
            Some(Recipe::PermitFromGraveyard(None))
        }
        Place::Battlefield(_) if targets_graveyard && ability.kind() == AbilityKind::Activated => {
            let index = abilities.iter().position(|a| std::ptr::eq(a, ability))?;
            Some(Recipe::PermitFromGraveyard(Some(u8::try_from(index).ok()?)))
        }
        _ => None,
    }
}

pub(super) fn openings(def: &'static CardDef) -> Vec<Opening> {
    Place::of(def)
        .into_iter()
        .flat_map(|place| {
            recipes(def, place)
                .into_iter()
                .filter_map(move |recipe| open_recipe(def, place, recipe))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_has_a_recipe() {
        let events = [
            TriggerEvent::Enters,
            TriggerEvent::Dies,
            TriggerEvent::Leaves,
            TriggerEvent::Attacks,
            TriggerEvent::PlayerAttacks,
            TriggerEvent::DealsCombatDamageToPlayer,
            TriggerEvent::Step(TurnStep::Upkeep),
            TriggerEvent::MainPhase,
            TriggerEvent::CreatesTokens,
            TriggerEvent::Casts,
            TriggerEvent::Discards,
            TriggerEvent::Draws,
            TriggerEvent::BecomesTarget,
            TriggerEvent::Mutates,
            TriggerEvent::State(&Condition::Did),
        ];
        for event in events {
            for subject in [Subject::This, Subject::A(Filter::Any)] {
                assert!(
                    !trigger(Trigger::of(event, subject)).is_empty(),
                    "{event:?}"
                );
            }
        }
        let events = [
            Replaced::ThisEnters,
            Replaced::Draw(Players::You, beryl_card::Draws::Each),
            Replaced::PutInto(&Filter::Any, Zone::Graveyard),
            Replaced::Enters {
                objects: &Filter::Any,
                not_cast: false,
            },
            Replaced::Enters {
                objects: &Filter::Any,
                not_cast: true,
            },
            Replaced::Damage,
        ];
        for event in events {
            assert!(!replacement(event).is_empty(), "{event:?}");
        }
        assert!(replacement(Replaced::None).is_empty());
    }
}
