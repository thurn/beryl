//! Beryl 3D's host: the window and event loop that drive the client and render its frames
//! (docs/beryl-3d.md, "Hosts"). `frames` is the loop every host runs, apart from any window;
//! `winit_host` uses winit for macOS and Windows, and `windowless` draws into an offscreen
//! target on a timer, for runs with no window, and `headless` (behind its feature) draws the
//! frames of a recording at moments an agent asks for. `drive` (behind its feature) plays a
//! script of actions on the display's clock. The Swift and Kotlin apps use `native_surface`
//! through `beryl_3d`'s C functions.

mod assets;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod clipboard;
mod data_watch;
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
pub mod native_surface;
mod performance;
pub mod rate;
mod threads;
mod windowless;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod winit_host;

pub use assets::{Assets, Scans, match_faces, spawn_glyph_worker};
pub use data_watch::{DataWatch, WatchingData};
#[cfg(feature = "drive")]
pub use drive::Drive;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub use events::{Translator, modifiers_from};
pub use frames::{
    Chosen, Draw, Feed, FrameLoop, NoFeed, Opened, Opening, Shown, Sleep, Ticked, Ticks, When,
};
#[cfg(feature = "headless")]
pub use headless::{Headless, HeadlessPage};
pub use inbox::Inbox;
pub use native_surface::{NativeSurface, Wakeup};
pub use threads::{
    Class, asset_thread, current_class, interactive_thread, set_class, utility_thread,
};
pub use windowless::{Windowless, WindowlessOptions};
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub use winit_host::{Options, WindowMode, WinitHost, is_fullscreen_key};
