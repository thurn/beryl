//! Recording playback timing, input and resolved game elements.

mod element;
mod recording;

pub use element::Element;
pub use kit_probe::driver::{
    Action, Duration, KeyChord, Moment, Name, Offset, ParseError, key_code, point,
};
pub use recording::{OPEN, RECORDINGS, plays_on};

/// How long after the table comes to rest a recorded reply is given.
pub const REPLY_DELAY: Duration = Duration::from_millis(400);
/// A tick of the test's clock unless the test sets another: a sixtieth of a second.
pub const TICK: Duration = Duration(16_666_667);
