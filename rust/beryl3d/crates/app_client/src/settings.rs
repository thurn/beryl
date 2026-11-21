//! The player's settings: sound levels, stops, the keys bound to the manual table's commands
//! and the graphics tier (docs/beryl-3d.md, "Sound" and "Choosing a tier"). The host saves
//! them and hands them back at startup as `Input::Settings`; a change made on the settings
//! sheet arrives as a `Press::Setting` and is sent back to be saved as
//! `Request::SaveSettings`.

use beryl_protocol::StopsSetting;

use game_manual::manual::bindings::Bindings;

use app_screens::settings::ScreenSettings;
pub use app_screens::settings::{Graphics, Setting};

/// Everything the player sets that outlives a game: the web interface's `AudioSettings`
/// (`ui/src/audio/sound.ts`), its stops (`beryl.stops` in `ui/src/game/stops.ts`) and its
/// table key bindings (`beryl.table.bindings` in `ui/src/table/bindings.ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// The keys the player bound to the manual table's commands.
    pub bindings: Bindings,
    /// How the visual tier is chosen.
    pub graphics: Graphics,
}

impl Default for Settings {
    /// The web interface's defaults: music at 35, effects at 70, sound on, the smart stops,
    /// every table command on its own keys, and the tier chosen automatically.
    fn default() -> Self {
        Self {
            music: 35,
            effects: 70,
            muted: false,
            full_control: false,
            stops: StopsSetting::SMART,
            bindings: Bindings::new(),
            graphics: Graphics::Automatic,
        }
    }
}

impl Settings {
    /// The controls shown on the settings sheet.
    #[must_use]
    pub fn screen(&self) -> ScreenSettings {
        ScreenSettings {
            music: self.music,
            effects: self.effects,
            muted: self.muted,
            full_control: self.full_control,
            graphics: self.graphics,
        }
    }

    /// Makes the change `setting` says. A level over 100 is taken as 100, and moving a level
    /// turns the sound back on, as the web interface's sound panel does.
    pub fn apply(&mut self, setting: Setting) {
        match setting {
            Setting::Music(level) => {
                self.music = level.min(100);
                self.muted = false;
            }
            Setting::Effects(level) => {
                self.effects = level.min(100);
                self.muted = false;
            }
            Setting::Muted(muted) => self.muted = muted,
            Setting::FullControl(on) => {
                self.full_control = on;
                self.stops.full_control = on;
            }
            Setting::Graphics(graphics) => self.graphics = graphics,
        }
    }
}
