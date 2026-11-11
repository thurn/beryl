//! The boards UI fixtures start from: each a scenario's setup, and the actions that play it
//! into the moment its fixture shows. A [`Scripted`] agent in each seat takes that seat's
//! actions in order as the session asks its decisions, so a fixture built on a board shows the
//! same game however the agents are tuned or the decks change.

mod choices;
mod combat;
mod script;
mod shorthand;
mod stack;
mod table;
mod tables;
mod targets;

use beryl_scenario::Scenario;

pub use self::script::{Scripted, Shared, lock};

/// Every board, by name, in groups.
pub const BOARDS: &[&[Scenario]] = &[
    combat::COMBAT,
    stack::STACK,
    targets::TARGETS,
    choices::CHOICES,
    table::TABLE,
];

/// The board named `name`.
pub fn named(name: &str) -> Result<&'static Scenario, String> {
    BOARDS
        .iter()
        .flat_map(|group| group.iter())
        .find(|b| b.name == name)
        .ok_or_else(|| format!("no board is named {name:?}"))
}
