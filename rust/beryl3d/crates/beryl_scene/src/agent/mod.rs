//! What the agent tools and the interface checks read from a frame (docs/beryl-3d.md, "The
//! agent interface" and "Interface checks"): the tree printout, locators, hit testing and the
//! checks. Every function here is a function of frames and options alone; nothing here plays
//! a recording or runs the client.

pub mod checks;
pub mod fault;
pub mod hit;
pub mod judged;
pub mod locate;
pub mod steps;
pub mod text_overlap;
pub mod tree;

pub use checks::check;
pub use fault::{Context, Fault, Layout, Rule, Subject};
pub use hit::{hit, uncovered};
pub use judged::{Count, Judged};
pub use locate::{LocateError, locate};
pub use tree::{TreeOptions, tree};
