//! The integration tests of `beryl_client`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

#[path = "../../../../test_data.rs"]
mod test_data;

mod app;
mod audio;
mod bugs;
mod drive;
mod ffi;
mod fixture;
mod frame_log;
mod live;
mod playground;
mod session;
mod settings;
