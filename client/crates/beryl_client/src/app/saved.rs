//! The settings the player saved, and the writer that saves their changes.

use crate::options::Options;
use crate::settings::{self, Saver};

/// The player's saved settings and the writer of their changes, in a live game the viewer
/// plays, with `--tier` in place of the saved graphics setting. It goes in the settings the
/// client holds, so that the settings the client asks to save never carry the other value back
/// to the frame loop.
pub(super) fn player_settings(
    options: &Options,
) -> (Option<app_client::settings::Settings>, Option<Saver>) {
    let (mut settings, saver) = saved_settings(options);
    if let Some(graphics) = options.graphics {
        settings.get_or_insert_default().graphics = graphics;
    }
    (settings, saver)
}

/// The player's saved settings and the writer of their changes, in a live game the viewer
/// plays. A replayed recording, a resumed replay file and a game `--agent` plays start from
/// the defaults and save nothing, so that a test's or an agent's run never changes the
/// player's settings.
fn saved_settings(options: &Options) -> (Option<app_client::settings::Settings>, Option<Saver>) {
    #[cfg(feature = "tools")]
    if options.fixture.is_some() {
        return (None, None);
    }
    if options.agent.is_some() || options.replay.is_some() {
        return (None, None);
    }
    let Some(dir) =
        beryl_shell::Dirs::for_installed_app(beryl_shell::Profile::current()).map(|dirs| dirs.data)
    else {
        return (None, None);
    };
    let settings = settings::load(&dir).unwrap_or_else(|error| {
        eprintln!("beryl: the saved settings could not be read, so the defaults apply: {error}");
        None
    });
    (settings, Some(Saver::start(dir)))
}
