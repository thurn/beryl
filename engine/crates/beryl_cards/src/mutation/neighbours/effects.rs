//! Neighbours of the instructions in an ability's effect.
use super::{Kind, leak, leak_slice};
use beryl_card::Effect;

mod instructions;
use instructions::local;

/// Each neighbour of each node of `effect`, at `at`: the node, the neighbour and the whole
/// effect with that node replaced.
pub(super) fn effect_neighbours(
    effect: &'static Effect,
    at: &str,
    out: &mut Vec<(String, Kind, String, Effect)>,
) {
    for (kind, neighbour, changed) in local(effect) {
        out.push((at.to_owned(), kind, neighbour, changed));
    }
    // A neighbour of a child, with the child replaced in this node.
    let mut child =
        |child: &'static Effect, at: String, rebuild: &dyn Fn(&'static Effect) -> Effect| {
            let mut inner = Vec::new();
            effect_neighbours(child, &at, &mut inner);
            for (node, kind, neighbour, changed) in inner {
                out.push((node, kind, neighbour, rebuild(leak(changed))));
            }
        };
    match *effect {
        Effect::Sequence(effects) => {
            for (i, step) in effects.iter().enumerate() {
                child(step, format!("{at}, step {i}"), &|changed| {
                    let mut steps = effects.to_vec();
                    steps[i] = *changed;
                    Effect::Sequence(leak_slice(steps))
                });
            }
        }
        Effect::If {
            condition,
            then,
            otherwise,
        } => {
            child(then, format!("{at}, then"), &|then| Effect::If {
                condition,
                then,
                otherwise,
            });
            if let Some(otherwise) = otherwise {
                child(otherwise, format!("{at}, otherwise"), &|otherwise| {
                    Effect::If {
                        condition,
                        then,
                        otherwise: Some(otherwise),
                    }
                });
            }
        }
        Effect::May { player, effect } => {
            child(effect, format!("{at}, inside"), &|effect| Effect::May {
                player,
                effect,
            });
        }
        Effect::Unless {
            effect,
            player,
            cost,
        } => {
            child(effect, format!("{at}, inside"), &|effect| Effect::Unless {
                effect,
                player,
                cost,
            });
        }
        Effect::ForEachPlayer { players, effect } => {
            child(effect, format!("{at}, inside"), &|effect| {
                Effect::ForEachPlayer { players, effect }
            });
        }
        _ => {}
    }
}
