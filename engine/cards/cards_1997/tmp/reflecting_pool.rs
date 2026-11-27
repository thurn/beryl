// Generated from the card store by `just card header`: notes go below the end marker.
//! Reflecting Pool
//! Land
//!
//! {T}: Add one mana of any type that a land you control could produce.
// End of the generated card header.

use crate::prelude::*;

static LAND_YOU_CONTROL_PARTS: &[Filter] =
    &[Filter::Types(Types::LAND), Filter::Whose(&Players::You)];
static LAND_YOU_CONTROL: Filter = Filter::All(LAND_YOU_CONTROL_PARTS);
static TERMS: [Term; 1] = [Term::could_produce(&LAND_YOU_CONTROL)];

pub static CARD: CardDef = Printed::ReflectingPool
    .abilities(&[Ability::mana_of(Cost::TAP, &TERMS)])
    .card();
