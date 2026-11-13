//! How a game ended, and the abilities that name its cause.

use beryl_engine::card::{Ability, AbilityKind, Face, FaceAbility};
use beryl_engine::event::LossCause;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{GameTable, Layout, Observation, Outcome};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Moment, Side, Stall, name, side};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutcomeView {
    Win {
        winner: Side,
        /// Why the other player lost.
        reason: LossReason,
        /// The card that made them lose, when one did and the session saw it.
        by: Option<Culprit>,
    },
    Draw,
    /// The engine gave up when a budget or capacity ran out; not a rules outcome.
    Stalled {
        reason: Stall,
    },
}

/// Why a player lost the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LossReason {
    /// [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0 or
    /// less life, that player loses the game."
    Life,
    /// [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b): "If a player attempted
    /// to draw a card from a library with no cards in it since the last time state-based
    /// actions were checked, that player loses the game."
    Library,
    /// An effect had the other player win
    /// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
    /// that a player wins the game."
    Effect,
    /// [CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a): "A player can concede
    /// the game at any time."
    Concede,
}

/// The card that ended the game: the one whose effect had a player win or lose, or the source
/// of the damage that took a player's last life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Culprit {
    /// The name of its face.
    pub name: String,
    /// Which of its abilities did it, as its card's text orders them; none for a spell, or
    /// for damage.
    pub ability: Option<AbilityRef>,
    /// The ability's rules text, when the printed text of its face has no paragraph that
    /// says it ([`StackItem::Ability`](super::StackItem::Ability)'s `text`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
}

/// One of a face's abilities, as its rules text finds it: the `nth` of its `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AbilityRef {
    pub kind: AbilityClass,
    pub nth: u8,
}

/// The kinds of ability a face's rules text tells apart
/// ([CR 113.3](https://yawgatog.com/resources/magic-rules/#R1133)): "There are four general
/// categories of abilities", of which spell abilities have no paragraph of their own and mana
/// abilities are written as activated ones are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AbilityClass {
    Triggered,
    Activated,
    /// A static ability, one that creates a replacement effect included.
    Static,
}

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
