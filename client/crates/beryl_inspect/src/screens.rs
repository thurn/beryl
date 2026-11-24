//! Names kept for the screen tools and their reference data.

/// Legacy recording names and their feature stories.
pub(crate) const ALIASES: [(&str, &str); 14] = [
    ("screen/start", "story/app_screens/start"),
    ("screen/start-tables", "story/app_screens/start-tables"),
    ("screen/lobby", "story/game_manual/lobby"),
    ("screen/table", "story/game_manual/table"),
    ("screen/table-assists", "story/game_manual/table-assists"),
    ("screen/table-first", "story/game_manual/table-first"),
    ("screen/table-opening", "story/game_manual/table-opening"),
    ("screen/table-left", "story/game_manual/table-left"),
    ("screen/table-search", "story/game_manual/table-search"),
    (
        "screen/table-pile-scroll",
        "story/game_manual/table-pile-scroll",
    ),
    ("screen/gallery", "story/app_screens/gallery"),
    ("screen/testing", "story/app_screens/testing"),
    ("screen/settings", "story/app_screens/settings"),
    ("screen/bug-report", "story/app_screens/bug-report"),
];
