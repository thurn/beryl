//! The client routes and sheets named by navigation controls.

/// A screen of the client: the web interface's routes (`Main` in `ui/src/App.tsx`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The start screen, where a game is started.
    Start,
    /// A game, from its opening hand to its end.
    Game,
    /// The manual table's lobby, where a table's decks, assists and settings are chosen
    /// before a game.
    Lobby,
    /// The manual table, where both players move cards by hand.
    Table,
    /// The card gallery.
    Gallery,
    /// The testing screen.
    Testing,
}

impl Screen {
    pub const ALL: [Self; 6] = [
        Self::Start,
        Self::Game,
        Self::Lobby,
        Self::Table,
        Self::Gallery,
        Self::Testing,
    ];

    /// The screen's name, as `data/beryl3d/screens.json` writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Game => "game",
            Self::Lobby => "lobby",
            Self::Table => "table",
            Self::Gallery => "gallery",
            Self::Testing => "testing",
        }
    }
}

/// A sheet: what opens over a screen and closes again without leaving it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    Settings,
    BugReport,
}

impl Sheet {
    pub const ALL: [Self; 2] = [Self::Settings, Self::BugReport];

    /// The sheet's name, as `data/beryl3d/screens.json` writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::BugReport => "bug-report",
        }
    }
}
