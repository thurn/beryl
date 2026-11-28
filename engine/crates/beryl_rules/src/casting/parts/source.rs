//! Pays the parts of an ability's cost that act on its own source: tapping it, sacrificing
//! it, discarding it or removing a counter from it.
//!
//! Implements: none. The rules for these parts are implemented in the enclosing module.

use crate::card::{CostAction, CostPart};
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::Layout;
use crate::replacement::event::{self, Reason, Sink};
use crate::state::{State, Zone};
use crate::table::GameTable;
use crate::zones::counters;

use super::super::steps::top;

/// Whether doing `action` to an ability's source takes it off the battlefield, so that the
/// ability pays it after its mana ([`super::pay`] says why). The card table refuses a cost that
/// exiles its own source.
pub(super) fn removes(action: CostAction) -> bool {
    action == CostAction::Sacrifice
}

/// Pays the parts of the cost of the ability on top of the stack that take its source off the
/// battlefield, once its mana is paid ([`super::pay`] says why they wait).
pub(in crate::casting) fn pay_removing_source<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let parts = top(s)
        .ability_in(t)
        .expect("an activated ability's entry")
        .cost()
        .parts();
    for part in parts {
        if let CostPart::This(action) = *part
            && removes(action)
        {
            pay_with_source(s, t, sink, action)?;
        }
    }
    Ok(())
}

/// Does `action` to the source of the ability `entry`, as the part of its cost that says so.
pub(super) fn pay_with_source<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    action: CostAction,
) -> Result<(), Full> {
    let source = top(s).object().expect("an activated ability's source");
    match action {
        CostAction::Tap => match s.permanent(source) {
            Some(permanent) => event::set_tapped(s, t, sink, permanent, true),
            None => Ok(()),
        },
        CostAction::Sacrifice => {
            event::move_one(s, t, sink, source.slot, Zone::Graveyard, Reason::Sacrifice)
        }
        CostAction::Discard => {
            // A card with no owner is in a shared library or graveyard, not in a hand, so it
            // cannot be discarded.
            let Some(discarder) = s.object(source.slot).owner() else {
                return Ok(());
            };
            event::move_one_by(
                s,
                t,
                sink,
                discarder,
                source.slot,
                Zone::Graveyard,
                Reason::Discard,
            )
        }
        CostAction::RemoveCounter(kind) => {
            counters::remove_from_object(&mut Game::new(s, t, sink), source, kind, 1)
        }
        CostAction::Exile | CostAction::Reveal => {
            unreachable!("the card table refuses a cost the engine cannot pay")
        }
    }
}
