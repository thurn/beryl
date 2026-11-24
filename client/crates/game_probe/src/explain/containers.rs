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
        let (zone, side, row, container, policy, file) = match group {
            Group::Pile { side, zone } => (
                zone,
                side,
                None,
                if layout.pile_visible(zone) {
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
            name: Place::Card { zone, side, row }.label(),
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
