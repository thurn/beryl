//! The shipped profiles select a layout and fit their named slots to the window.

use game_table::profiles::Profile;
use game_table::table::Window;
use kit_base::data::profiles::Region;
use kit_base::geometry::{Rect, Size};
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
            for (name, spec) in &definition.slots {
                let bounds = if spec.region == Region::Window {
                    Rect {
                        width: window.size.width,
                        height: window.size.height,
                        ..Rect::default()
                    }
                } else {
                    safe
                };
                let slot = profile.slot(name).expect("declared slot").rect;
                assert!(slot.width > 0.0 && slot.height > 0.0, "{name}: {slot:?}");
                assert!(slot.x >= bounds.x && slot.y >= bounds.y, "{name}: {slot:?}");
                assert!(
                    slot.x + slot.width <= bounds.x + bounds.width
                        && slot.y + slot.height <= bounds.y + bounds.height,
                    "{name}: {slot:?} outside {bounds:?}"
                );
            }
            assert!(profile.slot("unregistered").is_none());
        }
    }
}

#[test]
fn the_windows_use_the_supplied_table_and_screen_tuning() {
    use std::sync::Arc;

    use game_table::table::Table;
    use kit_base::data::Data;
    use kit_base::data::profiles::{HeldDraw, SHIPPED};

    let renamed = SHIPPED[0]
        .1
        .replace("\"name\": \"phone\"", "\"name\": \"narrow test\"")
        .replace("\"side\": 8,", "\"side\": 14,")
        .replace("\"target_points\": 44,", "\"target_points\": 46,")
        .replace("\"size\": 1.57,", "\"size\": 1.65,")
        .replace("\"action_height\": 48,", "\"action_height\": 52,")
        .replacen(
            "\"inset\": [0, 16, 16, 16]",
            "\"inset\": [0, 22, 16, 22]",
            1,
        );
    let names = ["glow", "dissolve", "loss"];
    let definitions = names.map(|name| {
        beryl_test_files::read_to_string(format!("data/beryl3d/effects/{name}.json")).unwrap()
    });
    let effects = names
        .into_iter()
        .zip(definitions.each_ref().map(String::as_str));
    let data = Data::parse(
        |name| {
            beryl_test_files::read_to_string(format!("data/beryl3d/{name}"))
                .map_err(|error| format!("{name}: {error}"))
        },
        &[("narrow-test.json", &renamed), SHIPPED[1]],
        effects,
    )
    .unwrap();
    let narrow = data.profiles.iter().next().unwrap().usual_window();
    let mut table = Table::new(Arc::new(data), Window::default());

    assert!(table.layout().style.far_shade);
    table.resize(narrow, None);
    assert_eq!(table.profile().data().name, "narrow test");
    assert!(!table.layout().style.far_shade);
    assert_eq!(table.layout().target.to_bits(), 46.0_f32.to_bits());
    assert_eq!(table.camera().area.x.to_bits(), 14.0_f32.to_bits());
    let HeldDraw::OverRest { size, .. } = table.layout().play.held_draw else {
        panic!("the draw must be held over the resting card");
    };
    assert_eq!(size.to_bits(), 1.65_f32.to_bits());
    let column = table.profile().screen_column().expect("scrolling column");
    assert_eq!(column.bounds.x.to_bits(), 22.0_f32.to_bits());
    assert_eq!(
        column.action(kit_ui::widgets::button("Keep")).style.height,
        Some(52.0)
    );
}
