//! The integration tests of `beryl_3d`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

mod app;
mod audio;
mod bugs;
mod drive;
mod ffi;
mod fixture;
mod frame_log;
mod live;
mod session;
mod settings;
