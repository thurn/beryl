//! A decision's prompt as its seat is sent it ([`PromptView`]): the record the decision
//! carries ([`Prompt`]) by public slot and relative to the viewer, worded by the table.

use beryl_engine::card::{ManaCost, Prompt, Purpose};
use beryl_engine::{Decision, GameTable, Layout, Observation, Zone};

use super::wording::{Holes, place_word, wording};
use super::{Lies, PromptView, Words};
use crate::protocol::{land_face_name, mana_text, name, shown, side, slots};

/// The prompt the pick `d` asks belongs to, as the seat `obs` sees for is sent it; `asker`
/// names the spell or ability whose resolution asks.
///
/// It says the record and its words. The prompt's state (its marks, its buttons and what is
/// picked) is not the session's yet: a UI holds it and plans the picks.
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
        cost: cost.as_deref().unwrap_or_default(),
        source: asker,
        name: about.and_then(|slot| name(t, obs, slot)),
        land: about.and_then(|slot| land_face_name(t, obs, slot)),
        yours: shown.is_some_and(|card| card.owner() == obs.viewer()),
        from: about.map(|slot| obs.object(slot).zone()),
    };
    PromptView {
        about: slots(obs.in_candidate_order(d.about)),
        from: shown.map(|card| Lies {
            player: side(obs, card.owner()),
            zone: card.zone().into(),
        }),
        ..worded(
            &d.prompt,
            shown.map(|card| card.zone()),
            cost.clone(),
            &holes,
        )
    }
}

/// The prompt of a halt ([`DecisionView::halt`](crate::protocol::DecisionView::halt)): a
/// priority the session asks where the engine asks nothing, with passing its one answer.
pub(crate) fn halt_prompt() -> PromptView {
    worded(
        &Prompt::new(Purpose::Priority),
        None,
        None,
        &Holes::default(),
    )
}

/// `prompt` in the table's words, about no object.
fn worded(prompt: &Prompt, zone: Option<Zone>, cost: Option<String>, holes: &Holes) -> PromptView {
    let words = wording(prompt, zone);
    PromptView {
        revision: 0,
        shape: prompt.shape().into(),
        purpose: prompt.purpose.name().to_owned(),
        part: prompt.part.into(),
        title: holes.title(&words),
        words: Words {
            confirm: holes.label(words.confirm),
            counted: holes.label(words.counted),
            decline: holes.label(words.decline),
            yes: holes.label(words.yes),
            no: holes.label(words.no),
            auto: holes.label(words.auto),
            piles: prompt.places.map(|place| place_word(place).to_owned()),
        },
        count: prompt.count,
        most: prompt.most,
        places: prompt.places.map(Into::into),
        cost,
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        buttons: Vec::new(),
        state: None,
    }
}
