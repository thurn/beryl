//! Changes to the sequence of phases added by an instruction.

use beryl_card::{Effect, TurnPhase};

use super::{Kind, leak_slice};

pub(super) fn neighbours(
    after: TurnPhase,
    phases: &'static [TurnPhase],
    add: &mut impl FnMut(Kind, String, Effect),
) {
    for (index, phase) in phases.iter().enumerate() {
        let mut changed = phases.to_vec();
        changed.remove(index);
        add(
            Kind::SequenceStep,
            format!("omit additional {phase:?} phase at {index}"),
            Effect::AdditionalPhases {
                after,
                phases: leak_slice(changed),
            },
        );
    }
    for (index, pair) in phases.windows(2).enumerate() {
        if pair[0] == pair[1] {
            continue;
        }
        let mut changed = phases.to_vec();
        changed.swap(index, index + 1);
        add(
            Kind::SequenceStep,
            format!("swap additional phases at {index} and {}", index + 1),
            Effect::AdditionalPhases {
                after,
                phases: leak_slice(changed),
            },
        );
    }
}
