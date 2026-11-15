use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub wire: String,
    pub secret: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableStatus {
    New,
    Resumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    Permission,
    Stale,
    Invalid { blocking: Vec<u32> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    Hello(Hello),
    Welcome {
        seat: u8,
        table: TableStatus,
        host: String,
    },
    Mismatch {
        reason: String,
    },
    Act {
        n: u64,
        action: Value,
    },
    Done {
        n: u64,
    },
    Refused {
        n: u64,
        reason: RefusalReason,
    },
    Update(Value),
    Lobby(Value),
    Leave,
    Ping {
        n: u64,
    },
    Pong {
        n: u64,
    },
}
