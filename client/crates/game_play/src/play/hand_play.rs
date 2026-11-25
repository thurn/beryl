//! Carries a player's chosen hand position into the script that plays their card.

use std::rc::Rc;

use beryl_protocol::protocol::{ObjectId, PlayerView, Side, Why, ZoneName};
use beryl_protocol::script::Act;

use super::Show;

impl Show {
    /// Remembers a released pose while a play from the player's hand is with the session.
    /// Returns whether the card was in that hand when the player clicked or dropped it, which
    /// a card the hand shows from the graveyard counts as.
    pub fn begin_hand_play(&mut self, slot: ObjectId) -> bool {
        let in_hand = self.settled()
            && self.shown().is_some_and(|update| {
                update.view.objects.iter().any(|object| {
                    object.slot == slot
                        && (object.zone == ZoneName::Hand && object.owner == Side::You
                            || game_table::table::in_hand_fan(object))
                })
            });
        if in_hand {
            self.pointer.await_play(usize::from(slot.0));
        }
        in_hand
    }

    /// Lets a card go home after its click was refused or its play was cancelled.
    pub fn cancel_hand_play(&mut self, slot: ObjectId) {
        self.pointer.resume_fall(usize::from(slot.0));
    }

    /// The session can use one candidate for interchangeable cards. If the script plays a
    /// twin of the one clicked or dragged, trade their places before compiling it. The
    /// script keeps its slot numbers, but the card it moves starts where the player had it,
    /// and the card it leaves in hand keeps the untouched card's place in the fan.
    pub fn finish_hand_play(&mut self, handled: ObjectId, after: &PlayerView) {
        let settled = self.settled();
        if let Some(shown) = self.shown.as_mut().filter(|_| settled)
            && let Some((chosen, played)) = hand_play_positions(&shown.view, handled, after)
        {
            if chosen != played {
                let shown = Rc::make_mut(shown);
                let first = usize::from(shown.view.objects[chosen].slot.0);
                let second = usize::from(shown.view.objects[played].slot.0);
                shown.view.objects.swap(chosen, played);
                self.hands.trade(first, second);
                self.pointer.trade(first, second);
                self.table.apply(&shown.view);
            }
        } else {
            self.cancel_hand_play(handled);
        }
    }
}

/// The handled card and the card the script plays, by index in the resting view. Their
/// visible records must match except for the slot, and the handled card must stay in hand
/// before trading. Only plays and casts can answer the priority click seen here.
fn hand_play_positions(
    before: &PlayerView,
    handled: ObjectId,
    after: &PlayerView,
) -> Option<(usize, usize)> {
    let script = after
        .script
        .as_ref()
        .filter(|script| script.reset.is_none())?;
    let objects = &before.objects;
    let chosen = objects.iter().position(|object| object.slot == handled)?;
    let played = script
        .beats
        .iter()
        .flatten()
        .find_map(|step| match &step.act {
            Act::Fly {
                from: ZoneName::Hand,
                why,
                object,
                ..
            } if (*why == Why::Play || *why == Why::Cast) && object.owner == Side::You => {
                objects.iter().position(|before| before.slot == object.slot)
            }
            _ => None,
        })?;
    if chosen == played {
        return Some((chosen, played));
    }
    if !after
        .objects
        .iter()
        .any(|o| o.slot == handled && o.zone == ZoneName::Hand)
    {
        return None;
    }
    let mut twin = objects[played].clone();
    twin.slot = handled;
    (twin.name.is_some() && objects[chosen].zone == ZoneName::Hand && twin == objects[chosen])
        .then_some((chosen, played))
}
