//! A card's playground examples as the session lists them: each a game set up from one of the
//! card's scenarios, which a person plays against an automated opponent. The session thread
//! of a tools build makes them (`beryl_cli::specs::playground`); the testing screen lists them,
//! and the dock over the table switches between them (`dock`).

use serde::{Deserialize, Serialize};

/// One card's playground, every variant of every example in the session's order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    /// The card's name: `Counterspell`.
    pub name: String,
    /// The card's slug, which the session finds its examples by: `counterspell`.
    pub slug: String,
    pub examples: Vec<Example>,
    /// Why an example the card should have could not be made, one line each.
    pub missing: Vec<String>,
}

/// One variant of one of a card's examples.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Example {
    /// The example's title, shared by its variants: `Play from hand`.
    pub title: String,
    /// The variant's own label: the heading of the scenario it was set up from.
    pub label: String,
    /// The variant's name, which the session starts it by: `playground-counter-a-spell-0`.
    pub name: String,
}

impl Example {
    /// The words a list shows for it: its title, then its label where that says more.
    #[must_use]
    pub fn text(&self) -> String {
        if self.label == self.title || self.label.is_empty() {
            self.title.clone()
        } else {
            format!("{} · {}", self.title, self.label)
        }
    }
}

impl Card {
    /// The words the dock shows for the example at `index`: the card, the variant's label and
    /// where it is among the card's examples, `Counterspell · Default (1/3)`.
    #[must_use]
    pub fn caption(&self, index: usize) -> String {
        let label = self
            .examples
            .get(index)
            .map_or_else(String::new, Example::text);
        format!(
            "{} · {label} ({}/{})",
            self.name,
            index + 1,
            self.examples.len()
        )
    }
}
