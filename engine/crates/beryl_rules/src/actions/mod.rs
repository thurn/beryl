//! Keyword actions (see [CR 701](https://yawgatog.com/resources/magic-rules/#R701)), one module
//! for each, named as the rules name it.
//!
//! Every file here has the same layout, and a new keyword action copies it:
//!
//! 1. The header names the action, links its rule and declares `Implements:` for the
//!    subrules the file carries out.
//! 2. The action's function, with the action's name (`scry::scry`, `destroy::destroy`). Its
//!    rustdoc quotes the rule it implements. It takes `&mut Game` and the typed reference of
//!    what it acts on: `Permanent` or `Permanents` for an action on the battlefield, `Spell`
//!    for one on a spell, `CardIn` for a card in one zone, and a slot set only where the
//!    objects may be in any zone, as exile's are. It rechecks a reference it may have been
//!    handed from an earlier step (`State::permanent`, `State::current`) and then does
//!    nothing to an object that has changed zones. It takes a `Resolution` only when the
//!    action asks a player a question or reads what is resolving (`scry`, `explore`), and
//!    returns `Result<_, Full>` when it cannot ask, `Result<_, Stop>` or `Flow` when it can.
//!    A simultaneous version for a set (`destroy_all`, `sacrifice_all`) moves the set as
//!    one event.
//! 3. The interpreter's arm, `pub(crate) fn instruction` (or `<variant>_instruction` where the
//!    file has several), named in the table in `interpret/run.rs`. It reads the effect's
//!    operands against the scene, calls the action's function for each object or player in
//!    the order the rules give, and returns the `Outcome` later instructions read ("those
//!    cards", "if you do").
//!
//! Implements: none. Groups keyword actions and coin flips.

pub mod attach;
pub mod coins;
pub mod counter;
pub mod destroy;
pub mod discard;
pub mod exile;
pub mod explore;
pub mod fateseal;
pub mod flip;
pub mod manifest;
pub mod manifest_dread;
pub mod meld;
pub mod mill;
pub mod regenerate;
pub mod reveal;
pub mod sacrifice;
pub mod scry;
pub mod search;
pub mod shuffle;
pub mod tap;
pub mod transform;
