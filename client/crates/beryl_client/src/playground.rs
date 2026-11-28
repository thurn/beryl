//! The card playground of a tools build: the session thread's side of the testing screen's
//! card field and of the playground dock. It finds the card a person types, lists the card's
//! playground examples, and sets up the game of one of them, which `Live` starts in its own
//! sessions so that its views reach the client as an ordinary game's do
//! (`beryl_cli::specs::playground`).

use app_screens::screens::testing::playground::{Card, Example};
use beryl_cli::Catalog;
use beryl_cli::specs::{self, playground};
use beryl_session::Prepared;

/// The cards the playground's games are played with, and the scenarios their examples are set
/// up from.
static CATALOG: Catalog = Catalog {
    cards: &beryl_cards::CARDS,
    scenarios: beryl_cards::scenarios,
    library_scenarios: || beryl_cards::library::by_card().collect(),
    derived_scenarios: |def| beryl_cards::derive::for_card(def).scenarios,
    fuzz_deck_from: beryl_cards::decks::fuzz_deck_from,
    keystone_family: beryl_cards::decks::keystone_family,
};

/// The name of the implemented card `text` names most nearly (`specs::closest`).
#[must_use]
pub fn find(text: &str) -> Option<String> {
    specs::closest(&CATALOG, text).map(|spec| spec.card.to_owned())
}

/// The playground examples of the card named `card`, every variant of each, made the first
/// time they are asked for, which plays each one for a moment.
///
/// # Errors
///
/// When no implemented card has that name.
pub fn examples(card: &str) -> Result<Card, String> {
    let spec = specs::spec(&CATALOG, card).ok_or_else(|| format!("No card named {card}"))?;
    let made = spec.playground();
    let examples = (made.examples.iter())
        .flat_map(|example| {
            example.variants.iter().map(|variant| Example {
                title: example.title.clone(),
                label: variant.label.clone(),
                name: variant.moment.name.to_owned(),
            })
        })
        .collect();
    Ok(Card {
        name: spec.card.to_owned(),
        slug: spec.slug.clone(),
        examples,
        missing: made.missing.clone(),
    })
}

/// The game of the variant `variant` of the card whose slug is `card`, set up to its first
/// live decision and seen from the seat of the player whose action it shows.
///
/// # Errors
///
/// When the card has no such variant, or its game cannot be set up.
pub fn prepared(card: &str, variant: &str) -> Result<Prepared, String> {
    let found = playground::find(&CATALOG, card, variant)
        .ok_or_else(|| format!("{card} has no example {variant}"))?;
    playground::prepared(&CATALOG, found, found.controller)
}
