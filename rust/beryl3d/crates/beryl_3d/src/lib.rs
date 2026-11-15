//! Beryl 3D's client: the crates that make up the native 3D app, and the app itself
//! (`beryl3d`, in `main.rs`).

pub mod app;
#[cfg(feature = "tools")]
pub mod fixture;
pub mod frame_log;
pub mod options;
pub mod session;
