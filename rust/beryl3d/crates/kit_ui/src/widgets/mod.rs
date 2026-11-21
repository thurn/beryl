//! The widget set of the flat layer (docs/beryl-3d.md, "The flat layer"): one function per
//! file, each returning an element with the role and name the agent interface finds it by.
//! Screens build themselves from these, so a widget looks and behaves the same everywhere.

mod button;
mod check;
mod field;
mod icon_button;
pub mod icon_shadows;
pub mod icons;
mod list;
mod menu;
/// Panels and actions that fill the window or its safe area.
pub mod screen;
mod slider;
/// The panel and controls shared by the start and lobby screens.
pub mod start_panel;
mod toggle;
mod tooltip;

pub use button::{button, pass_on, primary};
pub use check::check_mark;
pub use field::{FieldState, Fields, field};
pub use icon_button::icon_button;
pub use icons::IconId;
pub use list::list;
pub use menu::{MenuGroup, MenuPage, MenuRow, context_menu, menu, sheet_menu};
pub use slider::{STEP, level_at, slider};
pub use toggle::toggle;
pub use tooltip::{tip_label, tooltip};
