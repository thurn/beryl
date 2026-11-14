//! Regenerate ([CR 701.19]).
//!
//! A resolving "regenerate" creates a regeneration shield: an entry of the Effects store on
//! the permanent, lasting until end of turn. The shield is a replacement effect of
//! destruction ([CR 614.8]): "Regeneration is a destruction-replacement effect." Each
//! destruction is proposed to [`crate::replace::destruction`], which applies the shields: a
//! destruction by an effect ([`super::destroy`]) and the state-based destruction of a
//! creature with lethal damage (`sba`). A sacrifice and a creature with toughness 0 are not
//! destroyed, so no shield applies to them.
//!
//! [CR 701.19]: https://yawgatog.com/resources/magic-rules/#R70119
//! [CR 614.8]: https://yawgatog.com/resources/magic-rules/#R6148

use beryl_card::Objects;

use crate::fixed::Full;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{Continuous, Duration};

/// [`Effect::Regenerate`](beryl_card::Effect::Regenerate): "Regenerate [permanents]": a
/// shield on each of them ([CR 701.19a]): "If the effect of a resolving spell or ability
/// regenerates a permanent, it creates a replacement effect that protects the permanent the
/// next time it would be destroyed this turn." The outcome is the permanents shielded.
///
/// [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    let mut left = permanents;
    while let Some(permanent) = run.next_permanent(&mut left) {
        let (game, resolution) = run.resolving();
        let shield = Continuous::Regeneration(permanent);
        crate::continuous::start(game, resolution, shield, Duration::EndOfTurn)?;
    }
    Ok(Outcome::acted_on(permanents))
}
