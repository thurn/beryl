//! Ignite's pure logic. Nothing in this crate does I/O beyond reading the system clock in
//! [`time::SystemClock`]: every other crate brings the files, processes and network.

pub mod admission;
pub mod config;
pub mod dispatch;
pub mod explore;
pub mod fakes;
pub mod gate;
pub mod guide;
pub mod hash;
pub mod ids;
pub mod inputs;
pub mod manifest;
pub mod output;
pub mod prime;
pub mod protocol;
pub mod ready;
pub mod score;
pub mod shares;
pub mod time;
pub mod tipround;
pub mod traits;
pub mod verdict;
