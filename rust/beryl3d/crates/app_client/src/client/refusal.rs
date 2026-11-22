//! Why a click is refused (UI Conventions, "Why a click is refused"), as the web's
//! `useRefusal` answers it (`ui/src/stage/Refusal.tsx`). With each priority decision the session
//! lists the cards of the viewer's hand it will not take and why (`DecisionView::refused`). A
//! click on one of them plays the refusal sound, opens the inspector on the card with the reason
//! under its face while the pointer stays on it, and, when the reason is a timing rule an
//! opponent's permanent imposes, shows "Sorcery speed only" under that permanent for a moment
//! wherever the pointer goes. Nothing here works out what is allowed.

use beryl_protocol::protocol::{DecisionView, ObjectId, Side, Status, ZoneName};
use glam::Vec2;

use super::Client;
use app_screens::screens::Screen;
use game_hud::hud::inspector::{Message, RefusalNotice};
use kit_base::tokens::size;
use kit_frame::frame::{Anchor, ElementId, Place, Role};

impl Client {
    /// The open decision, as the latest view sends it, while it is the one the prompt answers.
    pub(super) fn open_decision(&self) -> Option<&DecisionView> {
        let open = self.features.prompts.decision()?;
        match &self.show.latest()?.view.status {
            Status::Deciding { decision } if decision.id == open => Some(decision),
            _ => None,
        }
    }

    /// A click on the card in `slot` that the open decision refuses: plays the refusal sound
    /// and keeps the reason for the inspector and the word at the permanent. Returns whether
    /// the decision refuses the card, in which case the click goes no further.
    pub(super) fn refuse_click(&mut self, slot: ObjectId) -> bool {
        if self.features.screen != Screen::Game {
            return false;
        }
        let Some(decision) = self.open_decision() else {
            return false;
        };
        let Some(refused) = decision.refused.iter().find(|refused| refused.slot == slot) else {
            return false;
        };
        let message = Message::Refused(decision.id, refused.clone());
        self.features.inspect(&message);
        // The manual table shares this refusal sound.
        self.features
            .update_table(game_manual::features::table::Change::Refused);
        true
    }

    /// At the frame at `now`, shows the word under the opponent's permanent that holds the
    /// viewer to sorcery speed, for `NOTICE` after the frame that first shows it, and returns
    /// when it goes. It shows only while the decision that refused the click is open, on the
    /// game screen with the table shown.
    pub(super) fn refusal_frame(&mut self, now: u64) -> Option<u64> {
        let anchor = self.notice_place();
        self.features.update_inspector(RefusalNotice {
            layer: &mut self.layer,
            anchor,
            now,
        })
    }

    /// Where the word stands, centred under the permanent the refusal names, when that is a
    /// named permanent an opponent controls on the battlefield and the decision is still open.
    fn notice_place(&self) -> Option<Anchor> {
        if self.features.screen != Screen::Game || !self.table_shown() {
            return None;
        }
        let decision = self.open_decision()?.id;
        let by = self.features.inspector().timing_permanent(decision)?;
        let id = ElementId(u32::from(by.0));
        let element = (self.frame.resolved.iter())
            .find(|element| element.role == Role::Card && element.id == id)?;
        let Place::Card {
            zone: ZoneName::Battlefield,
            side: Side::Opponent,
            ..
        } = element.place
        else {
            return None;
        };
        let x = element.corners.iter().map(|corner| corner.x).sum::<f32>() / 4.0;
        let bounds = kit_scene::hit::bounds(element);
        let across = (x - bounds.x) / bounds.width.max(f32::EPSILON);
        Some(Anchor::new(
            id,
            Vec2::new(across, 1.0),
            Vec2::new(0.5, 0.0),
            Vec2::Y * size::TIMING_REFUSAL_GAP,
        ))
    }
}
