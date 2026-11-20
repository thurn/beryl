//! Ignite's pure logic. Nothing in this crate does I/O beyond reading the system clock in
//! [`time::SystemClock`]: every other crate brings the files, processes and network.

pub mod admission;
pub mod budget;
pub mod config;
pub mod dispatch;
pub mod explore;
pub mod fakes;
pub mod formatting;
pub mod gate;
pub mod guide;
pub mod hash;
pub mod host;
pub mod ids;
pub mod inputs;
pub mod manifest;
pub mod output;
pub mod owner;
pub mod plan_usage;
pub mod prime;
pub mod protocol;
pub mod ready;
pub mod rust_source;
pub mod schedule;
pub mod score;
pub mod shares;
pub mod test_files;
pub mod time;
pub mod tipround;
pub mod traits;
pub mod verdict;

pub use protocol::hub::verdict_settings;
