//! The shipped profiles select a layout and fit their named slots to the window.

use game_table::profiles::Profile;
use game_table::table::Window;
use kit_base::geometry::Size;
use kit_scene::{SizeClass, Slot};

use crate::REGULAR_LAYOUT;

#[test]
fn the_shorter_side_selects_the_profile_in_logical_points() {
    let data = crate::test_data::data();
    for (width, height, name, class) in [
        (499.9, 1200.0, "phone", SizeClass::Compact),
        (1200.0, 499.9, "phone", SizeClass::Compact),
        (500.0, 500.0, "desktop", SizeClass::Regular),
        (1200.0, 500.0, "desktop", SizeClass::Regular),
    ] {
        let window = Window {
            size: Size { width, height },
            scale: 2.0,
            ..Window::default()
        };
        let profile = Profile::for_window(&data.profiles, window);
        assert_eq!(profile.data().name, name);
        assert_eq!(profile.size_class(), class);
    }
    assert_eq!(
        data.profiles.for_size(Window::default().size).table.camera,
        REGULAR_LAYOUT.camera
    );
}

#[test]
fn every_named_slot_fits_in_each_test_window() {
    let data = crate::test_data::data();
    assert_eq!(data.profiles.iter().len(), 2);
    for definition in data.profiles.iter() {
        for window in definition.test_windows() {
            let profile = Profile::for_window(&data.profiles, window);
            assert_eq!(profile.data().name, definition.name);
            let safe = Slot::safe_area(window);
            for name in definition.slots.keys() {
                let slot = profile.slot(name).expect("declared slot").rect;
                assert!(slot.width > 0.0 && slot.height > 0.0, "{name}: {slot:?}");
                assert!(slot.x >= safe.x && slot.y >= safe.y, "{name}: {slot:?}");
                assert!(
                    slot.x + slot.width <= safe.x + safe.width
                        && slot.y + slot.height <= safe.y + safe.height,
                    "{name}: {slot:?} outside {safe:?}"
                );
            }
            assert!(profile.slot("unregistered").is_none());
        }
    }
}
