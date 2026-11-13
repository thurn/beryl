//! Every card name as a constant, so a misspelled card is a compile error: the cards of the
//! decklists in `docs/sample-decks`, the basic lands, every card with a file and each card
//! complete from the store that `beryl_cards`' own sources name. `beryl_cards` re-exports this
//! crate as `beryl_cards::names`.
//!
//! It is a crate of its own, depending on no set crate, so that an edit to one card file
//! rebuilds it beside that card's set crate instead of after it
//! (`beryl_card_build::card_names`).

include!(concat!(env!("OUT_DIR"), "/names.rs"));
