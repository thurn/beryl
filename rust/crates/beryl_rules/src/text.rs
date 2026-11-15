//! Text-changing effects of resolving spells and abilities ([CR 612.1]): "Some continuous
//! effects change an object’s text." The Effects store keeps each, and every reader of a
//! word reads it through the object's words (`beryl_kernel`'s `effects::text`).
//!
//! [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121

use beryl_card::{ChosenKind, Objects, Prompt, Purpose, TextWord, Zone};

use crate::card::Stop;
use crate::choices;
use crate::continuous::lasting;
use crate::interpret::{Outcome, Run};
use crate::layers;
use crate::layout::Layout;
use crate::state::Continuous;

/// [`Effect::ChangeText`](beryl_card::Effect::ChangeText): "change the text of [objects] by
/// replacing all instances of one color word with another or one basic land type with
/// another [duration]". The outcome is the objects changed.
///
/// The controller chooses the words as it resolves ([CR 608.2d]): "If an effect of a spell or
/// ability offers any choices other than choices already made as part of casting the spell,
/// activating the ability, or otherwise putting the spell or ability on the stack, the player
/// announces these while applying the effect." Any of the ten words may be replaced, one the
/// object's text does not have included, which changes nothing; the new word is another of
/// the same kind, since a color word is replaced with a color word and a land type with a land
/// type ([CR 612.2]): "A text-changing effect changes only those words that are used in the
/// correct way".
///
/// Each object is a spell on the stack or a permanent, with an effect of its own: the text
/// changes on one object compose by their timestamps ([CR 613.7]), and those on another never
/// meet them, so whether the objects' effects share a timestamp changes nothing.
///
/// [CR 608.2d]: https://yawgatog.com/resources/magic-rules/#R6082d
/// [CR 612.2]: https://yawgatog.com/resources/magic-rules/#R6122
/// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    duration: beryl_card::Duration,
) -> Result<Outcome<L>, Stop> {
    let you = run.resolution().entry.controller();
    let duration = lasting(duration, you);
    let s = run.state();
    let named = (run.scene().objects(objects))
        .filter(|slot| matches!(s.object(slot).zone(), Zone::Battlefield | Zone::Stack));
    if named.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    let (game, resolution) = run.resolving();
    let from = (choices::catalog(game, resolution, you, ChosenKind::Word, named)?.word())
        .expect("a word is chosen as a word");
    let prompt = Prompt::new(Purpose::NewWord);
    let keep = |n| TextWord::of_number(n).and_then(|to| from.to(to)).is_some();
    let to = choices::catalog_among(game, resolution, you, ChosenKind::Word, prompt, named, keep)?;
    let change = (to.word().and_then(|to| from.to(to))).expect("a new word of the same kind");
    for slot in game.state.in_candidate_order(named) {
        let effect = Continuous::Text(game.state.object_ref(slot), change);
        (game.state).start_effect(resolution.entry.card(), effect, duration)?;
    }
    layers::refresh(game.state, game.table, game.sink)?;
    Ok(Outcome::acted_on(named))
}
