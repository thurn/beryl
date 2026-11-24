//! The `PlayerView` protocol (architecture §19.2, decision D14): what the UI draws. The
//! session builds each view from one player's observation of the game, so it carries nothing
//! that player does not know.

mod decision;
mod event;
mod names;
mod neutral;
mod outcome;
mod shown;
mod stops;
mod table;
mod view;

pub use decision::*;
pub use event::*;
pub use names::*;
pub use neutral::MarkerPreview;
pub use outcome::*;
pub use stops::*;
pub use table::*;
pub use view::*;
