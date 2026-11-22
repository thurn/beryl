//! The shipped profiles select a layout and fit their named slots to the window.

#[path = "../../../../test_profiles.rs"]
mod test_profiles;

use test_profiles::SHIPPED;

use game_table::profiles::Profile;
use game_table::table::Window;
use kit_base::data::effect::Effect;
use kit_base::data::profiles::Region;
use kit_base::geometry::{Rect, Size};
use kit_scene::{SizeClass, Slot};

use crate::reference;

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
        reference().camera
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
    use kit_base::data::profiles::HeldDraw;

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
    let names = Effect::ALL.map(Effect::name);
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

#[test]
fn the_reference_and_fitted_tables_read_the_supplied_geometry() {
    use kit_base::data::profiles::Profiles;

    let shipped = Profiles::parse(&SHIPPED, crate::test_data::data().card).unwrap();
    let window = shipped.fallback().usual_window();
    let original = Profile::for_window(&shipped, window).layout();
    let modified = SHIPPED[1]
        .1
        .replace("\"pitch_degrees\": 27.0", "\"pitch_degrees\": 29.0")
        .replace("\"land_width\": 100", "\"land_width\": 96")
        .replace("\"depth\": 300", "\"depth\": 306");
    let profiles = Profiles::parse(
        &[SHIPPED[0], (SHIPPED[1].0, &modified)],
        crate::test_data::data().card,
    )
    .unwrap();
    let profile = Profile::for_window(&profiles, window);
    let reference = profile.reference_layout().unwrap();
    let fitted = profile.layout();

    assert_eq!(reference.camera.pitch_degrees.to_bits(), 29.0_f32.to_bits());
    assert_eq!(reference.hand.depth.to_bits(), 306.0_f32.to_bits());
    assert_eq!(fitted.camera, reference.camera);
    assert_eq!(fitted.hand, reference.hand);
    assert!(fitted.fitted.unwrap()[0][1].width < original.fitted.unwrap()[0][1].width);
}

#[test]
fn a_put_back_tray_uses_the_deck_geometry_of_its_own_profile() {
    use std::sync::Arc;

    use beryl_protocol::protocol::ObjectId;
    use game_table::table::{Held, Table, TrayKind};
    use kit_base::data::profiles::Profiles;

    let compact = SHIPPED[0]
        .1
        .replace("\"deck_backs\": 4", "\"deck_backs\": 5");
    let regular = SHIPPED[1]
        .1
        .replace("\"deck_backs\": 4", "\"deck_backs\": 3");
    let mut data = (*crate::test_data::data()).clone();
    data.profiles = Profiles::parse(
        &[(SHIPPED[0].0, &compact), (SHIPPED[1].0, &regular)],
        data.card,
    )
    .unwrap();
    let data = Arc::new(data);
    let held = Held {
        piles: vec![vec![], vec![ObjectId(1)]],
        rooms: vec![Some(1), Some(1)],
        kind: TrayKind::PutBack,
    };
    for (definition, count) in data.profiles.iter().zip([5, 3]) {
        let table = Table::new(Arc::clone(&data), definition.usual_window());
        assert_eq!(table.profile().tray(), table.layout().tray);
        assert_eq!(table.tray_poses(&held).library.len(), count);
    }
}
