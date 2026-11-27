//! The names of the slots a layout profile places features in.

use std::fmt;

use serde::{Deserialize, Deserializer};

/// A named slot in a layout profile's `slots` table: the part of the window one feature, such as
/// the log or the prompt bar, is given. Each variant is spelled in the profile files as
/// [`SlotName::as_str`] returns it, and a profile naming any other slot fails to load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotName {
    BugHint,
    BugSheet,
    BugTools,
    DivideEdge,
    DivideSide,
    FullControl,
    Gear,
    Inspector,
    Log,
    LogToggle,
    ManualConnection,
    ManualLog,
    ManualMenu,
    ManualNext,
    ManualPool,
    ManualProposals,
    ManualProposalsPanel,
    ManualProposalsShort,
    Menu,
    Opening,
    Phase,
    PhaseIcon,
    PlayerOpponent,
    PlayerYou,
    PromptAsked,
    PromptBar,
    PromptCatalog,
    PromptClearance,
    PromptDock,
    PromptStacked,
    PromptWays,
    Result,
    ResultDock,
    RevealedPile,
    ScreenColumn,
    ScreenDock,
    SettingsSheet,
    Viewer,
}

impl SlotName {
    /// Every slot name, in the order of their spellings.
    pub const ALL: [Self; 38] = [
        Self::BugHint,
        Self::BugSheet,
        Self::BugTools,
        Self::DivideEdge,
        Self::DivideSide,
        Self::FullControl,
        Self::Gear,
        Self::Inspector,
        Self::Log,
        Self::LogToggle,
        Self::ManualConnection,
        Self::ManualLog,
        Self::ManualMenu,
        Self::ManualNext,
        Self::ManualPool,
        Self::ManualProposals,
        Self::ManualProposalsPanel,
        Self::ManualProposalsShort,
        Self::Menu,
        Self::Opening,
        Self::Phase,
        Self::PhaseIcon,
        Self::PlayerOpponent,
        Self::PlayerYou,
        Self::PromptAsked,
        Self::PromptBar,
        Self::PromptCatalog,
        Self::PromptClearance,
        Self::PromptDock,
        Self::PromptStacked,
        Self::PromptWays,
        Self::Result,
        Self::ResultDock,
        Self::RevealedPile,
        Self::ScreenColumn,
        Self::ScreenDock,
        Self::SettingsSheet,
        Self::Viewer,
    ];

    /// The slot's spelling in the profile files, which inspection output also prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BugHint => "bug.hint",
            Self::BugSheet => "bug.sheet",
            Self::BugTools => "bug.tools",
            Self::DivideEdge => "divide.edge",
            Self::DivideSide => "divide.side",
            Self::FullControl => "full_control",
            Self::Gear => "gear",
            Self::Inspector => "inspector",
            Self::Log => "log",
            Self::LogToggle => "log.toggle",
            Self::ManualConnection => "manual.connection",
            Self::ManualLog => "manual.log",
            Self::ManualMenu => "manual.menu",
            Self::ManualNext => "manual.next",
            Self::ManualPool => "manual.pool",
            Self::ManualProposals => "manual.proposals",
            Self::ManualProposalsPanel => "manual.proposals.panel",
            Self::ManualProposalsShort => "manual.proposals.short",
            Self::Menu => "menu",
            Self::Opening => "opening",
            Self::Phase => "phase",
            Self::PhaseIcon => "phase.icon",
            Self::PlayerOpponent => "player.opponent",
            Self::PlayerYou => "player.you",
            Self::PromptAsked => "prompt.asked",
            Self::PromptBar => "prompt.bar",
            Self::PromptCatalog => "prompt.catalog",
            Self::PromptClearance => "prompt.clearance",
            Self::PromptDock => "prompt.dock",
            Self::PromptStacked => "prompt.stacked",
            Self::PromptWays => "prompt.ways",
            Self::Result => "result",
            Self::ResultDock => "result.dock",
            Self::RevealedPile => "revealed.pile",
            Self::ScreenColumn => "screen.column",
            Self::ScreenDock => "screen.dock",
            Self::SettingsSheet => "settings.sheet",
            Self::Viewer => "viewer",
        }
    }

    /// The slot spelled `name` in the profile files, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|slot| slot.as_str() == name)
    }
}

impl fmt::Display for SlotName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SlotName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::parse(&name).ok_or_else(|| serde::de::Error::custom(format!("unknown slot `{name}`")))
    }
}
