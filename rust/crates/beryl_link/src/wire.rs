//! JSON envelopes for the table protocol. Actions and views belong to the caller.

use std::io::Write;

use flate2::{Compression, Decompress, FlushDecompress, Status, write::DeflateEncoder};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::LinkError;

pub const VERSION_MISMATCH: &str = "Both players need the same version of Beryl.";

// Bound expansion independently of the compressed frame length. Whole views
// usually compress well, so allow them to be larger than a frame before encoding.
const MAX_JSON_SIZE: usize = 32 * 1024 * 1024;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    Permission,
    Stale,
    Invalid,
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

pub fn encode(message: &Message) -> Vec<u8> {
    compress(&json(message))
}

pub(crate) fn encode_frame(message: &Message) -> Result<Vec<u8>, LinkError> {
    let json = json(message);
    check_json_size(json.len())?;
    let frame = compress(&json);
    crate::frame_length(frame.len())?;
    Ok(frame)
}

fn json(message: &Message) -> Vec<u8> {
    serde_json::to_vec(message).expect("serialize wire message to memory")
}

fn compress(json: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(json)
        .expect("compress wire message to memory");
    encoder.finish().expect("finish wire message in memory")
}

fn check_json_size(len: usize) -> Result<(), LinkError> {
    if len > MAX_JSON_SIZE {
        Err(LinkError::Refused(
            "The JSON message exceeds 32 MiB.".into(),
        ))
    } else {
        Ok(())
    }
}

pub fn decode(frame: &[u8]) -> Result<Message, LinkError> {
    crate::frame_length(frame.len())?;
    let mut json = Vec::with_capacity(frame.len().max(1));
    let mut decoder = Decompress::new(false);
    let mut consumed = 0;
    loop {
        let before = json.len();
        let status = decoder
            .decompress_vec(&frame[consumed..], &mut json, FlushDecompress::None)
            .map_err(|error| LinkError::Refused(format!("Invalid compressed message: {error}")))?;
        check_json_size(json.len())?;
        let next = decoder.total_in() as usize;
        if status == Status::StreamEnd && next == frame.len() {
            break;
        }
        if status == Status::StreamEnd || (next == consumed && json.len() == before) {
            return Err(LinkError::Refused(
                "Invalid compressed message length.".into(),
            ));
        }
        consumed = next;
        if json.len() == json.capacity() {
            let capacity = (2 * json.capacity()).min(MAX_JSON_SIZE + 1);
            json.reserve_exact(capacity - json.len());
        }
    }
    serde_json::from_slice(&json)
        .map_err(|error| LinkError::Refused(format!("Invalid wire message: {error}")))
}
