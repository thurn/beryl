//! An attacker's record in a script as it is declared: its part in combat, with the tap that
//! declares it or with no motion.

use super::build::Builder;
use super::{Act, Fact, Subject};
use crate::protocol::{Combat, GameEvent, ObjectView};

impl Builder<'_> {
    /// The attackers are declared: each that did not tap to attack takes its part in combat
    /// with no motion.
    pub(super) fn attack(&mut self, attackers: &[u16], cause: Option<Subject>) {
        for &slot in attackers {
            if let Some(mut object) = self.permanent(slot)
                && let Some(combat) = self.declared(&object)
            {
                object.combat = Some(combat);
                let fact = Fact::Object { object };
                self.emit(Act::Set { fact }, cause);
            }
        }
    }

    pub(super) fn tap(&mut self, slot: u16, tapped: bool, cause: Option<Subject>) {
        let Some(mut object) = self.permanent(slot) else {
            return;
        };
        if object.tapped == tapped {
            return;
        }
        object.tapped = tapped;
        // An attacker taps as it is declared: its record says both.
        let attacks = |e: &GameEvent| matches!(e, GameEvent::Attack { attackers, .. } if attackers.contains(&slot));
        if tapped
            && self.events.iter().any(attacks)
            && let Some(combat) = self.declared(&object)
        {
            object.combat = Some(combat);
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
