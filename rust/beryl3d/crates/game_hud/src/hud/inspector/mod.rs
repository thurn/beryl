//! The card inspector and the refusal currently shown under it.

use beryl_protocol::DecisionId;
use beryl_protocol::prompt::{PromptView, Purpose};
use beryl_protocol::protocol::{ObjectId, PlayerView, RefusalView, Refused, Side, ZoneName};
use game_table::{faces::FaceList, profiles::Profile};
use kit_frame::frame::{Anchor, ElementId, Frame, Place, Role, StateBits};
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, FloatId, floats};
use kit_ui::input::{FaceId, Rect};
use kit_ui::layout::{FloatLook, Layer};
use std::convert::Infallible;

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
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Invalidate,
    Pin(Option<ObjectId>),
    Refused(DecisionId, Refused),
}

pub enum Inputs<'a> {
    Card(CardInputs<'a>),
    Refusal,
}

#[derive(PartialEq)]
pub enum ViewKey {
    Card(Box<CardKey>),
    Refusal,
}

impl Feature for Inspector {
    const SOURCE: &'static str = file!();

    type State = Self;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        state: &mut Self,
        message: &Message,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        match message {
            Message::Invalidate => state.due = true,
            Message::Pin(slot) if *slot != state.pinned => {
                state.pinned = *slot;
                state.due = true;
            }
            Message::Pin(_) => return false,
            Message::Refused(decision, refused) => {
                state.refusal = Some(Refusal {
                    decision: *decision,
                    refused: refused.clone(),
                    said: true,
                    before_hover_refresh: true,
                    notice_until: None,
                    notice_done: false,
                });
                state.due = true;
            }
        }
        true
    }

    fn view_key(_: &Self, input: &Inputs<'_>) -> ViewKey {
        match input {
            Inputs::Card(card) => ViewKey::Card(Box::new(card.key())),
            Inputs::Refusal => ViewKey::Refusal,
        }
    }

    fn view(_: &Self, input: Inputs<'_>) -> Element {
        match input {
            Inputs::Card(card) => view::inspector(&card),
            Inputs::Refusal => view::notice(),
        }
    }
}

impl super::FloatFeature for Inspector {
    fn floats() -> impl Iterator<Item = FloatId> {
        [floats::INSPECTOR, floats::REFUSAL].into_iter()
    }
}

/// Uses the last frame's hovered card to update the inspector's float when needed.
pub struct Draw<'a> {
    pub layer: &'a mut Layer,
    pub frame: &'a Frame,
    pub faces: &'a FaceList,
    pub profile: Profile<'a>,
    pub view: Option<&'a PlayerView>,
    pub prompt: Option<&'a PromptView>,
    pub decision: Option<DecisionId>,
    pub hovered: Option<ObjectId>,
    pub until: Option<u64>,
}

impl StateUpdate<Inspector> for Draw<'_> {
    type Output = Option<FaceId>;

    fn apply(self, state: &mut Inspector) -> (bool, Self::Output) {
        state.observe(self.hovered, self.until);
        let inspected = self
            .view
            .and_then(|_| state.inspected_now(self.frame, self.faces, self.prompt, self.decision));
        if !state.refresh(inspected) {
            return (false, None);
        }
        if let Some((inspected, inputs)) =
            state.shown().zip(self.view).and_then(|(inspected, view)| {
                Some((
                    inspected,
                    CardInputs::new(inspected, self.profile, view, self.faces)?,
                ))
            })
        {
            let anchor = inputs.anchor();
            self.layer.float(
                floats::INSPECTOR,
                Some(Inspector::view(state, Inputs::Card(inputs))),
            );
            self.layer
                .anchor_float(floats::INSPECTOR, anchor, FloatLook::default());
            (true, Some(inspected.face))
        } else {
            self.layer.float(floats::INSPECTOR, None);
            (true, None)
        }
    }
}

/// Shows the temporary refusal notice at the permanent causing the timing restriction.
pub struct RefusalNotice<'a> {
    pub layer: &'a mut Layer,
    pub anchor: Option<Anchor>,
    pub now: u64,
}

impl StateUpdate<Inspector> for RefusalNotice<'_> {
    type Output = Option<u64>;

    fn apply(self, state: &mut Inspector) -> (bool, Self::Output) {
        let until = state.refusal_until(self.anchor.is_some(), self.now);
        let shown = until.and(self.anchor);
        self.layer.float(
            floats::REFUSAL,
            shown
                .as_ref()
                .map(|_| Inspector::view(state, Inputs::Refusal)),
        );
        if let Some(anchor) = shown {
            self.layer
                .anchor_float(floats::REFUSAL, anchor, FloatLook::default());
        }
        (false, until)
    }
}

impl Inspector {
    fn observe(&mut self, hovered: Option<ObjectId>, until: Option<u64>) {
        self.judged = hovered;
        self.notice_until = until;
        if let Some(refusal) = &mut self.refusal
            && !std::mem::take(&mut refusal.before_hover_refresh)
            && refusal.said
            && hovered != Some(refusal.refused.slot)
        {
            refusal.said = false;
            self.due = true;
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
    fn refresh(&mut self, inspected: Option<Inspected>) -> bool {
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
    fn refusal_until(&mut self, visible: bool, now: u64) -> Option<u64> {
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
    fn inspected_now(
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
        let bounds = kit_scene::hit::bounds(element);
        Some(Inspected {
            slot,
            face,
            x,
            origin: glam::Vec2::new(bounds.x, bounds.y),
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
    /// The first frame after a click still reads the previous frame's hovered card.
    before_hover_refresh: bool,
    /// When the word at the permanent named by a timing refusal goes, once a frame has shown
    /// it: `None` until then.
    notice_until: Option<u64>,
    /// Whether the word has gone.
    notice_done: bool,
}
