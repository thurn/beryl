//! A value a resolving spell or ability chooses, and the instructions after it that read
//! it: "Choose a card name, then target player mills a card. If a card with the chosen name
//! was milled this way, ...".
//!
//! The value is asked in the catalog prompt shape ([`crate::interpret::catalog`]), as a
//! permanent's choice as it enters is, and kept in the resolution's registers until it ends.
//! A question pauses the resolution, which runs again from its start with the answers given
//! so far, so the value is chosen again from the record each time.
//!
//! The instruction and the condition apply rules that other modules own: choosing a card
//! name (`beryl_kernel`'s table of names) and mill.
//!
//! Implements: none. It carries out rules that other modules implement.

use beryl_card::{ChosenKind, Objects, Players};

use crate::bitboard::Bitboard;
use crate::card::Stop;
use crate::interpret::{Outcome, Run, Scene, choices};
use crate::layout::Layout;

/// [`Effect::ChooseValue`](beryl_card::Effect::ChooseValue): "[player] chooses a [kind]", as
/// the instruction is reached (see [CR 608.2d]): "If an effect of a spell or ability offers
/// any choices other than choices already made as part of casting the spell, activating the
/// ability, or otherwise putting the spell or ability on the stack, the player announces
/// these while applying the effect." With no such player nothing is chosen. The outcome is
/// that something was done, acting on no object.
///
/// Out of line: a game without a card that chooses so never comes here.
///
/// [CR 608.2d]: https://yawgatog.com/resources/magic-rules/#R6082d
#[cold]
#[inline(never)]
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    player: Players,
    kind: ChosenKind,
) -> Result<Outcome<L>, Stop> {
    let Some(player) = run.one(player) else {
        return Ok(Outcome::NOTHING);
    };
    let (game, resolution) = run.resolving();
    let value = choices::catalog(game, resolution, player, kind, Bitboard::EMPTY)?;
    run.remember_chosen(value);
    Ok(Outcome::of(Bitboard::EMPTY, 1))
}

impl<L: Layout> Scene<'_, L> {
    /// [`Condition::HasChosenName`](beryl_card::Condition::HasChosenName): whether one of the
    /// objects `names` names has the card name chosen earlier in this resolution (see
    /// [CR 201.2a]): "Two or more objects have the same name if they have at least one name in
    /// common". A milled card is read where it went if that zone is public (see [CR 701.17c]):
    /// "An effect that refers to a milled card can find that card in the zone it moved to from
    /// the library, as long as that zone is a public zone." "Another card" is no card's name.
    ///
    /// [CR 201.2a]: https://yawgatog.com/resources/magic-rules/#R2012a
    /// [CR 701.17c]: https://yawgatog.com/resources/magic-rules/#R70117c
    #[cold]
    #[inline(never)]
    pub(crate) fn has_chosen_name(&self, names: Objects) -> bool {
        let Some(name) = self
            .registers
            .chosen
            .and_then(|value| value.card_name(self.t))
        else {
            return false;
        };
        self.objects(names).any(|slot| {
            self.s.object(slot).zone().is_public() && self.s.copiable(slot).has_name(self.t, name)
        })
    }
}
