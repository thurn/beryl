//! The hub's HTTP server, on `tiny_http`, with one thread per connection and its own database
//! connection per thread.
//!
//! Stub. The ticket Thrh4.16 (Hub ticket API) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;
use std::path::PathBuf;

use ignite_core::config::Config;

/// Where and how the hub serves.
#[derive(Clone, Debug)]
pub struct HubServer {
    pub config: Config,
    /// The address to listen on, such as `0.0.0.0:8443`.
    pub listen: String,
    pub database: PathBuf,
}

impl HubServer {
    /// Serves until the process ends.
    pub fn serve(&self) -> io::Result<()> {
        todo!("Thrh4.16 (Hub ticket API)")
    }
}
