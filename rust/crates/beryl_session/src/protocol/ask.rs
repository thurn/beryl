//! Today's [`AskView`] from a decision's prompt record ([`Prompt`]): the wire's form of a
//! question until the prompt itself goes on the wire, when this file goes with `AskView`.
//!
//! Each purpose the old UI tells apart keeps the variant and the numbers it had, so the record
//! changes nothing the UI sees.

use beryl_engine::card::{Part, Place, Prompt, Purpose};
use beryl_engine::{Decision, Layout, Observation, SlotId, Zone, ZoneKey};

use super::{AskView, mana_text, side, slots};

pub(super) fn ask_view<L: Layout>(obs: Observation<'_, L>, d: &Decision<L>) -> AskView {
    let Prompt {
        purpose,
        part,
        count,
        most,
        places,
        cost,
    } = d.prompt;
    // The card the question is about, and the first of the cards the prompt shows: a card of
    // its object set, or else a candidate. Its owner and zone say whose cards are shown, and
    // the cards shown share them, so the first in candidate order does.
    let about = obs.in_candidate_order(d.about).next();
    let shown = about
        .or_else(|| d.candidates.objects().next())
        .map(|slot| obs.object(slot));
    // Where a card picked, or the card asked about, goes.
    let to = zone_of(places[0]);
    match (purpose, about, shown, to) {
        (Purpose::Attack, ..) if part == Part::Aim => AskView::AttackTarget {
            attackers: slots(obs.in_candidate_order(d.about)),
        },
        (Purpose::Scry, ..) => AskView::Scry { count },
        (Purpose::Look, _, Some(card), Some(to)) => AskView::Look {
            player: side(obs, card.owner()),
            zone: card.zone().into(),
            // A hand is looked at whole.
            count: match card.zone() {
                Zone::Hand => u8::try_from(obs.zone(ZoneKey::Of(card.owner(), Zone::Hand)).count())
                    .unwrap_or(u8::MAX),
                _ => count,
            },
            to: to.into(),
            eligible: slots(obs.in_candidate_order(d.about)),
        },
        (Purpose::PutBack, ..) => AskView::PutBack { count: most },
        (Purpose::Fateseal, _, Some(card), _) => AskView::Fateseal {
            player: side(obs, card.owner()),
        },
        (Purpose::Arrange, ..) if part == Part::First => AskView::Take { count, take: most },
        (Purpose::Arrange, ..) => AskView::Bottom { count },
        (Purpose::Discard, ..) if part == Part::Instead => {
            AskView::DiscardBasicLand { count: most }
        }
        (Purpose::Move, Some(card), _, Some(to)) => AskView::MayMove {
            card: card.num(),
            to: to.into(),
        },
        (Purpose::Explore, ..) => AskView::Explore,
        (Purpose::ManifestDread, ..) => AskView::ManifestDread,
        (Purpose::ExileCost, _, Some(card), _) => AskView::ExileCards {
            zone: card.zone().into(),
            count: most,
        },
        (Purpose::TopOrBottom, Some(card), ..) => AskView::TopOrBottom { card: card.num() },
        (Purpose::Search, ..) => AskView::Search { max: most },
        (Purpose::MaySearch, ..) => AskView::MaySearch,
        (Purpose::Untap, ..) => AskView::Untap { max: most },
        (Purpose::Crew, ..) => AskView::Crew {
            crew: count,
            left: most,
        },
        (Purpose::Waterbend, ..) => AskView::Waterbend {
            cost: mana_text(cost),
        },
        (Purpose::UnlessPays, ..) => AskView::UnlessPays {
            cost: mana_text(cost),
        },
        (Purpose::PayLife, ..) => AskView::PayLife { life: count },
        (Purpose::Cleanup, ..) => AskView::Cleanup { count: most },
        _ => AskView::Plain,
    }
}

/// The zone a prompt's place is in.
fn zone_of(place: Place) -> Option<Zone> {
    match place {
        Place::None => None,
        Place::Top | Place::OnTop | Place::Bottom => Some(Zone::Library),
        Place::Hand => Some(Zone::Hand),
        Place::Graveyard => Some(Zone::Graveyard),
        Place::Exile => Some(Zone::Exile),
        Place::Battlefield => Some(Zone::Battlefield),
    }
}
