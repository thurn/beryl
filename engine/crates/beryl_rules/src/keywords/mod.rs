//! Keyword abilities (see [CR 702](https://yawgatog.com/resources/magic-rules/#R702)), one
//! module for each, named as the rules name it.
//!
//! A keyword ability lives beside the procedure it changes where there is one (protection in
//! `replacement`, mutate in `objects`, delve in `casting`); this directory holds the rest.
//! Each file has the layout of a keyword action's (see `actions`): a header with its rule and
//! `Implements:`, the functions that grant the keyword or answer what it changes, each quoting
//! its rule, then the interpreter's arm for an effect that grants it. A query that the
//! engine asks of a keyword, as `flashback::paid` is asked by casting, takes the state alone.
//!
//! Implements: none. Groups rules modules for keyword abilities.

pub mod flashback;
