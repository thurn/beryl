//! Facts about what an effect does, read from its data for code that judges play rather than
//! runs it: the AI's card knowledge, which generic agents derive from definitions, and later the exporters and the renderer. Each fact is defined by the
//! instructions an effect holds, never by a card's name, so a new card that says the same
//! thing has the same fact with no list to update.
//!
//! Each fact looks through [`Effect::any`]: every instruction a resolution could come to,
//! inside sequences, conditions and choices.

use crate::continuous::Modification;
use crate::counter::CounterKind;
use crate::effect::{Effect, Objects, Players};
use crate::ids::Zone;
use crate::index::SpecIx;

impl Effect {
    /// Whether it puts cards from the top of its controller's library into their hand or
    /// onto the battlefield: it has them draw
    /// ([CR 121.1](https://yawgatog.com/resources/magic-rules/#R1211): "A player draws a
    /// card by putting the top card of their library into their hand"), take cards from the
    /// top few ([`Effect::Dig`]) or manifest dread ([`Effect::ManifestDread`]). Each of these
    /// shrinks the library, and a draw from an empty one loses the game
    /// ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)): "If a player
    /// attempted to draw a card from a library with no cards in it since the last time
    /// state-based actions were checked, that player loses the game."
    ///
    /// Only an instruction for the controller alone counts. One for every player, such as a
    /// wheel's draw of seven, is not, because the wheel puts cards back into the library
    /// first ([`Effect::wheels`]).
    pub fn takes_from_own_library(&self) -> bool {
        self.any(&|instruction| {
            matches!(
                instruction,
                Effect::Draw {
                    player: Players::You,
                    ..
                } | Effect::Dig {
                    player: Players::You,
                    ..
                } | Effect::ManifestDread(Players::You)
            )
        })
    }

    /// Whether it replaces every player's hand, as a wheel does: each player shuffles their
    /// hand into their library ([`Effect::ShuffleInto`] of cards in each player's hand, with
    /// their graveyard or without it) and each player draws. Worth casting from a nearly
    /// empty hand, and a way to refill a library that is running low.
    pub fn wheels(&self) -> bool {
        let shuffles_hands = self.any(&|instruction| {
            matches!(
                instruction,
                Effect::ShuffleInto {
                    player: Players::EachPlayer,
                    cards: Objects::CardsIn {
                        players: Players::EachPlayer,
                        zones,
                    },
                } if zones.contains(&Zone::Hand)
            )
        });
        shuffles_hands
            && self.any(&|instruction| {
                matches!(
                    instruction,
                    Effect::Draw {
                        player: Players::EachPlayer,
                        ..
                    }
                )
            })
    }

    /// Whether it helps the object chosen for the `target`th instance of the word "target":
    /// it puts a +1/+1 counter on it, gives it a keyword, or gives it +N/+N with neither
    /// number negative. Such a target belongs on its controller's side even when the target's
    /// filter would allow either side.
    pub fn helps_target(&self, target: SpecIx) -> bool {
        let is_target = |objects: &Objects| *objects == Objects::Target(target);
        self.any(&|instruction| match instruction {
            Effect::AddCounters {
                objects,
                kind: CounterKind::PlusOne,
                ..
            } => is_target(objects),
            Effect::Modify {
                objects,
                modification,
                ..
            } => {
                is_target(objects)
                    && match *modification {
                        Modification::AddKeywords(_) => true,
                        Modification::ModifyPt(power, toughness) => {
                            power >= 0 && toughness >= 0 && (power, toughness) != (0, 0)
                        }
                        _ => false,
                    }
            }
            _ => false,
        })
    }

    /// Whether it has its controller draw cards and wins them the game: an ability that wins
    /// by drawing its controller's library empty, as "Draw seven cards. Then if your library
    /// has no cards in it, you win the game" does. Worth using only when the library is that
    /// small.
    pub fn wins_by_drawing_out(&self) -> bool {
        self.any(&|instruction| {
            matches!(
                instruction,
                Effect::Draw {
                    player: Players::You,
                    ..
                }
            )
        }) && self.any(&|instruction| matches!(instruction, Effect::WinGame(Players::You)))
    }
}
