//! The legacy stress board names, and how the tools exercise the corresponding stories.

use beryl_protocol::PlayerView;
use beryl_protocol::script::StepTiming;
use game_probe::story::Body;
use kit_probe::agent::Rule;

use crate::stories;

mod motion;
pub(crate) use motion::pointer_moves;

/// A synthetic board and any scripted changes to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stress {
    /// The first view is a reset with every object at rest. Each later view carries a script.
    pub views: Vec<PlayerView>,
    /// For each view after the first, one timing per step of its script, in script order:
    /// `timings[i]` belongs to `views[i + 1]`, whose script carries the same timings as a
    /// session's does.
    pub timings: Vec<Vec<StepTiming>>,
    /// What a page or a playback does besides handing over the views.
    pub drive: Drive,
    /// The rules the interface checks do not hold this board to.
    pub unjudged: &'static [Rule],
}

/// What a page or a playback of a stress board does besides handing over its views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drive {
    /// Moves the pointer over the viewer's hand, as `pointer_moves` lays out.
    pub pointer_over_hand: bool,
    /// Waits this many frames, once the table is at rest, before handing over the last view.
    pub rest_before_last: Option<usize>,
}

/// A stress board: the one place that maps a board's name to the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Board {
    /// 255 permanents.
    S255,
    /// 1024 permanents.
    S1024,
    /// Every verb at once.
    Verbs,
    /// Sixty cards in hand under a moving pointer.
    Hand,
    /// A full stack of spells and abilities.
    Stack,
}

impl Board {
    /// Every stress board, in the order they are listed and played.
    pub const ALL: [Self; 5] = [
        Self::S255,
        Self::S1024,
        Self::Verbs,
        Self::Hand,
        Self::Stack,
    ];

    /// The board's recording name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.names().0
    }

    pub(crate) const fn story(self) -> &'static str {
        self.names().1
    }

    const fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::S255 => ("stress-255", "story/game_table/stress-255"),
            Self::S1024 => ("stress-1024", "story/game_table/stress-1024"),
            Self::Verbs => ("stress-verbs", "story/game_table/stress-verbs"),
            Self::Hand => ("stress-hand", "story/game_table/stress-hand"),
            Self::Stack => ("stress-stack", "story/game_table/stress-stack"),
        }
    }

    /// The board called `name`, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|board| board.name() == name || board.story() == name)
    }

    /// How a page or a playback drives the board.
    #[must_use]
    pub const fn drive(self) -> Drive {
        match self {
            Self::Hand => Drive {
                pointer_over_hand: true,
                rest_before_last: None,
            },
            Self::Stack => Drive {
                pointer_over_hand: false,
                // Hold the full stack for two seconds before resolving the top ability.
                rest_before_last: Some(120),
            },
            Self::S255 | Self::S1024 | Self::Verbs => Drive {
                pointer_over_hand: false,
                rest_before_last: None,
            },
        }
    }

    /// The rules the interface checks do not hold the board to. `Verbs` starts every verb's acts
    /// on 100 objects in one beat with no stagger, so that the frame gates draw every verb in
    /// the same frames. No game plays a beat like that, and its steps crowd and cover each other
    /// on purpose, so "not crowded" and "every step shown" do not judge it (docs/beryl-3d.md,
    /// "What the checks run over"). Every other check still runs on it.
    #[must_use]
    pub const fn unjudged(self) -> &'static [Rule] {
        match self {
            Self::Verbs => &[Rule::Crowded, Rule::StepShown],
            _ => &[],
        }
    }

    /// The board's views and timings.
    ///
    /// # Panics
    ///
    /// When the board's story cannot be read.
    #[must_use]
    pub fn build(self) -> Stress {
        let story = stories::open(self.story()).unwrap_or_else(|why| panic!("{why}"));
        let stories::Story::Game(game_probe::story::Story {
            body: Body::Table(table),
            ..
        }) = story.as_ref()
        else {
            panic!("{} must be a table story", self.story());
        };
        Stress {
            views: table.views.clone(),
            timings: table
                .views
                .iter()
                .skip(1)
                .map(|view| {
                    view.script
                        .as_ref()
                        .expect("a later view carries a script")
                        .timing
                        .clone()
                })
                .collect(),
            drive: self.drive(),
            unjudged: self.unjudged(),
        }
    }
}

/// Whether the interface checks hold the recording `recording` to `rule`.
#[must_use]
pub fn audited(recording: &str, rule: Rule) -> bool {
    Board::parse(recording).is_none_or(|board| !board.unjudged().contains(&rule))
}

/// The synthetic recording called `name`, if there is one.
#[must_use]
pub fn stress(name: &str) -> Option<Stress> {
    Board::parse(name).map(Board::build)
}
