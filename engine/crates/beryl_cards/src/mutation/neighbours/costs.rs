//! Neighbours of the mana and nonmana parts of a cost.

use beryl_card::{Cost, CostAction, CostPart, ManaCost, Measure, Zone};

use super::{Kind, Neighbours, leak_slice};

pub(super) fn unsigned(n: u8) -> impl Iterator<Item = u8> {
    [n.checked_sub(1), n.checked_add(1)].into_iter().flatten()
}

pub(super) fn signed(n: i8) -> impl Iterator<Item = i8> {
    [n.checked_sub(1), n.checked_add(1)].into_iter().flatten()
}

pub(super) fn costs(cost: Cost, generic: bool) -> Vec<(Kind, String, Cost)> {
    let mut out = Vec::new();
    if generic {
        for value in unsigned(cost.mana.generic) {
            let mana = ManaCost {
                generic: value,
                ..cost.mana
            };
            out.push((
                Kind::ActivatedGeneric,
                format!("generic {value}"),
                Cost::of(mana, cost.parts()),
            ));
        }
    }
    out.extend(
        parts(cost.parts())
            .into_iter()
            .map(|(name, parts)| (Kind::CostPart, name, Cost::of(cost.mana, parts))),
    );
    out
}

pub(super) fn parts(parts: &'static [CostPart]) -> Neighbours<&'static [CostPart]> {
    let mut out = Vec::new();
    for (index, &part) in parts.iter().enumerate() {
        let mut remaining = parts.to_vec();
        remaining.remove(index);
        out.push((format!("drop part {index}"), leak_slice(remaining)));
        for (name, changed) in changes(part) {
            let mut changed_parts = parts.to_vec();
            changed_parts[index] = changed;
            out.push((format!("part {index} {name}"), leak_slice(changed_parts)));
        }
    }
    out
}

fn changes(part: CostPart) -> Neighbours<CostPart> {
    let number = |n, build: fn(u8) -> CostPart| {
        unsigned(n)
            .map(|value| (value.to_string(), build(value)))
            .collect()
    };
    match part {
        CostPart::This(CostAction::Tap) => vec![("sacrifice".to_owned(), CostPart::SACRIFICE)],
        CostPart::This(CostAction::Sacrifice) => vec![("tap".to_owned(), CostPart::TAP)],
        CostPart::This(_) | CostPart::DiscardHand => Vec::new(),
        CostPart::Life(n) => number(n, CostPart::Life),
        CostPart::Loyalty(n) => signed(n)
            .map(|value| (value.to_string(), CostPart::Loyalty(value)))
            .collect(),
        CostPart::Chosen(action, chosen) => {
            let (count, measure): (_, fn(u8) -> Measure) = match chosen.measure {
                Measure::Count(n) => (n, Measure::Count),
                Measure::TotalPower(n) => (n, Measure::TotalPower),
                Measure::ForGeneric(n) => (n, Measure::ForGeneric),
                // No printed cost has it: convoke's part is the rules', not a face's.
                Measure::ForMana => return Vec::new(),
            };
            let mut out: Neighbours<_> = unsigned(count)
                .map(|n| {
                    (
                        n.to_string(),
                        CostPart::Chosen(
                            action,
                            beryl_card::Chosen {
                                measure: measure(n),
                                ..chosen
                            },
                        ),
                    )
                })
                .collect();
            if chosen.zone == Zone::Battlefield {
                let other = match action {
                    CostAction::Tap if matches!(chosen.measure, Measure::Count(_)) => {
                        Some(CostAction::Sacrifice)
                    }
                    CostAction::Sacrifice => Some(CostAction::Tap),
                    _ => None,
                };
                if let Some(other) = other {
                    out.push((format!("{other:?}"), CostPart::Chosen(other, chosen)));
                }
            }
            out
        }
        CostPart::OneOf(options) => {
            let mut out = Vec::new();
            for (index, &option) in options.iter().enumerate() {
                for (_, name, changed) in costs(option, true) {
                    let mut options = options.to_vec();
                    options[index] = changed;
                    out.push((
                        format!("option {index} {name}"),
                        CostPart::OneOf(leak_slice(options)),
                    ));
                }
            }
            out
        }
    }
}
