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
use game_hud::hud::inspector::{Message, notice};
use kit_base::tokens::size;
use kit_frame::frame::{ElementId, Place, Role};
use kit_ui::element::floats;
use kit_ui::layout::FloatLook;

impl Client {
    /// The open decision, as the latest view sends it, while it is the one the prompt answers.
    pub(super) fn open_decision(&self) -> Option<&DecisionView> {
        let (open, _) = self.prompt.as_ref()?;
        match &self.show.latest()?.view.status {
            Status::Deciding { decision } if decision.id == *open => Some(decision),
            _ => None,
        }
    }

    /// A click on the card in `slot` that the open decision refuses: plays the refusal sound
    /// and keeps the reason for the inspector and the word at the permanent. Returns whether
    /// the decision refuses the card, in which case the click goes no further.
    pub(super) fn refuse_click(&mut self, slot: ObjectId) -> bool {
        if self.screen != Screen::Game {
            return false;
        }
        let Some(decision) = self.open_decision() else {
            return false;
        };
        let Some(refused) = decision.refused.iter().find(|refused| refused.slot == slot) else {
            return false;
        };
        let message = Message::Refused(decision.id, refused.clone());
        self.features.inspector_mut().update(message);
        // The manual table's refusals play the same sound (`Manual::take_refusal`).
        self.manual.refused = true;
        true
    }

    /// At the frame at `now`, shows the word under the opponent's permanent that holds the
    /// viewer to sorcery speed, for `NOTICE` after the frame that first shows it, and returns
    /// when it goes. It shows only while the decision that refused the click is open, on the
    /// game screen with the table shown.
    pub(super) fn refusal_frame(&mut self, now: u64) -> Option<u64> {
        let place = self.notice_place();
        let until = self
            .features
            .inspector_mut()
            .refusal_until(place.is_some(), now);
        let shown = until.and(place);
        self.layer.float(floats::REFUSAL, shown.map(|_| notice()));
        if let Some(point) = shown {
            self.layer.set_float_look(
                floats::REFUSAL,
                FloatLook {
                    point,
                    anchor: Vec2::new(0.5, 0.0),
                    ..FloatLook::default()
                },
            );
        }
        until
    }

    /// Where the word stands, centred under the permanent the refusal names, when that is a
    /// named permanent an opponent controls on the battlefield and the decision is still open.
    fn notice_place(&self) -> Option<Vec2> {
        if self.screen != Screen::Game || !self.table_shown() {
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
        let bottom = (element.corners.iter())
            .map(|corner| corner.y)
            .fold(f32::NEG_INFINITY, f32::max);
        Some(Vec2::new(x, bottom + size::TIMING_REFUSAL_GAP))
    }
}
