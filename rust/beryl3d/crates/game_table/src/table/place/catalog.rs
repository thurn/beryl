//! The placement groups and their policies in a fitted layout.

use beryl_protocol::protocol::{Side, ZoneName};
use kit_frame::frame::Place;
use kit_space::arrange::PolicyKind;

use super::super::{Layout, StackPlacement};
use super::Group;

/// The container that places one group's objects in this layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerInfo {
    pub place: Place,
    pub container: &'static str,
    pub policy: PolicyKind,
    pub source: &'static str,
}

pub(super) fn pile_visible(zone: ZoneName, layout: &Layout) -> bool {
    zone != ZoneName::Exile || layout.style.show_exile
}

/// Describes every group visited by the table's placement pass.
pub fn containers(layout: &Layout) -> impl ExactSizeIterator<Item = ContainerInfo> + '_ {
    Group::ALL.into_iter().map(|group| {
        let (zone, side, row, container, policy, file) = match group {
            Group::Pile { side, zone } => (
                zone,
                side,
                None,
                if pile_visible(zone, layout) {
                    "zone pile"
                } else {
                    "hidden beneath graveyard"
                },
                PolicyKind::Pile,
                "pile.rs",
            ),
            Group::Hand(side) => (
                ZoneName::Hand,
                side,
                None,
                "centred hand",
                PolicyKind::Fan,
                "hands.rs",
            ),
            Group::Row { side, row } => (
                ZoneName::Battlefield,
                side,
                Some(row),
                "battlefield row",
                PolicyKind::Row,
                "rows.rs",
            ),
            Group::Stack => (
                ZoneName::Stack,
                Side::You,
                None,
                match layout.stack {
                    StackPlacement::Anchored(_) => "anchored stack",
                    StackPlacement::Centred(_) => "centred stack",
                },
                PolicyKind::Cascade,
                "stack.rs",
            ),
        };
        ContainerInfo {
            place: Place::Card { zone, side, row },
            container,
            policy,
            source: file,
        }
    })
}
