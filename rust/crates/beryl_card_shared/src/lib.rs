//! What card files share: the [`tokens`] more than one card creates and the [`props`] any
//! card's scenarios may name. It sits below the set crates (`rust/cards`), which re-export both,
//! so a card file reaches them by `crate::` and no card file reaches another's. Its tokens start
//! from the printed characteristics its build script generates from the card store.

pub mod props;
pub use props::Prop;
pub mod tokens;

include!(concat!(env!("OUT_DIR"), "/printed.rs"));
