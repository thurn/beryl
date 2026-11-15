//! Wire data for a manual table, projected for one viewer.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ObjectId, Side, TargetRef, ZoneName};
use crate::script::{Arrow, Subject};

mod actions;

pub use actions::TableActionRefs;

/// An opaque object reference issued to one viewer for one published view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TableObjectRef {
    view: String,
    handle: u32,
    // An arrow source can follow this hand card to where an earlier batch member played it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    after_move: Option<bool>,
}

/// The action schema shared with the host log, with viewer references in every object field.
pub type TableAction = beryl_table::Action<TableObjectRef>;

/// An actionable card or stack marker in this view, with the zone required by commands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TableObjectAction {
    pub subject: Subject,
    pub reference: TableObjectRef,
    pub zone: beryl_table::state::Zone,
}

/// A battlefield row explicitly chosen by a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RowName {
    Creatures,
    Others,
    Lands,
}

/// The face of a card that the manual table is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FaceName {
    #[default]
    Front,
    Back,
    Down,
}

impl FaceName {
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's skip predicate takes a reference"
    )]
    pub(super) fn is_front(&self) -> bool {
        *self == Self::Front
    }
}

/// A manual marker's stable stack identity and its source face as seen when it was placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ManualAbility {
    /// The number used by the marker's ability subject in animation scripts.
    pub id: u32,
    /// None when the viewer does not know the source.
    pub face: Option<FaceName>,
}

/// A counter with a player-supplied name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NamedCounter {
    pub name: String,
    pub count: i32,
}

/// The current phase on the manual table's turn track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TablePhase {
    Untap,
    Upkeep,
    Draw,
    FirstMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndOfCombat,
    SecondMain,
    End,
}

/// A zone belonging to a player, relative to the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ZoneRef {
    pub side: Side,
    pub zone: ZoneName,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub n: u32,
    /// The actor's display name as seen by the viewer.
    pub by: String,
    pub text: String,
    pub struck: bool,
    pub undoable: bool,
    pub blocked_by: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProposalView {
    pub id: u32,
    pub title: String,
    pub lines: Vec<ProposalLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProposalLine {
    pub text: String,
    pub whose: Side,
    pub on: bool,
}

/// A state nudge. The host uses its id to look up the action when the viewer clicks it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LabelView {
    pub id: u32,
    pub on: TargetRef,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each assist has its own switch"
)]
pub struct AssistsView {
    pub turn_upkeep: bool,
    pub combat: bool,
    pub state_nudges: bool,
    pub library_flows: bool,
    pub ability_menus: bool,
}

/// The wire tag reserved for a host-held prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TablePrompt {
    None,
}

/// A player's ordered slots for the zones whose order is specific to manual play.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManualPiles {
    /// Left to right.
    pub hand: Vec<ObjectId>,
    /// Bottom first.
    pub exile: Vec<ObjectId>,
    /// Bottom first.
    pub sideboard: Vec<ObjectId>,
}

impl ManualPiles {
    pub(crate) fn order(&self, zone: ZoneName) -> Option<&Vec<ObjectId>> {
        match zone {
            ZoneName::Hand => Some(&self.hand),
            ZoneName::Exile => Some(&self.exile),
            ZoneName::Sideboard => Some(&self.sideboard),
            _ => None,
        }
    }

    pub(crate) fn order_mut(&mut self, zone: ZoneName) -> Option<&mut Vec<ObjectId>> {
        match zone {
            ZoneName::Hand => Some(&mut self.hand),
            ZoneName::Exile => Some(&mut self.exile),
            ZoneName::Sideboard => Some(&mut self.sideboard),
            _ => None,
        }
    }
}

/// Ordered slots by the side whose zone contains them, relative to the viewer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManualOrders {
    pub you: ManualPiles,
    pub opponent: ManualPiles,
}

/// A manual arrow between subjects in the viewer's current layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TableArrow {
    pub arrow: Arrow,
    pub from: Subject,
    pub to: Subject,
    // Whether the viewer drew this arrow and can remove it.
    pub own: bool,
}

/// What the viewer can see or use on a manual table in addition to the shared game view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TableView {
    pub log: Vec<LogLine>,
    pub proposals: Vec<ProposalView>,
    pub labels: Vec<LabelView>,
    pub assists: AssistsView,
    pub phase: TablePhase,
    pub seats: u8,
    /// The viewer's absolute seat, omitted for seat zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub seat: Option<u8>,
    pub orders: ManualOrders,
    /// Hidden zones lent or revealed to this viewer.
    pub acting: Vec<ZoneRef>,
    /// The opposing hand was lent to the viewer as a zone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<_>")]
    pub lent_hand: bool,
    /// References for actionable objects and stack markers shown in this view.
    pub object_actions: Vec<TableObjectAction>,
    /// The viewer's library has Keep top card revealed enabled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<_>")]
    pub top_revealed: bool,
    /// The viewer keeps looking at the top of their library.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<_>")]
    pub top_known: bool,
    /// Visible slots whose power and toughness were set by a player.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<_>")]
    pub manual_stats: Vec<super::ObjectId>,
    /// The viewer's latest draw, even when another entry blocks undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_draw: Option<u32>,
    /// The viewer's deck tokens, in deck list order, for the ten numbered shortcuts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<_>")]
    pub deck_tokens: Vec<crate::ipc::decks::CardTokenLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prompt: Option<TablePrompt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<_>")]
    pub arrows: Vec<TableArrow>,
}
