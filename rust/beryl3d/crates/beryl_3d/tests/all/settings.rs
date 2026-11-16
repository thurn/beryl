//! The settings file: what is written reads back the same.

use beryl_3d::settings;
use beryl_scene::settings::{Setting, Settings};

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
    settings::save(&dir, changed).unwrap();
    assert_eq!(settings::load(&dir), Ok(Some(changed)));
    let _ = std::fs::remove_dir_all(&dir);
}
