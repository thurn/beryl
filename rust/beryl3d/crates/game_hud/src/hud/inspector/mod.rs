//! The card inspector and the refusal currently shown under it.

use beryl_protocol::DecisionId;
use beryl_protocol::prompt::{PromptView, Purpose};
use beryl_protocol::protocol::{ObjectId, RefusalView, Refused, Side, ZoneName};
use game_table::faces::FaceList;
use kit_frame::frame::{ElementId, Frame, Place, Role, StateBits};
use kit_ui::input::Rect;

mod view;
pub use view::*;

/// The inspector’s last layout and any card the player pinned.
#[derive(Debug, Default)]
pub struct Inspector {
    /// The card it shows, as it was last laid out.
    shown: Option<Inspected>,
    /// The card the pointer was over when it last looked, which it may not read.
    judged: Option<ObjectId>,
    /// The card a finger tapped or held on to show it whole, until the next touch
    /// (`Client::card_tapped`). It is shown in place of the card under the pointer.
    pinned: Option<ObjectId>,
    /// The view or prompt changed since it was last laid out.
    due: bool,
    /// The click the open decision last refused, whose reason the inspector shows
    /// (`refusal`).
    refusal: Option<Refusal>,
    /// When the refusal's word under the permanent that forbade the click goes, while it shows.
    notice_until: Option<u64>,
}

/// Changes to which card the inspector reads.
pub enum Message {
    Invalidate,
    Pin(Option<ObjectId>),
    Refused(DecisionId, Refused),
    Observe {
        hovered: Option<ObjectId>,
        until: Option<u64>,
    },
}

impl Inspector {
    /// Marks the inspector for layout if its inputs or pinned card changed.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Invalidate => self.due = true,
            Message::Pin(slot) if slot != self.pinned => {
                self.pinned = slot;
                self.due = true;
            }
            Message::Pin(_) => {}
            Message::Refused(decision, refused) => {
                self.refusal = Some(Refusal {
                    decision,
                    refused,
                    said: true,
                    notice_until: None,
                    notice_done: false,
                });
                self.due = true;
            }
            Message::Observe { hovered, until } => {
                self.judged = hovered;
                self.notice_until = until;
                if let Some(refusal) = &mut self.refusal
                    && refusal.said
                    && hovered != Some(refusal.refused.slot)
                {
                    refusal.said = false;
                    self.due = true;
                }
            }
        }
    }

    #[must_use]
    pub fn shown(&self) -> Option<&Inspected> {
        self.shown.as_ref()
    }

    #[must_use]
    pub fn pinned(&self) -> bool {
        self.pinned.is_some()
    }

    #[must_use]
    pub fn needs_frame(&self, hovered: Option<ObjectId>) -> bool {
        self.due || self.judged != hovered
    }

    #[must_use]
    pub fn notice_until(&self) -> Option<u64> {
        self.notice_until
    }

    /// Returns whether the float needs updating after a change of inputs.
    pub fn refresh(&mut self, inspected: Option<Inspected>) -> bool {
        if self.shown == inspected && !self.due {
            return false;
        }
        self.due = false;
        let changed = self.shown.is_some() || inspected.is_some();
        self.shown = inspected;
        changed
    }

    fn refusal_of(&self, slot: ObjectId, decision: Option<DecisionId>) -> Option<&RefusalView> {
        let refusal = self.refusal.as_ref()?;
        (refusal.said && Some(refusal.decision) == decision && refusal.refused.slot == slot)
            .then_some(&refusal.refused.reason)
    }

    #[must_use]
    pub fn timing_permanent(&self, decision: DecisionId) -> Option<ObjectId> {
        let refusal = self
            .refusal
            .as_ref()
            .filter(|refusal| refusal.decision == decision)?;
        match refusal.refused.reason {
            RefusalView::Timing { by } => by,
            _ => None,
        }
    }

    /// Starts the refusal notice's timer once the permanent can be seen.
    pub fn refusal_until(&mut self, visible: bool, now: u64) -> Option<u64> {
        let refusal = self
            .refusal
            .as_mut()
            .filter(|refusal| visible && !refusal.notice_done)?;
        let until = *refusal.notice_until.get_or_insert(now + NOTICE);
        refusal.notice_done = now >= until;
        (!refusal.notice_done).then_some(until)
    }

    /// The card the inspector shows now, from the last frame: the card a finger asked to see
    /// (`Inspector::pinned`), or else the hovered card, at rest and not held, on the game screen
    /// or the manual table's with no sheet open, when it is one the web interface's `Controls`
    /// inspects. A card in the viewer's hand rises in the fan to be read instead, unless it is
    /// face down or its click was just refused; a finger, which raises no card, sees it whole. A
    /// face-down card shows its face only to a viewer who knows it, whose view names it (UI
    /// Conventions, "Inspecting a face-down manual card"): one with no name has no face to find.
    pub fn inspected_now(
        &self,
        frame: &Frame,
        faces: &FaceList,
        prompt: Option<&PromptView>,
        decision: Option<DecisionId>,
    ) -> Option<Inspected> {
        let pinned = self.pinned;
        let slot = pinned.or(self.judged)?;
        let id = ElementId(u32::from(slot.0));
        let element = (frame.resolved.iter())
            .find(|element| element.role == Role::Card && element.id == id)?;
        let Place::Card { zone, side, .. } = element.place else {
            return None;
        };
        let face_down = element.state.has(StateBits::FACE_DOWN);
        let refusal = self.refusal_of(slot, decision).cloned();
        let read = match zone {
            ZoneName::Battlefield | ZoneName::Stack => true,
            ZoneName::Hand => {
                side == Side::You && (face_down || pinned.is_some() || refusal.is_some())
            }
            _ => false,
        };
        if !read || element.state.has(StateBits::MOVING) || element.state.has(StateBits::HELD) {
            return None;
        }
        let name = frame.name(element.name);
        let face = faces.find(name)?;
        let x = element.corners.iter().map(|corner| corner.x).sum::<f32>() / f32::from(4_u8);
        Some(Inspected {
            slot,
            face,
            x,
            targets: marked(prompt, frame),
            refusal,
        })
    }
}

/// Priority marks actions the player can take. Those marks need no space
/// around them when the inspector opens, unlike the targets of a choice.
fn keep_clear_for_prompt(prompt: Option<&PromptView>) -> bool {
    prompt.is_some_and(|prompt| !prompt.is(Purpose::Priority))
}

/// The boxes marked by a prompt that is actually asking the player to pick a card.
fn marked(prompt: Option<&PromptView>, frame: &Frame) -> Vec<Rect> {
    if !keep_clear_for_prompt(prompt) {
        return Vec::new();
    }
    let marks = StateBits::ELIGIBLE | StateBits::TARGET | StateBits::SELECTED;
    (frame.resolved.iter())
        .filter(|element| {
            element.role == Role::Card
                && element.state.0 & marks != 0
                && matches!(
                    element.place,
                    Place::Card {
                        zone: ZoneName::Battlefield,
                        ..
                    }
                )
        })
        .map(|element| {
            let (mut left, mut top) = (f32::INFINITY, f32::INFINITY);
            let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
            for corner in element.corners {
                left = left.min(corner.x);
                top = top.min(corner.y);
                right = right.max(corner.x);
                bottom = bottom.max(corner.y);
            }
            Rect {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            }
        })
        .collect()
}

/// How long the word at the permanent that forbids a click stays, in nanoseconds: the web's
/// `NOTICE_MS` of 1,800 ms.
const NOTICE: u64 = 1_800_000_000;

/// The click the open decision last refused.
#[derive(Debug)]
struct Refusal {
    /// The decision that refused it: a later decision says its own.
    decision: DecisionId,
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
