//! The widget set of the flat layer (docs/beryl-3d.md, "The flat layer"): one function per
//! file, each returning an element with the role and name the agent interface finds it by.
//! Screens build themselves from these, so a widget looks and behaves the same everywhere.

mod button;
mod field;
mod icon_button;
pub mod icons;
mod list;
mod menu;
mod slider;
mod toggle;
mod tooltip;

pub use button::{button, primary};
pub use field::{FieldState, Fields, field};
pub use icon_button::icon_button;
pub use icons::IconId;
pub use list::list;
pub use menu::{BACK, MenuGroup, MenuId, MenuRow, context_menu, menu, sheet_menu};
pub use slider::{STEP, level_at, slider};
pub use toggle::toggle;
pub use tooltip::tooltip;
