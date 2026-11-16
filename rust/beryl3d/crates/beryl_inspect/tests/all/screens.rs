use beryl_inspect::pages::Recorded;
use beryl_inspect::recordings;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::Layout;
use beryl_scene::driver::Page;

/// The recordings `data/beryl3d/screens.json` names, in its order.
fn listed_recordings() -> Vec<String> {
    let path = recordings::root().join("data/beryl3d/screens.json");
    let text = std::fs::read_to_string(&path).expect("the screens file is readable");
    let file: serde_json::Value = serde_json::from_str(&text).expect("the screens file is JSON");
    file["screens"]
        .as_array()
        .expect("the screens file lists screens")
        .iter()
        .map(|screen| {
            screen["recording"]
                .as_str()
                .expect("each screen names its recording")
                .to_owned()
        })
        .collect()
}

#[test]
fn every_recording_the_screens_file_names_is_listed() {
    let names: Vec<String> = recordings::names()
        .expect("the recordings pack is readable")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let recordings = listed_recordings();
    assert_eq!(recordings.len(), 10);
    for recording in recordings {
        assert!(
            names.contains(&recording),
            "`just b3d list` does not list {recording:?}"
        );
    }
}

/// Every prompt shape of `ui/shapes.json` has a fixture named in the screens file's
/// `prompt_shapes`, for the parity list to count, and that fixture is a recording of the pack.
#[test]
fn the_screens_file_names_a_fixture_for_every_prompt_shape() {
    let root = recordings::root();
    let read = |path: &str| -> serde_json::Value {
        let text = std::fs::read_to_string(root.join(path)).expect("the file is readable");
        serde_json::from_str(&text).expect("the file is JSON")
    };
    let shapes = read("ui/shapes.json");
    let screens = read("data/beryl3d/screens.json");
    let names: Vec<String> = recordings::names()
        .expect("the recordings pack is readable")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let listed = screens["prompt_shapes"]
        .as_object()
        .expect("the screens file has prompt_shapes");
    let shapes = shapes["shapes"]
        .as_object()
        .expect("ui/shapes.json lists shapes");
    assert_eq!(
        listed.keys().collect::<Vec<_>>(),
        shapes.keys().collect::<Vec<_>>(),
        "prompt_shapes names each shape of ui/shapes.json"
    );
    for (shape, fixture) in listed {
        let fixture = fixture.as_str().expect("each shape names a fixture");
        assert_eq!(
            Some(fixture),
            shapes[shape]["yours"].as_str(),
            "{shape}'s fixture is its `yours` in ui/shapes.json"
        );
        assert!(
            names.iter().any(|name| name == fixture),
            "`just b3d list` does not list {fixture:?}"
        );
    }
}

#[test]
fn a_screen_recording_puts_the_client_on_its_screen() {
    let tree = Page::fixture("screen/start", Side::You, Layout::Desktop).tree();
    assert!(tree.contains("region \"start\""), "{tree}");
    let tree = Page::fixture("screen/settings", Side::You, Layout::Desktop).tree();
    assert!(tree.contains("region \"settings\""), "{tree}");
    let tree = Page::fixture("screen/lobby", Side::You, Layout::Desktop).tree();
    assert!(tree.contains("region \"lobby\""), "{tree}");
    assert!(tree.contains("Deck loaded"), "{tree}");
}
