//! Commands for opening and configuring linked manual tables.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const TABLE_HOST: &str = "table_host";
pub const TABLE_JOIN: &str = "table_join";
pub const TABLE_RESUME: &str = "table_resume";
pub const TABLE_SAVED: &str = "table_saved";
pub const TABLE_SETTINGS_GET: &str = "table_settings_get";
pub const TABLE_SETTINGS_SET: &str = "table_settings_set";
pub const TABLE_LINK: &str = "table-link";
pub const TABLE_LOBBY: &str = "table-lobby";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TableLink {
    Iroh,
    Local,
    /// Available only in the local HTTP test server.
    Memory,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TableHostSettings {
    pub name: String,
    #[ts(optional)]
    pub starting_life: Option<i32>,
    #[ts(optional)]
    pub starting_hand_size: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableHostRequest {
    pub link: TableLink,
    pub settings: TableHostSettings,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableHostReply {
    pub invite: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableJoinRequest {
    pub invite: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableResumeRequest {
    pub id: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SavedTable {
    pub id: String,
    pub host: String,
    pub guest: Option<String>,
}

/// Profile settings for linked tables. A null relay selects iroh's public relays.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TableSettings {
    pub relay: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableConnectionError {
    pub reason: String,
}

impl TableConnectionError {
    pub fn from_error(error: impl std::fmt::Display) -> Self {
        Self {
            reason: error.to_string(),
        }
    }
}
