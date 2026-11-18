//! The hub: the daemon on one machine also serves the shared database over HTTP, on the private
//! network, to every machine's daemon and to people through the dashboard.
//!
//! - [`server`] accepts connections and hands each request to [`routes`] or [`dashboard`].
//! - [`auth`] checks the write token and the actor on every write.
//! - [`rules`] find sources of waste in the records and file improvement tickets.
//! - [`upkeep`] files regression and flaky-check tickets and frees stuck work.

pub mod auth;
pub mod dashboard;
pub mod routes;
pub mod rules;
pub mod server;
pub mod upkeep;
