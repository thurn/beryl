//! Neighbours of a face's keywords and casting costs.

use beryl_card::{CastingOption, Face, Keywords};

use super::{Kind, costs, leak_slice, with};

pub(super) fn neighbours(face: &Face) -> Vec<(String, Kind, String, Face)> {
    let copy = || with(face, face.abilities, face.grants);
    let mut out = Vec::new();
    for (index, option) in face.casting.iter().enumerate() {
        let node = format!("casting option {index}");
        let mut remaining = face.casting.to_vec();
        remaining.remove(index);
        out.push((
            node.clone(),
            Kind::CastingOption,
            "drop".to_owned(),
            Face {
                casting: leak_slice(remaining),
                ..copy()
            },
        ));
        let ((cost, parts), rebuild): (_, fn(_, _) -> CastingOption) = match *option {
            CastingOption::Evoke { cost, parts } => ((cost, parts), |cost, parts| {
                CastingOption::Evoke { cost, parts }
            }),
            CastingOption::Alternative { cost, parts } => ((cost, parts), |cost, parts| {
                CastingOption::Alternative { cost, parts }
            }),
            CastingOption::Escape { cost, parts } => ((cost, parts), |cost, parts| {
                CastingOption::Escape { cost, parts }
            }),
            _ => continue,
        };
        for (name, changed) in costs::parts(parts) {
            let mut casting = face.casting.to_vec();
            casting[index] = rebuild(cost, changed);
            out.push((
                node.clone(),
                Kind::CastingOptionPart,
                name,
                Face {
                    casting: leak_slice(casting),
                    ..copy()
                },
            ));
        }
    }
    if !face.additional_cost.is_empty() {
        out.push((
            "additional cost".to_owned(),
            Kind::AdditionalCost,
            "drop".to_owned(),
            Face {
                additional_cost: &[],
                ..copy()
            },
        ));
        for (name, parts) in costs::parts(face.additional_cost) {
            // Dropping the sole part is the same as dropping the entire additional cost.
            if !parts.is_empty() {
                out.push((
                    "additional cost".to_owned(),
                    Kind::AdditionalCost,
                    name,
                    Face {
                        additional_cost: parts,
                        ..copy()
                    },
                ));
            }
        }
    }
    for &(bit, name) in Keywords::NAMED {
        if face.keywords.contains(bit) {
            out.push((
                "keyword".to_owned(),
                Kind::Keyword,
                format!("drop {name}"),
                Face {
                    keywords: face.keywords.minus(bit),
                    ..copy()
                },
            ));
        }
    }
    out
}
