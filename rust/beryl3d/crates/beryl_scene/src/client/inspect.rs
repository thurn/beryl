//! Which card the inspector shows, and showing it (`inspect::inspector`) as the float
//! `floats::INSPECTOR` of the flat layer.
//!
//! The pointer finds the card it is over after each frame is built (`Show::point`), so the
//! inspector follows the card the last frame found. A frame whose hovered card differs from the
//! one the inspector last looked at draws again at once (`Client::frame` sets `Wake::Now`). The float is laid out
//! only when the inspected card, its place or the game changes, so a still frame lays nothing
//! out and allocates nothing.

use beryl_protocol::prompt::Purpose;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};

use super::Client;
use crate::element::floats;
use crate::frame::{ElementId, Place, Role, StateBits};
use crate::input::Rect;
use crate::inspect::inspector::{self, Inspected};
use crate::layout::FloatLook;
use crate::screens::{Screen, ScreenIn};

/// The client's record of the inspector.
#[derive(Debug, Default)]
pub(super) struct Inspector {
    /// The card it shows, as it was last laid out.
    shown: Option<Inspected>,
    /// The card the pointer was over when it last looked, which it may not read.
    pub(super) judged: Option<ObjectId>,
    /// The card a finger tapped or held on to show it whole, until the next touch
    /// (`Client::card_tapped`). It is shown in place of the card under the pointer.
    pub(super) pinned: Option<ObjectId>,
    /// The view or prompt changed since it was last laid out.
    pub(super) due: bool,
    /// The click the open decision last refused, whose reason the inspector shows
    /// (`refusal`).
    pub(super) refusal: Option<super::refusal::Refusal>,
    /// When the refusal's word under the permanent that forbade the click goes, while it shows.
    pub(super) notice_until: Option<u64>,
}

impl Client {
    /// Shows the inspector for the card the last frame found under the pointer, or none, and
    /// lays it out again if the card, its place, or the view or prompt (`Inspector::due`) changed
    /// since it was last laid out. It asks for the card's face at full size. It also shows the
    /// word a refused click puts under a permanent, at the table's time `now`.
    pub(super) fn inspect(&mut self, now: u64) {
        self.inspector.notice_until = self.refusal_frame(now);
        self.inspector.judged = self.hovered_slot();
        self.refusal_hovered(self.inspector.judged);
        let inspected = self.inspected_now();
        if inspected == self.inspector.shown && !self.inspector.due {
            return;
        }
        self.inspector.due = false;
        let was_shown = self.inspector.shown.is_some();
        self.inspector.shown = inspected;
        // With nothing shown before or now, the flat layer has nothing to lay out.
        if !was_shown && self.inspector.shown.is_none() {
            return;
        }
        let input = ScreenIn {
            inspected: self.inspector.shown.as_ref(),
            ..self.screen_in()
        };
        let element = inspector::inspector(&input);
        let place = inspector::place(&input);
        let shown = element.is_some();
        self.layer.float(floats::INSPECTOR, element);
        if let (true, Some(place)) = (shown, place) {
            self.layer.set_float_look(
                floats::INSPECTOR,
                FloatLook {
                    point: place.corner,
                    ..FloatLook::default()
                },
            );
            if let Some(inspected) = &self.inspector.shown {
                self.show.faces_mut().want_full(inspected.face);
            }
        }
    }

    /// The slot of the card the last frame found under the pointer, whether or not the
    /// inspector reads it.
    pub(super) fn hovered_slot(&self) -> Option<ObjectId> {
        let slot = self.show.pointer().hovered()?;
        u16::try_from(slot).ok().map(ObjectId)
    }

    /// The card the inspector shows now, from the last frame: the card a finger asked to see
    /// (`Inspector::pinned`), or else the hovered card, at rest and not held, on the game screen
    /// with no sheet open, when it is one the web interface's `Controls` inspects. A card in the
    /// viewer's hand rises in the fan to be read instead, unless it is face down or its click was
    /// just refused; a finger, which raises no card, sees it whole.
    fn inspected_now(&self) -> Option<Inspected> {
        if self.screen != Screen::Game || self.sheet.is_some() {
            return None;
        }
        let pinned = self.inspector.pinned;
        let slot = pinned.or(self.hovered_slot())?;
        let frame = &self.frame;
        let id = ElementId(u32::from(slot.0));
        let element = (frame.resolved.iter())
            .find(|element| element.role == Role::Card && element.id == id)?;
        let Place::Card { zone, side, .. } = element.place else {
            return None;
        };
        let face_down = element.state.has(StateBits::FACE_DOWN);
        let refusal = self.refusal_of(slot).cloned();
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
        let face = self.show.faces().list().find(name)?;
        let x = element.corners.iter().map(|corner| corner.x).sum::<f32>() / 4.0;
        Some(Inspected {
            slot,
            face,
            x,
            targets: self.marked(),
            refusal,
        })
    }

    /// The boxes of the cards on the battlefield that the open prompt marks, which the player
    /// may be about to pick, as the web's `keptClear` finds them. A priority stop marks none.
    fn marked(&self) -> Vec<Rect> {
        let Some((_, prompt)) = &self.prompt else {
            return Vec::new();
        };
        if prompt.is(Purpose::Priority) {
            return Vec::new();
        }
        let marks = StateBits::ELIGIBLE | StateBits::TARGET | StateBits::SELECTED;
        (self.frame.resolved.iter())
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
}
