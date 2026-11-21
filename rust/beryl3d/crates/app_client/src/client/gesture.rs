//! What the primary pointer is doing since it went down (`Gesture`): pressing an element of the
//! flat layer or a pile, setting a slider, or carrying an
//! order prompt's item. Only one of these is under way at a time. The branch of
//! `Client::point` that claims the pointer as it goes down sets it, later phases match on it,
//! and `Client::end_gesture` ends it in one place: on the release, Cancel or Leave, the next
//! press, a change of screen or sheet, and a resize.

use game_hud::inspect::Viewed;

use super::Client;
use super::carry::Carry;

use kit_frame::frame::ElementId;

/// What letting the pointer go where it went down does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Release {
    /// Presses this element of the flat layer, if the pointer is still on it.
    Element(ElementId),
    /// Opens this pile's viewer, if the pointer is still on the pile.
    Pile(Viewed),
}

/// What the primary pointer is doing since it went down, as the module describes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Gesture {
    #[default]
    None,
    /// A press, which the release carries out.
    Press(Release),
    /// A slider, which every move sets until the release.
    Slider(ElementId),
    /// An item of an order prompt's row (`carry_item`). Until it leaves the row, its release
    /// is a press as usual.
    Carry {
        carry: Carry,
        release: Option<Release>,
    },
}

impl Gesture {
    /// What letting the pointer go would do now.
    fn release(self) -> Option<Release> {
        match self {
            Gesture::Press(release) => Some(release),
            Gesture::Carry { release, .. } => release,
            Gesture::None | Gesture::Slider(_) => None,
        }
    }

    /// Sets what letting the pointer go does. A move can still cancel the press if the
    /// pointer is carrying an item. A slider takes no press.
    pub(super) fn set_release(&mut self, to: Option<Release>) {
        match self {
            Gesture::Carry { release, .. } => *release = to,
            Gesture::None | Gesture::Press(_) => *self = to.map_or(Gesture::None, Gesture::Press),
            Gesture::Slider(_) => {}
        }
    }

    /// What letting the pointer go does, which only the first caller carries out.
    pub(super) fn take_release(&mut self) -> Option<Release> {
        let release = self.release();
        self.set_release(None);
        release
    }

    /// The element of the flat layer drawn held down: the slider being set, or the element a
    /// release would press.
    pub(super) fn held(self) -> Option<ElementId> {
        match (self, self.release()) {
            (Gesture::Slider(id), _) | (_, Some(Release::Element(id))) => Some(id),
            _ => None,
        }
    }

    /// The order prompt's item being carried, which the row leaves out once it has lifted.
    pub(super) fn carried(self) -> Option<beryl_protocol::script::Subject> {
        match self {
            Gesture::Carry { carry, .. } => Some(carry.on),
            _ => None,
        }
    }
}

impl Client {
    /// Ends whatever the primary pointer was doing, putting a carried item back in its row.
    pub(super) fn end_gesture(&mut self) {
        if let Gesture::Carry { carry, .. } = std::mem::take(&mut self.gesture)
            && carry.lifted
        {
            self.put_item_down();
        }
    }
}
