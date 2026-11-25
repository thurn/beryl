// Generated from the card store by `just card header`: notes go below the end marker.
//! Blood-Toll Harpy {2}{B}
//! Creature — Harpy, 2/1
//!
//! Flying
//! When this creature enters, each player loses 1 life.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::BloodTollHarpy
    .keywords(Keywords::FLYING)
    .abilities(&[when_enters(&lose_life(each_player(), 1))])
    .card();
