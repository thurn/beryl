//! How a game ended, and the abilities that name its cause.

use beryl_engine::card::{Ability, AbilityKind, Face, FaceAbility};
use beryl_engine::event::LossCause;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{GameTable, Layout, Observation, Outcome};
use super::{
    AbilityClass, AbilityRef, Culprit, LossReason, Moment, OutcomeView, name, side,
};

pub(super) fn outcome_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    over: Moment<'_, L>,
) -> OutcomeView {
    let Moment::Over {
        outcome,
        cause,
        hurt_by,
    } = over
    else {
        unreachable!("the game is over");
    };
    match outcome {
        Outcome::Win(player) => {
            let loser = obs.player(player.opponent());
            let (reason, by) = match cause {
                Some(LossCause::Life) => (
                    LossReason::Life,
                    hurt_by
                        .and_then(|r| name(t, obs, r.slot))
                        .map(|name| Culprit {
                            name: name.to_owned(),
                            ability: None,
                            text: None,
                        }),
                ),
                Some(LossCause::Library) => (LossReason::Library, None),
                Some(LossCause::Effect {
                    card,
                    face,
                    ability,
                    ..
                }) => (
                    LossReason::Effect,
                    Some(Culprit {
                        name: t.def(card).face_name(face).to_owned(),
                        ability: ability.map(|a| ability_ref(t.def(card).face(face), a)),
                        text: ability.and_then(|a| super::rules_text::unprinted(t, card, face, a)),
                    }),
                ),
                Some(LossCause::Concede) => (LossReason::Concede, None),
                // Without the loss's event, the state-based losses are read off the loser,
                // life first as the engine names them.
                None if loser.life() <= 0 => (LossReason::Life, None),
                None if loser.drew_from_empty() => (LossReason::Library, None),
                None => (LossReason::Effect, None),
            };
            OutcomeView::Win {
                winner: side(obs, player),
                reason,
                by,
            }
        }
        Outcome::Draw => OutcomeView::Draw,
        Outcome::Stalled(reason) => OutcomeView::Stalled {
            reason: reason.into(),
        },
    }
}

/// Where the rules text of `face` says ability `a`: the how-manyth of its kind.
pub(super) fn ability_ref(face: &Face, a: FaceAbility) -> AbilityRef {
    let (list, index) = match a {
        FaceAbility::Own(i) => (face.abilities, i.index()),
        FaceAbility::Grant(i) => (face.grants, i.index()),
    };
    let class = |a: &Ability| match a.kind() {
        AbilityKind::Triggered => Some(AbilityClass::Triggered),
        AbilityKind::Activated | AbilityKind::Mana => Some(AbilityClass::Activated),
        AbilityKind::Static | AbilityKind::Replacement => Some(AbilityClass::Static),
        AbilityKind::Spell => None,
    };
    let kind = list
        .get(index)
        .and_then(class)
        .unwrap_or(AbilityClass::Static);
    AbilityRef {
        kind,
        nth: super::build::paragraphs_before(list, index, |a| class(a) == Some(kind)),
    }
}
