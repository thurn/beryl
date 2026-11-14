//! Wire data for a manual table, projected for one viewer.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Side, TargetRef, ZoneName};

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

/// A counter with a player-supplied name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NamedCounter {
    pub name: String,
    pub count: u16,
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
    /// Hidden zones lent or revealed to this viewer.
    pub acting: Vec<ZoneRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prompt: Option<TablePrompt>,
}
