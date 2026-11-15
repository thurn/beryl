//! Beryl 3D's host: the window and event loop that drive the client and render its frames
//! (docs/beryl-3d.md, "Hosts"). `frames` is the loop every host runs, apart from any window;
//! `desktop` is the winit host for macOS and Windows, and `windowless` draws into an offscreen
//! target on a timer, for runs with no window.

mod desktop;
mod events;
mod frames;
mod inbox;
#[cfg(target_os = "macos")]
mod macos;
mod threads;
mod windowless;

pub use desktop::{Desktop, Options, WindowMode, is_fullscreen_key};
pub use events::{LINE_POINTS, Translator, modifiers_from};
pub use frames::{Draw, Feed, FrameLoop, NoFeed, Opened, Shown, Sleep, Ticked, Ticks};
pub use inbox::Inbox;
pub use threads::{interactive_thread, utility_thread};
pub use windowless::{Windowless, WindowlessOptions};
