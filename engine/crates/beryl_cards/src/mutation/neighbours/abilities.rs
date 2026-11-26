//! Neighbours of an ability's interface, and reinserting its changed effect.

use beryl_card::words::Words;
use beryl_card::{
    Ability, AbilityKind, BasicLandTypes, Behavior, Condition, Cost, Designation, Face, Filter,
    ManaColor, ManaSet, Subject, Timing, Trigger, TriggerEvent, TurnStep,
};
use beryl_kernel::listener::listens_to;
use beryl_kernel::table::{is_sacrificed, is_simple};

use super::{Kind, Neighbours, continuous, costs, effects, leak, leak_slice};

pub(super) fn ability_neighbours(
    ability: &Ability,
    siblings: &[Ability],
    face: Option<&Face>,
    at: &str,
) -> Vec<(String, Kind, String, Ability)> {
    let mut out = Vec::new();
    let mut add = |field: &str, kind, name, changed| {
        out.push((format!("{at}, {field}"), kind, name, changed));
    };
    match ability.kind() {
        AbilityKind::Spell => {}
        AbilityKind::Activated | AbilityKind::Mana => {
            for (kind, name, cost) in
                costs::costs(*ability.cost(), ability.kind() == AbilityKind::Activated)
            {
                add(
                    "cost",
                    kind,
                    name,
                    with_cost_or_product(ability, cost, ability.produces()),
                );
            }
            if ability.kind() == AbilityKind::Mana {
                for (name, product) in products(ability, siblings, face) {
                    add(
                        "mana product",
                        Kind::ManaProduct,
                        name,
                        with_cost_or_product(ability, *ability.cost(), product),
                    );
                }
            }
        }
        AbilityKind::Triggered => {
            let trigger = ability.trigger().expect("triggered ability");
            for (field, kind, name, trigger) in triggers(trigger) {
                if listens_to(&trigger, ability.zones()).is_ok() {
                    add(field, kind, name, ability.with_trigger(leak(trigger)));
                }
            }
        }
        AbilityKind::Static | AbilityKind::Replacement => {
            for (field, kind, name, changed) in continuous::abilities(ability) {
                add(field, kind, name, preserve(ability, changed));
            }
        }
    }
    if let Some(condition) = ability.condition() {
        add(
            "condition",
            match ability.kind() {
                AbilityKind::Triggered => Kind::TriggerCondition,
                AbilityKind::Replacement => Kind::ReplacementCondition,
                AbilityKind::Spell
                | AbilityKind::Activated
                | AbilityKind::Mana
                | AbilityKind::Static => Kind::AbilityCondition,
            },
            "negated".to_owned(),
            ability.only_if(leak(Condition::Not(condition))),
        );
    }
    for (j, spec) in ability.targets().iter().enumerate() {
        if spec.minimum > 0 {
            continue;
        }
        let mut specs = ability.targets().to_vec();
        specs[j].optional = !spec.optional;
        add(
            &format!("target {j}"),
            Kind::TargetOptional,
            if spec.optional {
                "required"
            } else {
                "optional"
            }
            .to_owned(),
            ability.targeting(leak_slice(specs)),
        );
    }
    if ability.kind() != AbilityKind::Mana
        && let Behavior::Data(effect) = ability.body()
    {
        let mut effects = Vec::new();
        effects::effect_neighbours(effect, at, &mut effects);
        for (node, kind, neighbour, changed) in effects {
            let node = if neighbour.starts_with("rule ") && node == at {
                format!("{at}, rule")
            } else {
                node
            };
            let kind = if ability.kind() == AbilityKind::Replacement {
                Kind::ReplacementBody
            } else {
                kind
            };
            out.push((node, kind, neighbour, ability.effect(leak(changed))));
        }
    }
    out
}

fn with_cost_or_product(ability: &Ability, cost: Cost, product: ManaSet) -> Ability {
    if ability.kind() == AbilityKind::Activated {
        if let Designation::LevelUp(level) = ability.designation() {
            return Ability::level(level, cost);
        }
        return preserve(ability, Ability::activated(cost));
    }
    // A product no `ManaSet` says keeps its terms: its neighbours change the cost alone.
    let mut changed = if ability.produces() == ManaSet::NONE {
        Ability::mana_of(cost, ability.product())
    } else {
        Ability::mana(cost, product)
    };
    if let Some(filter) = ability.only_if_you_control() {
        changed = changed.if_you_control(filter);
    }
    if let Some(restriction) = ability.spend() {
        changed = changed.spent_only(restriction);
    }
    preserve(ability, changed)
}

/// A kind's constructor builds its derived fields again. Carry over the other fields
/// through the same builders that card definitions use.
pub(super) fn preserve(ability: &Ability, mut changed: Ability) -> Ability {
    changed = changed.in_zones(ability.zones());
    changed = match ability.timing() {
        Timing::YourTurn => changed.during_your_turn(),
        Timing::Sorcery => changed.as_a_sorcery(),
        Timing::AsInstant => changed.as_an_instant(),
        Timing::Instant => changed,
    };
    changed = match ability.designation() {
        Designation::AtLevel(level) => changed.at_class_level(level),
        Designation::Solved => changed.solved(),
        Designation::Imprint => changed.imprint(),
        _ => changed,
    };
    if ability.shares_paragraph() {
        changed = changed.in_paragraph_before();
    }
    if !ability.targets().is_empty() {
        changed = changed.targeting(ability.targets());
    }
    if let Some(condition) = ability.condition() {
        changed = changed.only_if(condition);
    }
    if matches!(
        ability.kind(),
        AbilityKind::Spell | AbilityKind::Activated | AbilityKind::Triggered
    ) && let Some(rule) = ability.rule()
    {
        changed = changed.starting(rule);
    }
    match ability.kind() {
        AbilityKind::Mana | AbilityKind::Static => changed,
        _ => match ability.body() {
            Behavior::Data(effect) => changed.effect(effect),
            body @ Behavior::Custom(_) => changed.custom(body),
        },
    }
}

fn products(ability: &Ability, siblings: &[Ability], face: Option<&Face>) -> Neighbours<ManaSet> {
    let product = ability.produces();
    if product == ManaSet::NONE {
        return Vec::new();
    }
    let native = face.map_or(0, |face| {
        BasicLandTypes::of(face.subtypes.read(&Words::NONE))
            .mana()
            .intrinsic_mana()
    });
    let other_simple = siblings
        .iter()
        .filter(|other| !std::ptr::eq(*other, ability) && is_simple(other))
        .fold(native, |colors, other| colors | other.produces().colors);
    let mut out = Vec::new();
    if let Some(from) = ManaColor::ALL
        .into_iter()
        .find(|&color| product.includes(color))
        && let Some(to) = ManaColor::ALL.into_iter().find(|&color| {
            let bit = ManaSet::one(color).colors;
            (bit & product.colors) == 0 && (!is_simple(ability) || (bit & other_simple) == 0)
        })
    {
        let colors = (product.colors & !ManaSet::one(from).colors) | ManaSet::one(to).colors;
        out.push((format!("{from:?} to {to:?}"), ManaSet { colors, ..product }));
    }
    // The builder represents one to three. The payment engine also requires any
    // source that makes several mana to make just one unrestricted kind.
    if product.colors.is_power_of_two()
        && ability.spend().is_none()
        && (!is_simple(ability) || other_simple == 0)
        && (is_simple(ability) || is_sacrificed(ability) || ability.timing() == Timing::AsInstant)
    {
        for amount in costs::unsigned(product.amount).filter(|n| (1..=3).contains(n)) {
            out.push((format!("amount {amount}"), ManaSet { amount, ..product }));
        }
    }
    out
}

fn triggers(trigger: &Trigger) -> Vec<(&'static str, Kind, String, Trigger)> {
    use TriggerEvent as Event;
    // Choose an event with the same kind of subject, and prefer adjacent timing for
    // step triggers. The engine matcher rejects combinations it would never read.
    let event = match trigger.event {
        Event::Enters | Event::Dies => Event::Leaves,
        // Without "untapped", a permanent that enters tapped triggers it too.
        Event::EntersUntapped => Event::Enters,
        Event::Leaves | Event::BecomesTarget | Event::Attacks => Event::Dies,
        Event::PlayerAttacks | Event::CreatesTokens | Event::Discards | Event::CastsFrom(_) => {
            Event::Casts
        }
        Event::Casts if matches!(trigger.subject, Subject::This) => Event::BecomesTarget,
        Event::Casts => Event::Discards,
        Event::BecomesTargetOfSpell
        | Event::DealsCombatDamageToPlayer
        | Event::DealsDamage
        | Event::Mutates
        | Event::TurnedFaceUp => Event::BecomesTarget,
        Event::FirstStep(step) => Event::Step(step),
        Event::Step(TurnStep::Upkeep) | Event::Draws => Event::Step(TurnStep::Draw),
        Event::Step(_) => Event::Step(TurnStep::Upkeep),
        Event::MainPhase => Event::Step(TurnStep::PrecombatMain),
        Event::ClassLevel(level) => Event::ClassLevel(if level == u8::MAX {
            level - 1
        } else {
            level + 1
        }),
        Event::Chapter { first, last } if last < u8::MAX => Event::Chapter {
            first: first + 1,
            last: last + 1,
        },
        Event::Chapter { first, last } => Event::Chapter {
            first: first.saturating_sub(1).max(1),
            last: last - 1,
        },
        Event::State(condition) => Event::State(leak(Condition::Not(condition))),
    };
    let subject = match trigger.subject {
        Subject::This => Subject::A(Filter::Any),
        Subject::A(_) => Subject::This,
    };
    let mut out = vec![
        (
            "trigger event",
            Kind::TriggerEvent,
            format!("{event:?}"),
            Trigger {
                event,
                cast_number: None,
                ..*trigger
            },
        ),
        (
            "trigger subject",
            Kind::TriggerSubject,
            format!("{subject:?}"),
            Trigger {
                subject,
                ..*trigger
            },
        ),
    ];
    if let Some(number) = trigger.cast_number {
        use beryl_card::CastNumber;
        let other = match number {
            CastNumber::InTurn(n) => CastNumber::ByCaster(n),
            CastNumber::ByCaster(n) => CastNumber::InTurn(n),
        };
        out.push((
            "cast number",
            Kind::TriggerFrequency,
            format!("{other:?}"),
            trigger.numbered(other),
        ));
    }
    if !trigger.delayed {
        out.push((
            "trigger frequency",
            Kind::TriggerFrequency,
            "next time".to_owned(),
            Trigger {
                delayed: true,
                ..*trigger
            },
        ));
    }
    out
}
