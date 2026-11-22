//! Playing level bars and solving Cases before generating their later abilities.

use beryl_card::{Ability, Amount, CardDef, Comparison, Condition, Designation};
use beryl_engine::TurnStep;
use beryl_scenario::{Action, P0};

use super::opening::{Opening, open_recipe, source_name};
use super::{Recipe, trigger};
use crate::derive::play::Stimulus;
use crate::derive::position::{Place, TURN, preamble, with_filter_partners};

/// The activations that take a Class from its initial level to `level`. The runner
/// resolves each activation and any triggers it creates before taking the next one.
fn level_bars(def: &'static CardDef, place: Place, level: u8) -> Option<Vec<Stimulus>> {
    let Place::Battlefield(face) = place else {
        return None;
    };
    let abilities = def.face(face).abilities;
    let source = source_name(def, place);
    (2..=level)
        .map(|level| {
            let index = abilities
                .iter()
                .position(|a| a.designation() == Designation::LevelUp(level))?;
            Some(Stimulus::Try(Action::activate(
                P0,
                source,
                u8::try_from(index).ok()?,
            )))
        })
        .collect()
}

/// Gain a level to exercise its bar and any abilities that trigger on that change.
pub(super) fn reach_level(def: &'static CardDef, place: Place, level: u8) -> Option<Opening> {
    let mut opening = Opening::new(def, place, format!("reach Class level {level}"));
    opening.stimuli.extend(level_bars(def, place, level)?);
    Some(opening)
}

pub(super) fn openings(def: &'static CardDef, place: Place) -> Vec<Opening> {
    let Place::Battlefield(face) = place else {
        return Vec::new();
    };
    let mut openings = Vec::new();
    for ability in def.face(face).abilities {
        let (Designation::AtLevel(level), Some(event)) = (ability.designation(), ability.trigger())
        else {
            continue;
        };
        for recipe in trigger(*event) {
            if matches!(recipe, Recipe::ClassLevel(_)) {
                continue;
            }
            let (Some(mut opening), Some(bars)) = (
                open_recipe(def, place, recipe),
                level_bars(def, place, level),
            ) else {
                continue;
            };
            let beginning = preamble(def, place).len();
            opening.stimuli.splice(beginning..beginning, bars);
            opening.label = format!("{} at Class level {level}", opening.label);
            if let Some(mut with_targets) = with_filter_partners(def, place, &opening.start) {
                let mut interaction =
                    [opening.start.interaction, with_targets.interaction].concat();
                interaction.sort_unstable();
                interaction.dedup();
                with_targets.interaction = Vec::leak(interaction);
                opening.start = with_targets;
            }
            openings.push(opening);
        }
    }
    if let Some(solve) = solve_with_empty_hand(def, place, def.face(face).abilities) {
        openings.push(solve);
    }
    openings
}

/// Start early enough to solve the Case and reach its controller's next upkeep.
/// The same empty-hand condition is used for any Case that checks hand size.
fn solve_with_empty_hand(
    def: &'static CardDef,
    place: Place,
    abilities: &[Ability],
) -> Option<Opening> {
    let empty_hand = beryl_card::prelude::hand_of(&beryl_card::Players::You);
    if !abilities.iter().any(|a| {
        a.designation() == Designation::ToSolve
            && matches!(
                a.condition(),
                Some(Condition::Compare(Amount::Count(cards), Comparison::Equal, Amount::Fixed(0)))
                    if **cards == empty_hand
            )
    }) {
        return None;
    }
    let mut opening = Opening::new(def, place, "solve with an empty hand and reach upkeep");
    opening.start = opening
        .start
        .at(TURN - 2, P0, TurnStep::PrecombatMain)
        .hand(P0, &[]);
    opening.stimuli = vec![
        Stimulus::PassUntil(TURN - 2, TurnStep::End),
        Stimulus::PassUntil(TURN, TurnStep::Upkeep),
    ];
    Some(opening)
}
