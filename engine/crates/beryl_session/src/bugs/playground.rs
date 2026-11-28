//! The playground example a report names.

/// A card's playground example, a game set up from one of its scenarios that a person plays
/// against an automated opponent: the card's slug and the variant's name, as
/// `beryl_cli::specs::playground::find` takes them to start it again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaygroundExample {
    /// The card's slug: `counterspell`.
    pub card: String,
    /// The variant's name: `playground-counter-a-spell-0`.
    pub variant: String,
}

impl std::fmt::Display for PlaygroundExample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.card, self.variant)
    }
}
