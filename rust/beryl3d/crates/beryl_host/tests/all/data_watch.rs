//! The watcher observes edits once, including changes to which effect files exist.

use std::fs::{self, File};
use std::time::Duration;

use beryl_host::DataWatch;

#[test]
fn reports_a_modification_time_change_once_and_detects_effect_additions_and_removals() {
    let directory = std::env::temp_dir().join(format!("b3d-data-watch-{}", std::process::id()));
    fs::create_dir_all(directory.join("effects")).unwrap();
    let tuning =
        ["verbs.json", "pointer.json"].map(|name| File::create(directory.join(name)).unwrap());
    let mut watch = DataWatch::new(directory.clone());
    assert!(!watch.changed());

    for file in tuning {
        let modified = file.metadata().unwrap().modified().unwrap();
        file.set_modified(modified - Duration::from_secs(1))
            .unwrap();
        assert!(watch.changed());
        assert!(!watch.changed());
    }

    let effect = directory.join("effects/new.json");
    fs::write(&effect, "{}").unwrap();
    assert!(watch.changed());
    assert!(!watch.changed());
    fs::remove_file(effect).unwrap();
    assert!(watch.changed());
    assert!(!watch.changed());
    fs::remove_dir_all(directory).unwrap();
}
