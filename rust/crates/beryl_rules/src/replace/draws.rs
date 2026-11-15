//! Replacement effects of permanents' static abilities that replace a draw ([`Replaced::Draw`]):
//! which apply to a draw, and the drawing player's choice among them ([CR 616.1]). Dredge,
//! which works from a graveyard, is [`super::dredge`].
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use beryl_card::prompt::Purpose;
use beryl_card::{Draws, Prompt};

use super::{MOST, answer};
use crate::card::{Ability, AbilityIx, FaceAbility, Replaced, Replacement};
use crate::decision::{Decision, DecisionKind, Opt};
#[cfg(doc)]
use crate::event::Sink;
use crate::event::{LossCause, ReplacementKind};
use crate::filter;
use crate::fixed::{Fixed, Full};
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement;
use crate::state::{Count, ObjectRef, State, TurnStep};
use crate::table::GameTable;

/// A replacement of draws by a static ability of a permanent ([`Replaced::Draw`]) that applies
/// to a draw: the permanent, the index of the ability on its face, the ability and the
/// permanent's controller, for whom the players it names are read (CR 109.5).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawReplacement<L: Layout> {
    pub permanent: L::Slot,
    pub index: AbilityIx,
    pub ability: &'static crate::card::Ability,
    pub controller: Seat,
}

impl<L: Layout> DrawReplacement<L> {
    /// Whether applying `other` instead would do the same: the same ability of the same
    /// card, controlled by the same player. Which of two Chains of the same player's applies
    /// first makes no difference, since the other then applies to the draw that replaced it.
    fn same_as(&self, other: &Self) -> bool {
        std::ptr::eq(self.ability, other.ability) && self.controller == other.controller
    }

    /// What the rules record as having applied ([`Sink::applied`]).
    pub fn applied(&self, s: &State<L>) -> (ObjectRef<L>, u8) {
        (s.object_ref(self.permanent), self.index.index() as u8)
    }

    /// Who caused what happens instead, should it end the game.
    pub fn cause(&self, s: &State<L>, t: &GameTable) -> LossCause<L> {
        let (card, face) = s.definition(self.permanent).record(t);
        LossCause::Effect {
            source: Some(s.object_ref(self.permanent)),
            card,
            face,
            ability: Some(FaceAbility::Own(self.index)),
        }
    }
}

/// The replacement of `player`'s next draw by a permanent that applies now, if any. It
/// replaces a draw from an empty library too ([CR 614.11]): "Some effects replace card draws.
/// These effects are applied even if no cards could be drawn because there are no cards in the
/// affected player's library."
///
/// A replacement that has applied to this draw, or to a draw it replaced, is not offered again
/// ([`Sink::applied`], CR 614.5). Of those left, where two would do different things, the
/// drawing player chooses which applies first ([CR 616.1]): "the affected object's controller
/// (or its owner if it has no controller) or the affected player chooses one to apply". The
/// others may then apply to whatever replaced the draw (CR 616.1f). With every candidate the
/// same, the first in the order of the battlefield applies.
///
/// [`Full`] past [`MOST`] replacements, or where the choice stops the run ([`answer`]).
///
/// [CR 614.11]: https://yawgatog.com/resources/magic-rules/#R61411
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
pub(crate) fn draw<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
) -> Result<Option<DrawReplacement<L>>, Full> {
    let (s, t) = (&*game.state, game.table);
    let mut found: Fixed<Option<DrawReplacement<L>>, MOST> = Fixed::new();
    for &permanent in s.battlefield() {
        if !s
            .copiable(permanent)
            .may_replace(t)
            .replaces(ReplacementKind::Draw)
        {
            continue;
        }
        let controller = s.controller(permanent);
        let draws = |ability: &Ability, r: &Replacement| match *r {
            Replacement {
                replaces: Replaced::Draw(players, draws),
                ..
            } if replacement::replaces_draws(ability)
                && filter::is_for(&players, player, controller)
                && replaces_this_draw(s, player, draws) =>
            {
                Some(())
            }
            _ => None,
        };
        for (index, ability, ()) in super::statics(s, t, permanent, draws) {
            let applies = !game
                .sink
                .applied
                .as_slice()
                .contains(&(s.object_ref(permanent), index.index() as u8));
            if applies {
                found.push(Some(DrawReplacement {
                    permanent,
                    index,
                    ability,
                    controller,
                }))?;
            }
        }
    }
    let Some(&Some(first)) = found.as_slice().first() else {
        return Ok(None);
    };
    let chosen = if found.iter().flatten().all(|r| r.same_as(&first)) {
        first
    } else {
        choose_draw_replacement(game, player, &found)?
    };
    crate::coverage::hit(chosen.ability);
    Ok(Some(chosen))
}

/// The drawing player's choice among the replacements of their draw in `found`, which do not
/// all do the same ([`draw`]): one candidate for each that differs from those before it.
#[cold]
fn choose_draw_replacement<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    found: &Fixed<Option<DrawReplacement<L>>, MOST>,
) -> Result<DrawReplacement<L>, Full> {
    let mut candidates = <L::Queue<Opt<L>, 64>>::default();
    let mut firsts: Fixed<Option<DrawReplacement<L>>, MOST> = Fixed::new();
    for r in found.iter().flatten() {
        if !firsts.iter().flatten().any(|f| f.same_as(r)) {
            firsts.push(Some(*r))?;
            candidates.push(Opt::Object(r.permanent))?;
        }
    }
    let d = Decision::options(
        player,
        DecisionKind::ChooseObject,
        candidates,
        Prompt::new(Purpose::Replace),
    );
    let chosen = usize::from(answer(game.state, game.sink, &d)?.0);
    Ok(firsts.as_slice()[chosen].expect("a candidate"))
}

/// Whether a replacement of `player`'s draws that replaces those `draws` replaces the one they
/// would make now. Every draw but the first one they draw in their own draw step
/// ([CR 504.1]): "First, the active player draws a card." That is the count of cards they
/// have drawn in their draw step this turn, which counts only cards drawn: a draw that was
/// replaced never happened ([CR 614.6]), so the next one is still the first.
///
/// [CR 504.1]: https://yawgatog.com/resources/magic-rules/#R5041
/// [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146
fn replaces_this_draw<L: Layout>(s: &State<L>, player: Seat, draws: Draws) -> bool {
    match draws {
        Draws::Each => true,
        Draws::ExceptFirstInDrawStep => {
            let turn = s.turn();
            let first = turn.step() == TurnStep::Draw
                && turn.active() == player
                && s.counted(Count::DrewInDrawStep, player) == 0;
            !first
        }
    }
}
