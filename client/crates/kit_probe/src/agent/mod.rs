//! Frame inspection tools for tree printouts, locators and interface checks
//! (docs/beryl-3d.md, "The agent interface" and "Interface checks").
//! Each operation reads only frames and options.

pub mod card_marks;
pub mod checks;
pub mod covers;
pub mod fault;
pub mod judged;
pub mod line;
pub mod locate;
pub mod motion;
pub mod text_overlap;
pub mod tree;

pub use fault::{Context, Fault, Rule, Subject};
pub use judged::{Checked, Count, Judged};
pub use locate::{LocateError, locate};
pub use tree::{TreeOptions, tree};
