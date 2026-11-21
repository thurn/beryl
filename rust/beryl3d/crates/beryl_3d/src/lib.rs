//! Beryl 3D's client: the crates that make up the native 3D app, and the app itself
//! (`beryl3d`, in `main.rs`). On the phones it is a library whose C functions (`ffi`) the Swift
//! and Kotlin apps call.

pub mod app;
pub mod audio;
pub mod bugs;
mod data;
pub mod ffi;
pub mod fixture;
pub mod frame_log;
#[cfg(all(feature = "tools", unix))]
pub mod live;
pub mod load;
pub mod options;
mod places;
pub mod session;
pub mod settings;
pub mod tier;
