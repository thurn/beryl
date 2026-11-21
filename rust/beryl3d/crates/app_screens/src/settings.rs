//! The settings sheet's controls and the changes a player can make from it.

/// The values displayed by the settings sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenSettings {
    /// The music's level, from 0 to 100.
    pub music: u8,
    /// The sound effects' level, from 0 to 100.
    pub effects: u8,
    /// Whether every sound is off, whatever its level.
    pub muted: bool,
    /// Whether the player gets priority wherever they receive it.
    pub full_control: bool,
    /// How the visual tier is chosen.
    pub graphics: Graphics,
}

/// How the visual tier is chosen (docs/beryl-3d.md, "Choosing a tier").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Graphics {
    /// Enhanced when a measurement at startup passes, then Baseline for the rest of the session
    /// if frames run late.
    #[default]
    Automatic,
    /// Always Baseline.
    Baseline,
    /// Enhanced wherever the renderer has it, and never stepped down.
    Enhanced,
}

/// One change to the settings, as a control on the settings sheet makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    /// The music's level, from 0 to 100.
    Music(u8),
    /// The sound effects' level, from 0 to 100.
    Effects(u8),
    Muted(bool),
    FullControl(bool),
    Graphics(Graphics),
}

impl Setting {
    /// The level a level setting sets, from 0 to 100, or `None` for a switch.
    #[must_use]
    pub fn level(self) -> Option<u8> {
        match self {
            Self::Music(level) | Self::Effects(level) => Some(level),
            Self::Muted(_) | Self::FullControl(_) | Self::Graphics(_) => None,
        }
    }

    /// The same level setting at `level`, or the switch unchanged.
    #[must_use]
    pub fn at_level(self, level: u8) -> Self {
        match self {
            Self::Music(_) => Self::Music(level),
            Self::Effects(_) => Self::Effects(level),
            switch @ (Self::Muted(_) | Self::FullControl(_) | Self::Graphics(_)) => switch,
        }
    }
}
