//! Mutate ([CR 702.140](https://yawgatog.com/resources/magic-rules/#R702140)).

use beryl_card::{Objects, Prompt, Purpose, prompt};

use crate::bitboard::Bitboard;
use crate::card::Stop;
use crate::choices;
use crate::event::Event;
use crate::interpret::{Outcome, Run};
use crate::layers;
use crate::layout::Layout;
use crate::trigger;

/// [`Effect::Mutate`](beryl_card::Effect::Mutate): the mutating creature spell merges with
/// its target, `objects` ([CR 702.140c]): "As a mutating creature spell resolves, if its
/// target is legal, it doesn't enter the battlefield. Rather, it merges with the target
/// creature and becomes one object represented by more than one card or token ... The spell's
/// controller chooses whether the spell is put on top of the creature or on the bottom." The
/// spell resolves this way only with its target legal (`resolve`), so the target is there.
///
/// The merged permanent is then the object it was (CR 731.2c), and "whenever this creature
/// mutates" triggers for it ([CR 702.140d]): "An ability that triggers whenever a creature
/// mutates triggers when a spell merges with a creature as a result of a resolving mutating
/// creature spell." The outcome is the merged permanent.
///
/// [CR 702.140c]: https://yawgatog.com/resources/magic-rules/#R702140c
/// [CR 702.140d]: https://yawgatog.com/resources/magic-rules/#R702140d
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Stop> {
    let mut permanents = run.permanents(objects);
    let Some(onto) = run.next_permanent(&mut permanents) else {
        return Ok(Outcome::default());
    };
    let (game, resolution) = run.resolving();
    let spell = resolution.source();
    let (face, controller) = (resolution.entry.face(), resolution.controller());
    let over = Prompt::new(Purpose::Mutate).places(prompt::Place::Battlefield, prompt::Place::None);
    let about = Bitboard::single(spell.slot) | Bitboard::single(onto.slot);
    let top = choices::yes_no(game, resolution, controller, over, about)?;
    game.state.merge(game.sink, spell, face, onto, top)?;
    layers::refresh(game.state, game.table, game.sink)?;
    let mutated = Event::Mutated {
        object: onto,
        controller,
    };
    trigger::match_event(game.state, game.table, &mutated)?;
    Ok(Outcome::acted_on(Bitboard::single(onto.slot)))
}
