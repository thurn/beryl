//! Rule changes: what the rules ask of [`State::rule_applies`], the one question about a
//! change to a rule of the game, whether a static ability or a resolved effect makes it
//! ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some continuous
//! effects affect game rules rather than objects."
//!
//! The answer for a player is kept current with the layers ([`crate::layers`]) and by the
//! Effects store, so asking walks nothing. Which permanent makes a change is found only to
//! tell a player why.

use crate::card::Rule;
use crate::filter;
use crate::layout::Layout;
use crate::state::{Affects, State};
use crate::table::GameTable;

/// Whether `rule` applies to `to` now, a player or a permanent, by a static ability or a
/// resolved effect: one AND for a player.
#[inline]
pub(crate) fn applies<L: Layout>(s: &State<L>, t: &GameTable, rule: Rule, to: Affects<L>) -> bool {
    let applies = s.rule_applies(t, rule, to);
    // A permanent's rule about itself did what it says as it is read. One for players did
    // as the layers put it in force ([`crate::layers`]): finding its ability again here
    // would walk the battlefield at every read.
    #[cfg(feature = "coverage")]
    if applies && let Affects::Object { slot, .. } = to {
        has(s, t, slot, rule);
    }
    applies
}

/// The first permanent with a static ability that makes `rule` apply to `to`, if one does
/// and not a resolved effect alone. "Each opponent" and "you" in the ability are read for
/// the permanent's controller after continuous effects
/// ([CR 109.5](https://yawgatog.com/resources/magic-rules/#R1095)): "For a static ability,
/// this is the current controller of the object it's on."
pub(crate) fn maker<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rule: Rule,
    to: Affects<L>,
) -> Option<L::Slot> {
    match to {
        Affects::Object { slot, .. } => {
            (rule.whose().is_none() && has(s, t, slot, rule)).then_some(slot)
        }
        Affects::Player(player) => {
            let whose = rule.whose()?;
            // A permanent's derived characteristics, its controller among them, are at its
            // place in the battlefield list.
            s.permanents()
                .find(|&(slot, d)| {
                    filter::is_whose(whose, player, d.controller) && has(s, t, slot, rule)
                })
                .map(|(slot, _)| slot)
        }
        // No rule is changed for a set of objects.
        Affects::Set(_) => None,
    }
}

/// Whether a permanent on the battlefield has a static ability with this rule.
#[inline]
fn has<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot, rule: Rule) -> bool {
    let has = s.copiable(slot).has_rule(t, rule);
    #[cfg(feature = "coverage")]
    if has
        && let Some(a) = crate::trigger::face(s, t, slot)
            .abilities
            .iter()
            .find(|a| a.rule == rule)
    {
        crate::coverage::hit(a);
    }
    has
}
