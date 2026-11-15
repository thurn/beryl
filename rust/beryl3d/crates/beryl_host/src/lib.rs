//! Beryl 3D's host: the window and event loop that drive the client and render its frames
//! (docs/beryl-3d.md, "Hosts"). `frames` is the loop every host runs, apart from any window;
//! `desktop` is the winit host for macOS and Windows.

mod desktop;
mod events;
mod frames;
#[cfg(target_os = "macos")]
mod macos;

pub use desktop::{Desktop, Inbox, Options, WindowMode, is_fullscreen_key};
pub use events::{LINE_POINTS, Translator, modifiers_from};
pub use frames::{Draw, FrameLoop, Sleep, Ticks};
