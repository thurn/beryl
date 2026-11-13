//! An attacker's and a blocker's record in a script as it is declared: its part in combat,
//! with the tap that declares it or with no motion.

use super::build::Builder;
use super::{Act, Fact, Subject};
use crate::protocol::ObjectId;
use crate::protocol::{Combat, GameEvent, ObjectView};

impl Builder<'_> {
    /// The attackers are declared: each that did not tap to attack takes its part in combat
    /// with no motion.
    pub(super) fn attack(&mut self, attackers: &[ObjectId], cause: Option<Subject>) {
        for &slot in attackers {
            self.declare(slot);
            if let Some(mut object) = self.permanent(slot)
                && let Some(combat) = self.declared(&object)
            {
                object.combat = Some(combat);
                let fact = Fact::Object { object };
                self.emit(Act::Set { fact }, cause);
            }
        }
    }

    /// The attacker in `slot` has its record as it is declared, or as the view ends: its line
    /// may be drawn, unless what it attacks is still to be chosen as the view ends.
    pub(super) fn declare(&mut self, slot: ObjectId) {
        if !self.pending.contains(&slot) {
            self.aimless.retain(|&s| s != slot);
        }
    }

    /// The creature in `blocker` is declared a blocker of the one in `attacker`
    /// ([CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "For each of the
    /// chosen creatures, the defending player chooses one creature for it to block"), and
    /// the attacker is blocked
    /// ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h): "An attacking
    /// creature with one or more creatures declared as blockers for it becomes a blocked
    /// creature"): each record says so with no motion, and the line between them begins.
    pub(super) fn block(&mut self, blocker: ObjectId, attacker: ObjectId, cause: Option<Subject>) {
        if let Some(mut object) = self.permanent(blocker) {
            let blocking = Some(Combat::Blocking {
                attacker: Some(attacker),
            });
            if object.combat != blocking {
                object.combat = blocking;
                let fact = Fact::Object { object };
                self.emit(Act::Set { fact }, cause);
            }
        }
        if let Some(mut object) = self.permanent(attacker)
            && let Some(Combat::Attacking { blocked, .. }) = &mut object.combat
            && !*blocked
        {
            *blocked = true;
            let fact = Fact::Object { object };
            self.emit(Act::Set { fact }, cause);
        }
    }

    /// Every permanent leaves combat as a step that is not combat's begins
    /// ([CR 511.3](https://yawgatog.com/resources/magic-rules/#R5113): "As soon as the end
    /// of combat step ends, all creatures, battles, and planeswalkers are removed from
    /// combat"): each record says so with no motion, and the lines of combat end, so that a
    /// combat that begins and ends in one script does not leave them to its last beat.
    pub(super) fn end_combat(&mut self) {
        let fighting: Vec<ObjectId> = (self.table.battlefield.iter())
            .filter(|slot| self.permanent(**slot).is_some_and(|o| o.combat.is_some()))
            .copied()
            .collect();
        for slot in fighting {
            if let Some(mut object) = self.permanent(slot) {
                object.combat = None;
                let fact = Fact::Object { object };
                self.emit(Act::Set { fact }, None);
            }
        }
    }

    pub(super) fn tap(&mut self, slot: ObjectId, tapped: bool, cause: Option<Subject>) {
        let Some(mut object) = self.permanent(slot) else {
            return;
        };
        if object.tapped == tapped {
            return;
        }
        object.tapped = tapped;
        // An attacker taps as it is declared: its record says both.
        let attacks = |e: &GameEvent| matches!(e, GameEvent::Attack { attackers, .. } if attackers.contains(&slot));
        if tapped && self.events.iter().any(attacks) {
            self.declare(slot);
            if let Some(combat) = self.declared(&object) {
                object.combat = Some(combat);
            }
        }
        self.emit(Act::Tap { object }, cause);
    }

    /// The part in combat `object` takes as it is declared, where its record does not say it
    /// yet: the view's, not yet blocked. One whose target was still to be chosen at the view
    /// before has a record that says only that it attacks; the planeswalker it was sent at
    /// shows from here, so that its line begins as it is declared, as a line to the player
    /// does (by-w8gv.21.57).
    fn declared(&self, object: &ObjectView) -> Option<Combat> {
        let after = self.after.objects.get(&object.slot);
        let combat = match after.and_then(|o| o.combat.as_ref()) {
            Some(&Combat::Attacking {
                planeswalker,
                target_departed,
                ..
            }) => Combat::Attacking {
                planeswalker,
                target_departed,
                blocked: false,
            },
            _ => Combat::Attacking {
                planeswalker: None,
                target_departed: None,
                blocked: false,
            },
        };
        let aimless = Combat::Attacking {
            planeswalker: None,
            target_departed: None,
            blocked: false,
        };
        match &object.combat {
            None => Some(combat),
            Some(now) if *now == aimless && *now != combat => Some(combat),
            Some(_) => None,
        }
    }
}
