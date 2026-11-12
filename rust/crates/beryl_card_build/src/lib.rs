//! What a card crate's build script runs (plan K8: "Card files are grouped by first-printing
//! set, in many crates, with registries generated at build time").
//!
//! A card's file is `rust/cards/<crate>/<set>/<card>.rs`: the crate is its first-printing set's
//! group in `data/set-groups.json`, and the two modules are the set's code and the card's name
//! spelled as [`names::set_module`] and [`names::card_module`] spell them. Nothing lists the
//! files by hand: a set crate's `build.rs` calls [`set_crate`], which writes the crate's module
//! tree, its tables and the name constants its files use, and `beryl_cards`' calls
//! [`registry`], which writes the tables over every set crate and `beryl_cards::names`. A new
//! card is one new file.
//!
//! Names come from the card store, `data/card-store.tsv` ([`store`]): a file is found there by
//! its set and module, so a misspelled file or constant fails the build, and no crate compiles
//! a constant it does not use.

pub mod files;
mod generate;
pub mod names;
pub mod store;

pub use generate::{registry, registry_source, set_crate, set_crate_source};

#[cfg(test)]
mod tests;
