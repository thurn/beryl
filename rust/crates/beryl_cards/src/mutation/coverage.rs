//! A playable witness for each mutation kind, and a report on generated coverage.

use std::collections::BTreeMap;

use super::neighbours::Kind;
use super::{expected, killed, mutants, scenarios_of};
use crate::{CARDS, derive};

fn generated_for(card: &str) -> impl Iterator<Item = &'static beryl_scenario::Scenario> {
    derive::all()
        .iter()
        .filter(move |(name, _)| *name == card)
        .flat_map(|(_, scenarios)| scenarios.iter())
}

enum Witness {
    /// Varying generated answers and positions may all expose this field.
    Generated(&'static str),
    /// This sequence reaches a field the generator does not exercise yet.
    Scripted(&'static str, &'static str),
}

fn witness(kind: Kind) -> Witness {
    use Witness::{Generated, Scripted};
    match kind {
        Kind::AbilityCondition => {
            Scripted("Trinisphere", "Trinisphere: a one-mana spell costs three")
        }
        Kind::ActivatedGeneric | Kind::PreventionSource => Generated("Circle of Protection: Red"),
        Kind::AdditionalCost => Scripted(
            "Abhorrent Oculus",
            "Abhorrent Oculus can't be cast with five cards in the graveyard",
        ),
        Kind::Amount | Kind::CopyTargets => Generated("Fork"),
        Kind::CastingOption | Kind::CastingOptionPart => Generated("Force of Will"),
        Kind::ChoiceOptional => Generated("Elite Spellbinder"),
        Kind::ChoiceVisibility | Kind::EffectObjects => Generated("Show and Tell"),
        Kind::CopyException => Scripted("Fork", "Fork copies Braingeyser for 3 with a new target"),
        Kind::CostPart | Kind::ManaProduct => Generated("Forest"),
        Kind::DamageRecipients => Generated("Lightning Bolt"),
        Kind::Destination => Generated("No More Lies"),
        Kind::EffectCondition => Generated("Dark Depths"),
        Kind::EffectPlayers => Generated("Timetwister"),
        Kind::FreeCast => Scripted(
            "Malcolm, Alluring Scoundrel",
            "Malcolm's fourth chorus counter casts the discard",
        ),
        Kind::Keyword | Kind::TriggerFrequency => Generated("Worldgorger Dragon"),
        Kind::May | Kind::SearchFilter => Generated("Trinket Mage"),
        Kind::Prevention => Scripted(
            "Circle of Protection: Red",
            "Circle of Protection: Red prevents Lightning Bolt's damage",
        ),
        Kind::PutController
        | Kind::PutLink
        | Kind::PutTapped
        | Kind::SequenceStep
        | Kind::TargetOptional
        | Kind::TriggerCondition
        | Kind::TriggerSubject => Generated("Animate Dead"),
        Kind::Regeneration => Generated("Strip Mine"),
        Kind::ReplacementBody | Kind::ReplacementEvent | Kind::ReplacementRewrite => {
            Generated("Chains of Mephistopheles")
        }
        Kind::ReplacementCondition => Generated("Jace, Wielder of Mysteries"),
        Kind::RuleGoverned | Kind::RulePlayers | Kind::RuleWhen | Kind::TriggerEvent => {
            Generated("Dandân")
        }
        Kind::StaticAffected => Generated("Glorious Anthem"),
        Kind::StaticCondition => Generated("Wild Nacatl"),
        Kind::StaticModification => Generated("Lord of Atlantis"),
        Kind::TextChangeDuration => Scripted(
            "Mind Bend",
            "Mind Bend makes Circle of Protection: Red a circle against white",
        ),
        Kind::Unless => Generated("Mana Tithe"),
    }
}

#[test]
fn every_mutation_kind_has_a_witness() {
    for &kind in Kind::ALL {
        let (card, scenarios) = match witness(kind) {
            Witness::Generated(card) => (card, generated_for(card).collect()),
            Witness::Scripted(card, name) => {
                let scenarios = scenarios_of(card).into_iter().filter(|s| s.name == name);
                (card, scenarios.collect())
            }
        };
        let def = CARDS.get(card).expect("the witness card has a definition");
        let expected = expected(scenarios);
        assert!(
            mutants(def)
                .into_iter()
                .filter(|m| m.kind == kind)
                .any(|m| killed(m.def, &expected)),
            "{card}: no witness distinguished a {kind:?} neighbour",
        );
    }
}

/// Counts actual kills, excluding hand-written scenarios and equivalences from the score.
#[test]
#[ignore = "reports generated-scenario mutation coverage for a developer measurement"]
fn generated_mutation_coverage() {
    let mut survivors = BTreeMap::<_, usize>::new();
    let mut covered = 0;
    for &def in CARDS.cards() {
        let generated = expected(generated_for(def.name).collect());
        let mut only_generated = true;
        for mutant in mutants(def) {
            if !killed(mutant.def, &generated) {
                only_generated = false;
                *survivors.entry(mutant.kind).or_default() += 1;
            }
        }
        covered += usize::from(only_generated);
    }
    println!(
        "{covered}/{} cards with every mutant killed by generated scenarios",
        CARDS.cards().len()
    );
    println!("survivors by kind: {survivors:?}");
}
