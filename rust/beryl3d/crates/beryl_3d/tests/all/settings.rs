//! The settings file: what is written reads back the same.

use app_client::settings::{Graphics, Setting, Settings};
use beryl_3d::settings;
use game_manual::manual::commands;

#[test]
fn settings_written_read_back_equal() {
    let dir = std::env::temp_dir().join(format!("beryl3d-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(settings::load(&dir), Ok(None));
    let mut changed = Settings::default();
    changed.apply(Setting::Music(12));
    changed.apply(Setting::Effects(99));
    changed.apply(Setting::Muted(true));
    changed.apply(Setting::FullControl(true));
    changed.apply(Setting::Graphics(Graphics::Enhanced));
    // A new key bound to Draw, then Untap all's key bound to Draw, which leaves Untap all with
    // none: both kinds of entry are saved.
    let (_, draw) = commands::command("library.draw").unwrap();
    changed.bindings.bind(draw, "Cmd+Shift+F9", true);
    let (_, untap) = commands::command("table.untap_all").unwrap();
    let own = untap.key.unwrap().mac;
    changed.bindings.bind(draw, own, true);
    settings::save(&dir, changed.clone()).unwrap();
    assert_eq!(settings::load(&dir), Ok(Some(changed)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_without_the_graphics_setting_chooses_the_tier_automatically() {
    let dir = std::env::temp_dir().join(format!("beryl3d-graphics-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(settings::FILE), r#"{"music": 20}"#).unwrap();
    let loaded = settings::load(&dir).unwrap().unwrap();
    assert_eq!(loaded.music, 20);
    assert_eq!(loaded.graphics, Graphics::Automatic);
    let _ = std::fs::remove_dir_all(&dir);
}
