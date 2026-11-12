//! The cards first printed from 1993 to 1994: one file per card, `<set>/<card>.rs`, in the sets
//! `data/set-groups.json` gives this crate. A new card is a new file and nothing else: the
//! build script (`beryl_card_build`) writes the modules, `CARDS`, `SCENARIOS` and the `names`
//! the files use.

include!(concat!(env!("OUT_DIR"), "/cards.rs"));
