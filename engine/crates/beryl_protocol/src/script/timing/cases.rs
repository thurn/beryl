//! Case classification for script steps, matching the case names in `data/ui-conventions.json`
//! and `data/arena-motion.json`.

use std::ptr;

use super::{draws, draws_to_read};
use crate::protocol::{Counter, Side, StackItem, Step as TurnStep, Why, ZoneName};
use crate::script::{Act, Arrow, Fact, Mark, Notice, Span, Stream, Subject, Table, Tally};

/// Arena's name for the case the step `act` is of (`data/arena-motion.json`'s `case`): none
/// for a case Arena's instances do not have, which is played with its verb's values. `found`
/// is the table the step finds, which says whether a life total rises or falls, and `beside`
/// the acts of its whole beat, which say whether a card is drawn alone.
#[allow(clippy::too_many_lines)] // One branch for each verb.
pub fn case_of(act: &Act, found: &Table, beside: &[&Act]) -> Option<&'static str> {
    match act {
        Act::Tap { object } => Some(if object.tapped { "tap" } else { "untap" }),
        Act::Strike { to, .. } => Some(if matches!(to, Subject::Player { .. }) {
            "unblocked attacker"
        } else {
            "blocked attacker"
        }),
        Act::Count { tally } => match tally {
            Tally::Life { player, life } => {
                let was = found.player(*player).life;
                match life.cmp(&was) {
                    std::cmp::Ordering::Equal => None,
                    std::cmp::Ordering::Greater => Some("life gain"),
                    std::cmp::Ordering::Less => Some("life loss"),
                }
            }
            Tally::Counters { counter, .. } => Some(if *counter == Counter::Loyalty {
                "loyalty"
            } else {
                "counter"
            }),
            _ => None,
        },
        Act::Fly { from, object, .. } => match (from, object.zone) {
            (ZoneName::Hand, ZoneName::Stack) => Some(if object.controller == Side::You {
                "own spell cast"
            } else {
                "opponent spell cast"
            }),
            (ZoneName::Exile, ZoneName::Stack) => Some("exile to stack"),
            (ZoneName::Stack, ZoneName::Battlefield) => Some("permanent resolves"),
            (ZoneName::Hand, ZoneName::Battlefield) => Some(if object.controller == Side::You {
                "land played"
            } else {
                "opponent land played"
            }),
            (ZoneName::Library, ZoneName::Battlefield) if object.owner == Side::You => {
                Some("library to battlefield")
            }
            (ZoneName::Library, ZoneName::Battlefield) => Some("opponent library to battlefield"),
            (ZoneName::Graveyard, ZoneName::Hand)
                if object.owner == Side::Opponent && object.name.is_some() =>
            {
                Some("opponent graveyard to hand")
            }
            (ZoneName::Exile, ZoneName::Battlefield) => Some("exile to battlefield"),
            (ZoneName::Hand, ZoneName::Graveyard) => {
                // In a shared-deck game, a card in the shared graveyard carries a fixed side
                // regardless of who discarded it, so read the side from the hand it left when
                // that card is present on `found`.
                let side = found
                    .objects
                    .get(&object.slot)
                    .map_or(object.owner, |was| was.zone_side.unwrap_or(was.owner));
                Some(if side == Side::You {
                    "discard"
                } else {
                    "opponent discard"
                })
            }
            // Yours, drawn alone during the beginning phase, plays at Arena's slower turn-draw
            // speed (0.7x); draws outside the beginning phase and multi-draws play at full speed
            // (1.0x), and cards no one reads fly face down (1.2x).
            (ZoneName::Library, ZoneName::Hand) => {
                if !draws(act) {
                    None
                } else if !draws_to_read(act) {
                    Some("opponent draw")
                } else if beside
                    .iter()
                    .any(|other| !ptr::eq(*other, act) && draws_to_read(other))
                {
                    Some("draw of several")
                } else if matches!(
                    found.turn.step,
                    TurnStep::Untap | TurnStep::Upkeep | TurnStep::Draw
                ) {
                    Some("draw")
                } else {
                    Some("fast draw")
                }
            }
            _ => None,
        },
        Act::Appear { from, why, .. } => {
            if *from == ZoneName::Nowhere {
                Some("token")
            } else if *why == Why::Draw {
                Some("opening hand")
            } else {
                None
            }
        }
        Act::Emerge { item, .. } => Some(
            if matches!(
                item,
                StackItem::Ability {
                    triggered: true,
                    ..
                }
            ) {
                "triggered ability"
            } else {
                "activated ability"
            },
        ),
        Act::Vanish {
            subject,
            from,
            why,
            object,
            ..
        } => {
            if matches!(subject, Subject::Ability { .. }) {
                return Some("ability finished");
            }
            // Where it goes says more than why. A permanent an effect returns to a hand is
            // bounced. A spell that leaves the stack for a hand by vanishing is the opponent's
            // (the viewer's own flies to their hand), and it breaks up in place on the stack as
            // a countered spell does, whether it was countered (Remand) or only returned
            // (Unsubstantiate) (UI Conventions, "An opponent's spell returned to hand": "It
            // breaks up in place on the stack over 0.5 to 1 s").
            match object.as_ref().map(|o| o.zone) {
                Some(ZoneName::Hand) if *from == ZoneName::Stack => return Some("countered"),
                Some(ZoneName::Hand) => return Some("bounced"),
                Some(ZoneName::Exile) => return Some("exiled"),
                _ => {}
            }
            match why {
                Why::Destroy | Why::StateBased => Some("destroyed"),
                Why::Counter => Some("countered"),
                Why::Resolve => Some("resolved instant or sorcery"),
                Why::Sacrifice => Some("sacrificed"),
                _ => None,
            }
        }
        Act::Point {
            span, arrow, to, ..
        } => {
            if *span == Span::End {
                return Some(if *arrow == Arrow::Source {
                    "source line end"
                } else {
                    "end"
                });
            }
            match arrow {
                Arrow::Source => Some("source line"),
                Arrow::Attack => Some("attack line"),
                Arrow::Block => Some("block line"),
                Arrow::FriendlyTarget | Arrow::HostileTarget => {
                    // At another spell or ability on the stack, its arrow loops over the two
                    // (UI Conventions, "Two items on the stack").
                    let stacked = match to {
                        Subject::Ability { .. } => true,
                        Subject::Object { slot } => found.stack.iter().any(|entry| {
                            let spell = match entry.item {
                                StackItem::Spell { slot, .. } => Some(slot),
                                StackItem::Ability { .. } => None,
                            };
                            spell == Some(*slot)
                        }),
                        Subject::Player { .. } => false,
                    };
                    Some(if stacked {
                        "stack arrow"
                    } else {
                        "target arrow"
                    })
                }
            }
        }
        Act::Mark { span, mark, .. } => {
            if *span == Span::End {
                return Some("end");
            }
            Some(match mark {
                Mark::FriendlyTarget | Mark::HostileTarget => "target",
                other => other.name(),
            })
        }
        Act::Streak { stream, .. } => Some(match stream {
            Stream::Mana { .. } => "mana",
            Stream::Damage => "damage",
        }),
        Act::Announce { notice } => match notice {
            Notice::Turn { .. } => Some("turn banner"),
            // The pill naming the end of a library the opponent chose or a value a player
            // chose stands as long as a scry's.
            Notice::Scry { .. } | Notice::TopOrBottom { .. } | Notice::Chosen { .. } => {
                Some("scry pill")
            }
            Notice::Coin { .. } => Some("coin"),
            _ => None,
        },
        // On a player, it is the table left to be seen as the game ends (script/spans.rs).
        Act::Dwell { subject } => matches!(subject, Subject::Player { .. }).then_some("game's end"),
        Act::Set { fact } => match fact {
            Fact::Turn { .. } => Some("step begins"),
            Fact::Library { .. } | Fact::ShuffledLibrary { .. } => Some("library shuffled"),
            _ => None,
        },
        // The session's lifts have no tray: only the web stage's own steps of the cards a
        // prompt holds up are cases of `lift`, and those are not timed here.
        Act::Lift { .. } | Act::Flip { .. } | Act::Showcase { .. } => None,
    }
}
