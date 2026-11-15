//! The player's settings: sound levels and stops (docs/beryl-3d.md, "Sound"). The host saves
//! them and hands them back at startup as `Input::Settings`; a change made on the settings
//! sheet arrives as a `Press::Setting` and is sent back to be saved as
//! `Request::SaveSettings`.

use beryl_protocol::StopsSetting;

/// Everything the player sets that outlives a game: the web interface's `AudioSettings`
/// (`ui/src/audio/sound.ts`) and its stops (`beryl.stops` in `ui/src/game/stops.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The music's level, from 0 to 100.
    pub music: u8,
    /// The sound effects' level, from 0 to 100.
    pub effects: u8,
    /// Whether every sound is off, whatever its level.
    pub muted: bool,
    /// Whether the player gets priority wherever they receive it, even where passing is all
    /// they can do.
    pub full_control: bool,
    /// Where the player stops with the stack empty.
    pub stops: StopsSetting,
}

impl Default for Settings {
    /// The web interface's defaults: music at 35, effects at 70, sound on, and the smart stops.
    fn default() -> Self {
        Self {
            music: 35,
            effects: 70,
            muted: false,
            full_control: false,
            stops: StopsSetting::SMART,
        }
    }
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
}

impl Settings {
    /// Makes the change `setting` says. A level over 100 is taken as 100.
    pub fn apply(&mut self, setting: Setting) {
        match setting {
            Setting::Music(level) => self.music = level.min(100),
            Setting::Effects(level) => self.effects = level.min(100),
            Setting::Muted(muted) => self.muted = muted,
            Setting::FullControl(on) => {
                self.full_control = on;
                self.stops.full_control = on;
            }
        }
    }
}
