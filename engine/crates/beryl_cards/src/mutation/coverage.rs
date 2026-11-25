//! A playable witness for each mutation kind, and a report on generated coverage.

use std::collections::BTreeMap;

use super::neighbours::Kind;
use super::{expected, killed, mutants};
use crate::tests::fixtures;
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
    Scripted(&'static str, &'static beryl_scenario::Scenario),
}

fn witness(kind: Kind) -> Witness {
    use Witness::{Generated, Scripted};
    match kind {
        Kind::AbilityCondition => Scripted("Trinisphere", fixtures::TRINISPHERE),
        Kind::ActivatedGeneric | Kind::PreventionSource => Generated("Circle of Protection: Red"),
        Kind::AdditionalCost => Scripted("Abhorrent Oculus", fixtures::OCULUS_SHORT_GRAVEYARD),
        Kind::Amount | Kind::CopyTargets => Generated("Fork"),
        Kind::CastingOption | Kind::CastingOptionPart => Generated("Force of Will"),
        Kind::ChoiceOptional => Generated("Elite Spellbinder"),
        Kind::ChoiceVisibility | Kind::EffectObjects => Generated("Show and Tell"),
        Kind::CopyException => Scripted("Fork", fixtures::FORK),
        Kind::CostPart | Kind::ManaProduct => Generated("Forest"),
        Kind::DamageRecipients => Generated("Lightning Bolt"),
        Kind::Destination => Generated("No More Lies"),
        Kind::EffectCondition => Generated("Dark Depths"),
        Kind::EffectPlayers => Generated("Timetwister"),
        Kind::FreeCast => Scripted("Malcolm, Alluring Scoundrel", fixtures::MALCOLM),
        Kind::Keyword | Kind::TriggerFrequency => Generated("Worldgorger Dragon"),
        Kind::ModifyModification => Generated("Coordinated Assault"),
        Kind::ModifyDuration => {
            let card = crate::names::COORDINATED_ASSAULT;
            Scripted(card, &crate::written_scenarios_for(card)[0])
        }
        Kind::May | Kind::SearchFilter => Generated("Trinket Mage"),
        Kind::SearchCast => {
            let card = crate::names::PANGLACIAL_WURM;
            Scripted(card, &crate::written_scenarios_for(card)[0])
        }
        Kind::Prevention => Scripted("Circle of Protection: Red", fixtures::RED_PREVENTION),
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
        Kind::TextChangeDuration => Scripted("Mind Bend", fixtures::MIND_BEND),
        Kind::Unless => Generated("Mana Tithe"),
    }
}

#[test]
fn every_mutation_kind_has_a_witness() {
    for &kind in Kind::ALL {
        let (card, scenarios) = match witness(kind) {
            Witness::Generated(card) => (card, generated_for(card).collect()),
            Witness::Scripted(card, scenario) => (card, vec![scenario]),
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
