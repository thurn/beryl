//! Beryl 3D's client: the crates that make up the native 3D app, and the app itself
//! (`beryl3d`, in `main.rs`). On the phones it is a library whose C functions (`ffi`) the Swift
//! and Kotlin apps call.

pub mod app;
pub mod ffi;
pub mod fixture;
pub mod frame_log;
pub mod load;
pub mod options;
#[cfg(feature = "tools")]
pub mod pictures;
pub mod session;
