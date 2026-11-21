//! Replacement effects of permanents' static abilities that replace a draw ([`Replaced::Draw`]):
//! which apply to a draw, and the drawing player's choice among them ([CR 616.1]). Dredge,
//! which works from a graveyard, is [`dredge`].
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use beryl_card::prompt::Purpose;
use beryl_card::{Draws, Prompt};

use super::{MOST, answer};
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, FaceAbility, FaceIx, ObjectAbility, Replaced, Replacement, Rewrite,
};
use crate::decision::{Candidates, Decision, DecisionKind, OPTIONS, Opt};
#[cfg(doc)]
use crate::event::Sink;
use crate::event::{LossCause, Reason, ReplacementKind};
use crate::filter;
use crate::fixed::{Fixed, Full};
use crate::game::Game;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Layout, Queue};
use crate::replacement;
use crate::state::{Count, ObjectRef, State, TurnStep, Zone, ZoneKey};
use crate::table::GameTable;
use crate::{actions, zones};

/// A replacement of draws by a static ability of a permanent ([`Replaced::Draw`]) that applies
/// to a draw: the permanent, the ability's place among the permanent's abilities (one of a card
/// under a merged permanent among them), the ability and the permanent's controller, for whom
/// the players it names are read (CR 109.5).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawReplacement<L: Layout> {
    pub permanent: L::Slot,
    pub index: ObjectAbility,
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
    pub fn applied(&self, s: &State<L>) -> (ObjectRef<L>, ObjectAbility) {
        (s.object_ref(self.permanent), self.index)
    }

    /// The card whose ability it is, that card's face and the ability's index on it: the
    /// permanent's face that is up, or, for an ability of a card under a merged permanent,
    /// that card ([CR 702.140e]): "A mutated permanent has all abilities of each card and token
    /// that represents it."
    ///
    /// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
    pub fn card(&self, s: &State<L>, t: &GameTable) -> (L::Card, FaceIx, AbilityIx) {
        if let Some((card, face, index, _)) = s.merged_ability(t, self.permanent, self.index) {
            return (card, face, index);
        }
        let ObjectAbility::Own(index) = self.index else {
            unreachable!("the card table refuses a granted replacement of draws");
        };
        let (card, face) = s.definition(self.permanent).record(t);
        (card, face, index)
    }

    /// Who caused what happens instead, should it end the game: the permanent, and the card
    /// whose ability it is ([`DrawReplacement::card`]).
    pub fn cause(&self, s: &State<L>, t: &GameTable) -> LossCause<L> {
        let (card, face, index) = self.card(s, t);
        LossCause::Effect {
            source: Some(s.object_ref(self.permanent)),
            card,
            face,
            ability: Some(FaceAbility::Own(index)),
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
/// [`Full`] if a layer refresh or a scratch list runs out of room, or where the choice stops
/// the run ([`answer`]).
///
/// [CR 614.11]: https://yawgatog.com/resources/magic-rules/#R61411
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
pub(crate) fn draw<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
) -> Result<Option<DrawReplacement<L>>, Full> {
    // The draw may follow an instruction that changed a layer input during resolution.
    layers::refresh(game.state, game.table, game.sink)?;
    let (s, t) = (&*game.state, game.table);
    let mut found: Fixed<Option<DrawReplacement<L>>, MOST> = Fixed::new();
    for &permanent in s.battlefield() {
        if !super::may_replace(s, t, permanent, ReplacementKind::Draw) {
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
                .contains(&(s.object_ref(permanent), index));
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
    let mut candidates = <L::Queue<Opt<L>, OPTIONS>>::default();
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

/// N of the card in `slot`'s dredge, with its ability, if it has dredge.
fn dredge_of<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<(&'static crate::card::Ability, u8)> {
    s.abilities(t, slot)
        .find_map(|(_, a)| match a.replacement() {
            Some(&Replacement {
                rewrite: Rewrite::Dredge(n),
                ..
            }) => Some((a, n)),
            _ => None,
        })
}

/// The cards in `player`'s graveyard whose dredge could replace their next draw
/// ([CR 702.52a]): "As long as you have at least N cards in your library, if you would draw a
/// card, you may instead mill N cards and return this card from your graveyard to your hand."
/// Dredge works only there, and only for the card's owner, whose graveyard it is.
///
/// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
pub(crate) fn dredges<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Bitboard<L::Slot> {
    let library = s.library_cards_from_top(player).count();
    s.zone(ZoneKey::Of(player, Zone::Graveyard))
        .filter(|slot| dredge_of(s, t, slot).is_some_and(|(_, n)| library >= usize::from(n)))
}

/// Offers `player` the dredge abilities that could replace their next draw, if any
/// ([`dredges`]), and carries out the one they choose: they mill N cards, then return the
/// card from their graveyard to their hand ([CR 702.52a]). Returns whether the draw was
/// replaced. One draw is replaced by at most one dredge (CR 616.1f: once one has applied there
/// is no draw left to replace).
///
/// The choice is among the dredge cards and the draw itself ([`Purpose::Replace`]). A
/// replacement of draws by a permanent is not optional, so declining every dredge leaves it to
/// apply ([`draw`]), which is what applying it first would do.
///
/// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
pub(crate) fn dredge<L: Layout>(game: &mut Game<'_, L>, player: Seat) -> Result<bool, Full> {
    let cards = dredges(game.state, game.table, player);
    if cards.is_empty() {
        return Ok(false);
    }
    let mut options = <L::Queue<Opt<L>, OPTIONS>>::default();
    options.push(Opt::Done)?;
    Candidates::<L>::extend_options(&mut options, cards, |out, slot| out.push(Opt::Object(slot)))?;
    let d = Decision::options(
        player,
        DecisionKind::ChooseUpTo,
        options,
        Prompt::new(Purpose::Replace),
    );
    let Some(Opt::Object(card)) = d.candidates.option(answer(game.state, game.sink, &d)?) else {
        return Ok(false);
    };
    let (ability, n) = dredge_of(game.state, game.table, card).expect("a dredge card");
    crate::coverage::hit(ability);
    let s = &*game.state;
    game.sink.enter_source(|| s.object_ref(card));
    let done = actions::mill::mill(game, player, n)
        .and_then(|_| zones::move_all(game, Bitboard::single(card), Zone::Hand, Reason::Effect));
    game.sink.leave();
    done.map(|()| true)
}
