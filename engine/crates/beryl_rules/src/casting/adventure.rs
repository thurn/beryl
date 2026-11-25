//! Exiling a resolving Adventure and recording who may play its card.
//!
//! Implements: CR 716.3c, CR 716.3d

use beryl_kernel::reference::Spell;

use crate::fixed::Full;
use crate::layout::Layout;
use crate::replacement::event::{Reason, Sink};
use crate::state::{Continuous, Duration, Permission, StackEntry, State, Zone};
use crate::table::GameTable;

use super::leave_stack;

/// Finishes a successfully resolved Adventure ([CR 716.3d]): "Instead of putting a spell that
/// was cast as an Adventure into its owner’s graveyard as it resolves, its controller exiles
/// it." The permission is keyed to the card's new incarnation: "For as long as that card
/// remains exiled, that player may play it." A copy counts as cast as an Adventure
/// ([CR 716.3c]): "Any rule or effect that refers to a spell cast as an Adventure refers to
/// the copy as well." The copy has no card to play. It ceases to exist in exile as a state-based action
/// (see CR 704.5e).
///
/// [CR 716.3c]: https://yawgatog.com/resources/magic-rules/#R7163c
/// [CR 716.3d]: https://yawgatog.com/resources/magic-rules/#R7163d
#[cold]
#[inline(never)]
pub(crate) fn finish_adventure<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    entry: &StackEntry<L>,
    spell: Spell<L>,
) -> Result<(), Full> {
    let slot = spell.slot();
    let copy = s.object(slot).is_token();
    leave_stack(s, t, sink, entry, Zone::Exile, Reason::Resolve)?;
    if copy || s.object(slot).zone() != Zone::Exile {
        return Ok(());
    }
    let permission = Permission {
        player: entry.controller(),
        increase: 0,
        on_adventure: true,
        replacement: None,
    };
    let card = s.object_ref(slot);
    s.start_effect(
        entry.card(),
        Continuous::Cast(card, permission),
        Duration::Indefinite,
    )
}
