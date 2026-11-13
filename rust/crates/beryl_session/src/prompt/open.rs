//! A decision's prompt as its seat is sent it ([`PromptView`]): the record the decision
//! carries ([`Prompt`]) by public slot and relative to the viewer, worded by the table.

use beryl_engine::card::{ManaCost, Prompt, Purpose};
use beryl_engine::{Decision, GameTable, Layout, Observation, Zone};

use super::wording::{Holes, cancel_word, end_words, pile_word, wording};
use super::{Lies, PromptView, ShapeBody, Words};
use crate::protocol::ObjectId;
use crate::protocol::{mana_text, name, shown, side, slots};

/// The prompt the pick `d` asks belongs to, as the seat `obs` sees for is sent it; `asker`
/// names the spell or ability whose resolution asks.
///
/// It says the record and its words. The prompt's state (its marks, its buttons and what is
/// picked) is not the session's yet ([`ShapeBody::Candidates`]): [`Held`](super::Held) adds
/// it where the session holds the shape's state.
pub(crate) fn prompt_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    d: &Decision<L>,
    asker: Option<&str>,
) -> PromptView {
    // The card the question is about, and the first of the cards the prompt shows.
    let about = obs.in_candidate_order(d.about).next();
    let shown = shown(obs, d).map(|slot| obs.object(slot));
    let cost = (d.prompt.cost != ManaCost::default()).then(|| mana_text(d.prompt.cost));
    let holes = Holes {
        count: d.prompt.count,
        most: d.prompt.most,
        cost: cost.as_deref(),
        source: asker,
        name: about.and_then(|slot| name(t, obs, slot)),
        from: about.map(|slot| obs.object(slot).zone()),
    };
    PromptView {
        about: slots(obs.in_candidate_order(d.about)),
        from: shown.map(|card| Lies {
            player: side(obs, card.owner()),
            zone: card.zone().into(),
        }),
        ..worded(&d.prompt, shown.map(|card| card.zone()), &holes)
    }
}

/// The prompt of a halt ([`DecisionView::halt`](crate::protocol::DecisionView::halt)): a
/// priority the session asks where the engine asks nothing, with passing its one answer.
pub(crate) fn halt_prompt() -> PromptView {
    worded(&Prompt::new(Purpose::Priority), None, &Holes::default())
}

/// The choice among the ways to use the card in `slot`, named `name` if the viewer knows it,
/// which a click on it opens over a priority (`Purpose::Act`), before its state: its buttons
/// are the ways ([`Mode::act`](super::mode::Mode::act)).
pub(crate) fn act_prompt(slot: ObjectId, name: Option<&str>) -> PromptView {
    let holes = Holes {
        name,
        ..Holes::default()
    };
    PromptView {
        about: vec![slot],
        ..worded(&Prompt::new(Purpose::Act), None, &holes)
    }
}

/// `prompt` in the table's words, about no object, before the session holds any state of it.
fn worded(prompt: &Prompt, zone: Option<Zone>, holes: &Holes) -> PromptView {
    let words = wording(prompt, zone);
    PromptView {
        revision: 0,
        purpose: prompt.purpose,
        part: prompt.part.into(),
        title: holes.title(&words),
        words: Words {
            confirm: holes.label(words.confirm),
            counted: holes.label(words.counted),
            decline: holes.label(words.decline),
            yes: holes.label(words.yes),
            no: holes.label(words.no),
            auto: holes.label(words.auto),
            cancel: cancel_word(prompt.purpose).to_owned(),
            piles: end_words(prompt.purpose)
                .unwrap_or_else(|| prompt.places.map(|place| pile_word(prompt.purpose, place)))
                .map(str::to_owned),
        },
        count: (prompt.count > 0).then_some(prompt.count),
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        buttons: Vec::new(),
        body: ShapeBody::Candidates,
    }
}
