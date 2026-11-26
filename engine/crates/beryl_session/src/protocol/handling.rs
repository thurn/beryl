//! What a decision shows: the cards its prompt shows, and what the opponent sees of the cards
//! it has its player look through.

use beryl_engine::card::{Purpose, Shape};
use beryl_engine::layout::Queue;
use beryl_engine::{Candidates, Decision, Layout, Observation, Opt, Zone};

use super::{CardHandling, side};

/// The first of the cards `d`'s prompt shows: a card of its object set, or else an object it
/// offers. The cards shown share an owner and a zone, so the first in candidate order says
/// whose they are and where they lie.
pub(crate) fn shown<L: Layout>(d: &Decision<L>) -> Option<L::Slot> {
    let about = d.about.min(|slot| slot);
    about.or_else(|| match &d.candidates {
        Candidates::Objects(_) => d.object_candidates().next(),
        Candidates::Options(opts) => opts.iter().find_map(|&o| match o {
            Opt::Object(slot) => Some(slot),
            _ => None,
        }),
    })
}

/// What the opponent sees of the cards `d`'s prompt has its player look through: the hand or
/// the library its object set lies in, or else its candidates, and how much of it.
pub(super) fn card_handling<L: Layout>(
    obs: Observation<'_, L>,
    d: &Decision<L>,
) -> Option<CardHandling> {
    // A yes or no, or a mode, about a card shows the opponent nothing more than the table
    // does: the card is revealed (an explore's) or being played (a land that may cost life, a
    // card offered to cast). The two that are looks at hidden cards do.
    if matches!(d.prompt.shape(), Shape::YesNo | Shape::Mode)
        && !matches!(d.prompt.purpose, Purpose::Fateseal | Purpose::Mulligan)
    {
        return None;
    }
    let card = obs.object(shown(d)?);
    let player = card.owner();
    match card.zone() {
        Zone::Library => {
            // A shared library is no player's: the side named is the player who looks.
            let player = if obs.shown().shares_zones() {
                d.decider
            } else {
                player
            };
            // The top cards the prompt counts, or the whole library.
            let size = obs.library(player).len();
            let count = (d.prompt.count > 0)
                .then(|| d.prompt.count.min(u8::try_from(size).unwrap_or(u8::MAX)));
            Some(CardHandling::Library {
                side: side(obs, player),
                count,
                scry: (d.prompt.purpose == Purpose::Scry).then_some(true),
            })
        }
        Zone::Hand => Some(CardHandling::Hand {
            side: side(obs, player),
        }),
        _ => None,
    }
}
