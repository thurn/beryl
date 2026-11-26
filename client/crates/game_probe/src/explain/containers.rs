//! The placement groups and their policies in a fitted layout.

use beryl_protocol::protocol::{Side, ZoneName};
use kit_space::arrange::PolicyKind;

use game_table::table::{Group, Layout, Place};

/// A container that places cards in this layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerInfo {
    pub name: &'static str,
    pub container: &'static str,
    pub policy: &'static str,
    pub source: &'static str,
}

/// Describes the resting groups and the prompt tray in the fitted layout.
pub fn containers(layout: &Layout) -> impl Iterator<Item = ContainerInfo> + '_ {
    let groups = Group::ALL.into_iter().map(|group| {
        let (place, container, policy, file) = match group {
            Group::Pile { side, zone } => (
                match side {
                    Some(side) => Place::Card {
                        zone,
                        side,
                        row: None,
                    },
                    None => Place::SharedPile(zone),
                },
                if layout.pile_visible(zone) {
                    "zone pile"
                } else {
                    "hidden beneath graveyard"
                },
                PolicyKind::Pile,
                "pile.rs",
            ),
            Group::Hand(side) => (
                Place::Card {
                    zone: ZoneName::Hand,
                    side,
                    row: None,
                },
                "centred hand",
                PolicyKind::Fan,
                "hands.rs",
            ),
            Group::Row { side, row } => (
                Place::Card {
                    zone: ZoneName::Battlefield,
                    side,
                    row: Some(row),
                },
                "battlefield row",
                PolicyKind::Row,
                "rows.rs",
            ),
            Group::Stack => (
                Place::Card {
                    zone: ZoneName::Stack,
                    side: Side::You,
                    row: None,
                },
                if layout.centred_stack_line().is_some() {
                    "centred stack"
                } else {
                    "anchored stack"
                },
                PolicyKind::Cascade,
                "stack.rs",
            ),
        };
        ContainerInfo {
            name: place.label(),
            container,
            policy: policy.word(),
            source: file,
        }
    });
    groups.chain(std::iter::once(ContainerInfo {
        name: "tray",
        container: if layout.tray.second.is_some() {
            "prompt tray with stacked boxes"
        } else {
            "prompt tray with side-by-side boxes"
        },
        policy: "ScreenFit (client/crates/kit_space/src/arrange/screen.rs)",
        source: "tray_layout.rs",
    }))
}
