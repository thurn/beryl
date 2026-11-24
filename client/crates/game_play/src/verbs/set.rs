//! `set`: a part of the table is now as the step's `Fact` says, and nothing moves. The fact has
//! already been applied to the table the script plays over (`Table::apply` in
//! `beryl_protocol`): `Fact::Object` replaces a record, which the badges on its card then show;
//! `Fact::Library` and `Fact::ShuffledLibrary` reorder a pile, and `Fact::Forget` drops slots,
//! each placed at once with no motion (`Motion::step` gives what a `set` places again an instant
//! change, not a slide); `Fact::Item` and `Fact::Turn` move no card. The subject, if any, is at
//! its new place as the step begins. A `Fact::Turn` moves the step row's glow to the step of
//! the turn that begins, as the step begins (`StepOut::step_begins`).

use beryl_protocol::script::{Act, Fact, Turn};

use super::{StepIn, StepOut};

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    if let Act::Set {
        fact:
            Fact::Turn {
                number,
                own_number,
                active,
                step,
            },
    } = *step.act
    {
        out.step_begins(Turn {
            number,
            own_number,
            active,
            step,
        });
    }
    out.instant();
}
