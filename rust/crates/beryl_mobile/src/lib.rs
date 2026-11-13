//! Native `WebView` snapshots and device details for the phone apps.

#![cfg(mobile)]

mod mobile;

pub use mobile::{Device, Mobile, init};
