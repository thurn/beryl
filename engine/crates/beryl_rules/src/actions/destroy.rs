//! Destroy ([CR 701.8](https://yawgatog.com/resources/magic-rules/#R7018)).
//!
//! Implements: CR 701.8, CR 702.12b

use beryl_card::{Keywords, Objects, Regeneration};
use beryl_kernel::reference::{Permanent, Permanents};

use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement;
use crate::replacement::event::{self, Entry, Reason};
use crate::state::{State, Zone};

/// Destroys a permanent ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)):
/// "To destroy a permanent, move it from the battlefield to its owner's graveyard." One that
/// has left the battlefield since is not destroyed (see CR 400.7), nor is one with indestructible
/// ([`indestructible`]). A regeneration shield on it replaces the destruction
/// ([`replacement::destruction`]).
#[inline]
pub fn destroy<L: Layout>(game: &mut Game<'_, L>, permanent: Permanent<L>) -> Result<(), Full> {
    let Some(permanent) = game.state().permanent(permanent.into()) else {
        return Ok(());
    };
    let one = game
        .state()
        .permanents_in(Bitboard::single(permanent.slot()));
    destroy_all(game, one, Regeneration::Allowed)
}

/// Destroys a set of permanents simultaneously (CR 701.8a), as "Destroy each creature"
/// does: one event, so "whenever one or more" abilities see them together. Those with
/// indestructible stay ([`indestructible`]). Unless `regeneration` forbids it, a
/// regeneration shield replaces the destruction of the permanent it is on
/// ([`replacement::destruction`]).
#[inline]
pub fn destroy_all<L: Layout>(
    game: &mut Game<'_, L>,
    permanents: Permanents<L>,
    regeneration: Regeneration,
) -> Result<(), Full> {
    let permanents = permanents.slots();
    let permanents = game
        .state()
        .permanents_in(permanents & !indestructible(game.state(), permanents));
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    let permanents = replacement::destruction(s, t, sink, permanents, regeneration)?;
    let (to, reason) = (Zone::Graveyard, Reason::Destroy);
    let entry = Entry::default();
    event::move_batch(
        game.state, game.table, game.sink, permanents, to, reason, entry,
    )
}

/// Those of `permanents` with indestructible ([CR 702.12b]): "A permanent with indestructible
/// can't be destroyed." A regeneration shield on one is not used, since nothing would destroy
/// it.
///
/// [CR 702.12b]: https://yawgatog.com/resources/magic-rules/#R70212b
#[inline]
fn indestructible<L: Layout>(s: &State<L>, permanents: Bitboard<L::Slot>) -> Bitboard<L::Slot> {
    permanents.filter(|slot| s.derived(slot).keywords.contains(Keywords::INDESTRUCTIBLE))
}

/// [`Effect::Destroy`](beryl_card::Effect::Destroy): "Destroy [objects]." A regeneration
/// shield saves a permanent unless `regeneration` is
/// [`Regeneration::Forbidden`] ([`destroy_all`]). The outcome is the permanents that left the
/// battlefield, which a regenerated one has not.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    regeneration: Regeneration,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    if permanents.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(permanents.slots())?;
    let found = run.with_piles(permanents.slots());
    destroy_all(run.game(), permanents, regeneration)?;
    Ok(run.left_the_battlefield(found))
}
