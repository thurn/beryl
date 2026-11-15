//! Beryl 3D's host: the window and event loop that drive the client and render its frames
//! (docs/beryl-3d.md, "Hosts"). `frames` is the loop every host runs, apart from any window;
//! `desktop` is the winit host for macOS and Windows, and `windowless` draws into an offscreen
//! target on a timer, for runs with no window, and `headless` (behind its feature) draws the
//! frames of a recording at moments an agent asks for. `drive` (behind its feature) plays a
//! script of actions on the display's clock. `phone` is the part of the iOS and Android hosts
//! that is Rust: their Swift and Kotlin apps call it through `beryl_3d`'s C functions.

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod desktop;
#[cfg(feature = "drive")]
mod drive;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod events;
mod frames;
#[cfg(feature = "headless")]
mod headless;
mod inbox;
#[cfg(target_os = "macos")]
mod macos;
mod phone;
pub mod rate;
mod threads;
mod windowless;

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub use desktop::{Desktop, Options, WindowMode, is_fullscreen_key};
#[cfg(feature = "drive")]
pub use drive::Drive;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub use events::{LINE_POINTS, Translator, modifiers_from};
pub use frames::{Draw, Feed, FrameLoop, NoFeed, Opened, Shown, Sleep, Ticked, Ticks};
#[cfg(feature = "headless")]
pub use headless::{Headless, Scans};
pub use inbox::Inbox;
pub use phone::Phone;
pub use threads::{Class, current_class, interactive_thread, set_class, utility_thread};
pub use windowless::{Windowless, WindowlessOptions};
