//! The player's settings on disk (docs/beryl-3d.md, "Sound" and "Choosing a tier"): `settings.json` in the app's
//! data folder. The app reads it at startup and hands it to the client as `Input::Settings`,
//! and writes it again for each `Request::SaveSettings`, on a thread of its own so that the
//! frame thread does no file I/O.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use beryl_scene::manual::bindings::Bindings;
use beryl_scene::settings::{Graphics, Settings};
use beryl_session::protocol::StopsSetting;
use serde::{Deserialize, Serialize};

/// The file's name in the data folder.
pub const FILE: &str = "settings.json";

/// The file's contents: `Settings`, field for field. A field the file lacks takes the web
/// interface's default, so that a file from an older build still reads.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    music: u8,
    effects: u8,
    muted: bool,
    full_control: bool,
    stops: StopsSetting,
    /// The table commands whose keys the player changed, by command id (`Bindings`).
    bindings: BTreeMap<String, Option<String>>,
    graphics: SavedGraphics,
}

/// `Graphics` as the file writes it: `automatic`, `baseline` or `enhanced`.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedGraphics {
    #[default]
    Automatic,
    Baseline,
    Enhanced,
}

impl From<Graphics> for SavedGraphics {
    fn from(graphics: Graphics) -> Self {
        match graphics {
            Graphics::Automatic => Self::Automatic,
            Graphics::Baseline => Self::Baseline,
            Graphics::Enhanced => Self::Enhanced,
        }
    }
}

impl From<SavedGraphics> for Graphics {
    fn from(saved: SavedGraphics) -> Self {
        match saved {
            SavedGraphics::Automatic => Self::Automatic,
            SavedGraphics::Baseline => Self::Baseline,
            SavedGraphics::Enhanced => Self::Enhanced,
        }
    }
}

impl Default for Saved {
    fn default() -> Self {
        Settings::default().into()
    }
}

impl From<Settings> for Saved {
    fn from(settings: Settings) -> Self {
        Self {
            music: settings.music,
            effects: settings.effects,
            muted: settings.muted,
            full_control: settings.full_control,
            stops: settings.stops,
            bindings: settings.bindings.overrides().clone(),
            graphics: settings.graphics.into(),
        }
    }
}

impl From<Saved> for Settings {
    fn from(saved: Saved) -> Self {
        Self {
            music: saved.music.min(100),
            effects: saved.effects.min(100),
            muted: saved.muted,
            full_control: saved.full_control,
            stops: saved.stops,
            bindings: Bindings::from_overrides(saved.bindings),
            graphics: saved.graphics.into(),
        }
    }
}

/// Writes `settings` to `settings.json` in `dir`, making the folder if it is missing. The file
/// is written whole beside its final name and then renamed, so that a crash never leaves half
/// of one.
///
/// # Errors
///
/// When the folder or the file cannot be written.
pub fn save(dir: &Path, settings: Settings) -> Result<(), String> {
    let path = dir.join(FILE);
    let text = serde_json::to_string_pretty(&Saved::from(settings))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let partial = dir.join(format!("{FILE}.partial"));
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&partial, text))
        .and_then(|()| std::fs::rename(&partial, &path))
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The settings `settings.json` in `dir` holds; `None` when there is no such file.
///
/// # Errors
///
/// When the file exists but cannot be read or is not settings.
pub fn load(dir: &Path) -> Result<Option<Settings>, String> {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let saved: Saved =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Some(saved.into()))
}

/// Writes settings on a thread of its own. A burst of changes, such as a slider dragged, is
/// written once, as its last settings.
pub struct Saver {
    sender: Sender<Settings>,
}

impl Saver {
    /// Starts the thread that writes settings into `dir`.
    #[must_use]
    pub fn start(dir: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel::<Settings>();
        let started = std::thread::Builder::new()
            .name("settings writer".to_owned())
            .spawn(move || {
                while let Ok(mut settings) = receiver.recv() {
                    // Only the latest of the settings already waiting is worth writing.
                    while let Ok(later) = receiver.try_recv() {
                        settings = later;
                    }
                    if let Err(error) = save(&dir, settings) {
                        eprintln!("beryl3d: the settings could not be saved: {error}");
                    }
                }
            });
        if let Err(error) = started {
            eprintln!("beryl3d: the settings will not be saved: {error}");
        }
        Self { sender }
    }

    /// Asks the thread to write `settings`.
    pub fn save(&self, settings: Settings) {
        // The thread ends only with the app, or never started, which was logged then.
        let _ = self.sender.send(settings);
    }
}
