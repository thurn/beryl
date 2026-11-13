//! The cards first printed in 2000: one file per card, `<set>/<card>.rs`, in the sets
//! `data/set-groups.json` gives this crate. A new card is a new file and nothing else: the
//! build script (`beryl_card_build`) writes the modules, `CARDS`, `SCENARIOS` and the `names`
//! the files use, and `STORE_CARDS`, the cards of these sets complete from the card store, which
//! have no file. `just card crates` writes this crate.

include!(concat!(env!("OUT_DIR"), "/cards.rs"));
