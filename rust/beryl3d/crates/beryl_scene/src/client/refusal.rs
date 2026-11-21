//! Why a click is refused (UI Conventions, "Why a click is refused"), as the web's
//! `useRefusal` answers it (`ui/src/stage/Refusal.tsx`). With each priority decision the session
//! lists the cards of the viewer's hand it will not take and why (`DecisionView::refused`). A
//! click on one of them plays the refusal sound, opens the inspector on the card with the reason
//! under its face while the pointer stays on it, and, when the reason is a timing rule an
//! opponent's permanent imposes, shows "Sorcery speed only" under that permanent for a moment
//! wherever the pointer goes. Nothing here works out what is allowed.

use beryl_protocol::protocol::{
    DecisionView, ObjectId, RefusalView, Refused, Side, Status, ZoneName,
};
use glam::Vec2;

use super::Client;
use crate::element::{Element, floats, row, text};
use crate::layout::FloatLook;
use crate::screens::Screen;
use kit_base::tokens::{color, size, type_style};
use kit_frame::frame::{ElementId, Place, Role};

/// How long the word at the permanent that forbids a click stays, in nanoseconds: the web's
/// `NOTICE_MS` of 1,800 ms.
const NOTICE: u64 = 1_800_000_000;

/// The click the open decision last refused.
#[derive(Debug)]
pub(super) struct Refusal {
    /// The decision that refused it: a later decision says its own.
    decision: beryl_protocol::DecisionId,
    refused: Refused,
    /// Whether the pointer has stayed on the refused card since the click, which keeps the
    /// reason under its inspector.
    said: bool,
    /// When the word at the permanent named by a timing refusal goes, once a frame has shown
    /// it: `None` until then.
    notice_until: Option<u64>,
    /// Whether the word has gone.
    notice_done: bool,
}

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
        self.inspector.refusal = Some(Refusal {
            decision: decision.id,
            refused: refused.clone(),
            said: true,
            notice_until: None,
            notice_done: false,
        });
        self.inspector.due = true;
        // The manual table's refusals play the same sound (`Manual::take_refusal`).
        self.manual.refused = true;
        true
    }

    /// Why the open decision refused the click on the card in `slot`, while the pointer has
    /// stayed on it since.
    pub(super) fn refusal_of(&self, slot: ObjectId) -> Option<&RefusalView> {
        let refusal = self.inspector.refusal.as_ref()?;
        let open = self.open_decision()?;
        (refusal.said && refusal.decision == open.id && refusal.refused.slot == slot)
            .then_some(&refusal.refused.reason)
    }

    /// The pointer is over the card in `hovered`, or none: the reason under the inspector goes
    /// once it has left the refused card.
    pub(super) fn refusal_hovered(&mut self, hovered: Option<ObjectId>) {
        if let Some(refusal) = &mut self.inspector.refusal
            && refusal.said
            && hovered != Some(refusal.refused.slot)
        {
            refusal.said = false;
            self.inspector.due = true;
        }
    }

    /// At the frame at `now`, shows the word under the opponent's permanent that holds the
    /// viewer to sorcery speed, for `NOTICE` after the frame that first shows it, and returns
    /// when it goes. It shows only while the decision that refused the click is open, on the
    /// game screen with the table shown.
    pub(super) fn refusal_frame(&mut self, now: u64) -> Option<u64> {
        let place = self.notice_place();
        let refusal = self.inspector.refusal.as_mut();
        let until = match (refusal, place) {
            (Some(refusal), Some(_)) if !refusal.notice_done => {
                let until = *refusal.notice_until.get_or_insert(now + NOTICE);
                refusal.notice_done = now >= until;
                (!refusal.notice_done).then_some(until)
            }
            _ => None,
        };
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
        let refusal = self.inspector.refusal.as_ref()?;
        if self.open_decision()?.id != refusal.decision {
            return None;
        }
        let RefusalView::Timing { by: Some(by) } = refusal.refused.reason else {
            return None;
        };
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

/// The word at the permanent that holds its controller's opponents to sorcery speed: the web's
/// `TimingRefusal`.
fn notice() -> Element {
    let mut word = row()
        .name("Sorcery speed only")
        .background(color::TIMING_REFUSAL)
        .border((color::STAT_DOWN, 1.0))
        .radius(size::TIMING_REFUSAL_RADIUS)
        .passes(true)
        .child(text("Sorcery speed only").style(type_style::TIMING_REFUSAL));
    let (across, down) = (
        size::TIMING_REFUSAL_PADDING_X,
        size::TIMING_REFUSAL_PADDING_Y,
    );
    word.style.padding = [down, across, down, across];
    word
}
