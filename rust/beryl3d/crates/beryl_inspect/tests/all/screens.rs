use std::collections::BTreeSet;

use app_client::driver::{Page, Recording};
use beryl_inspect::pages::Recorded;
use beryl_inspect::recordings;
use beryl_protocol::Seat;
use beryl_protocol::prompt::{Purpose, shape_name};
use beryl_protocol::protocol::{Side, Status};
use kit_base::data::profiles::SizeClass;

fn screens_file() -> serde_json::Value {
    let text = beryl_test_files::read_to_string("data/beryl3d/screens.json")
        .expect("the screens file is readable");
    serde_json::from_str(&text).expect("the screens file is JSON")
}

/// The recordings `data/beryl3d/screens.json` names, in its order.
fn listed_recordings() -> Vec<String> {
    screens_file()["screens"]
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

/// The screens file names a recording for every native prompt shape. Each recording
/// contains that prompt body for the first seat, where the client can render it.
#[test]
fn the_screens_file_names_a_fixture_for_every_prompt_shape() {
    let screens = screens_file();
    let listed = screens["prompt_shapes"]
        .as_object()
        .expect("the screens file has prompt_shapes");
    let shapes: BTreeSet<_> = Purpose::ALL
        .iter()
        .map(|purpose| shape_name(purpose.shape()))
        .collect();
    assert_eq!(
        listed.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        shapes,
        "prompt_shapes must name every native prompt shape"
    );
    for (shape, fixture) in listed {
        let fixture = fixture.as_str().expect("each shape names a fixture");
        let messages = recordings::messages(fixture, Seat::P0);
        let views = Recording(messages.to_vec())
            .views()
            .unwrap_or_else(|why| panic!("{fixture}: {why}"));
        assert!(
            views.iter().any(|update| matches!(
                &update.view.status,
                Status::Deciding { decision }
                    if decision.prompt.body.shape().map(shape_name) == Some(shape.as_str())
            )),
            "{fixture:?} contains no {shape:?} prompt for the first seat"
        );
    }
}

/// Each screen the screens file marks built shows in its recording: at the moment its `at`
/// names, the tree printout has the line its `shows` names. A screen marked built that its
/// recording does not show would count in the parity list without being there.
/// The start screen lists each saved table, the joined table by both players' names, with
/// Resume table beside it, on the desktop and on the phone, which also lists the joined table.
#[test]
fn the_start_screen_offers_resume_table_for_saved_tables() {
    for layout in ["desktop", "phone"] {
        for shows in ["text \"Sam · Alex\"", "button \"Resume table\""] {
            let entry = serde_json::json!({
                "recording": "screen/start-tables",
                "at": "end",
                "layout": layout,
                "shows": shows,
            });
            assert_shows(&format!("screen/start-tables ({layout})"), &entry);
        }
    }
}

#[test]
fn every_built_screen_shows_in_its_recording() {
    let file = screens_file();
    for screen in file["screens"]
        .as_array()
        .expect("the screens file lists screens")
    {
        let name = screen["name"].as_str().expect("each screen has a name");
        if screen["built"] != serde_json::Value::Bool(true) {
            continue;
        }
        assert_shows(name, screen);
    }
}

/// Asserts that the recording `entry` names shows the tree line its `shows` names at the
/// moment its `at` names, in the usual desktop window, once the actions its optional `do` lists
/// (as `just b3d tree --do` takes them, such as `hover card #8`) are done there in order. It is
/// seen from the player's seat, or from the opponent's where its optional `seat` is
/// `"opponent"`. The table is first let come to rest, unless its optional `still` is `false`,
/// which shows a moment of an animation as `just b3d tree --at` does. `name` says which entry
/// failed. The screens file's built screens and the conventions files' entries are both held
/// to this.
///
/// Its optional `layout`, `"phone"`, plays it in the phone layout's first window instead. The
/// whole tree is read, as `just b3d tree --full` prints it, and `shows` may name a line with
/// or without its numeric id and box, which change with any change to the recording or the
/// layout: `card "Island" battlefield.yours.lands interactive tapped` matches the line
/// `card "Island" #8 battlefield.yours.lands 140,573 121x133 interactive tapped`.
pub(crate) fn assert_shows(name: &str, entry: &serde_json::Value) {
    use std::str::FromStr;

    use app_client::driver::{Action, Moment};
    use kit_ui::input::Size;

    let field = |key: &str| {
        entry[key]
            .as_str()
            .unwrap_or_else(|| panic!("{name} names no `{key}`"))
    };
    let (recording, at, shows) = (field("recording"), field("at"), field("shows"));
    let (layout, size) = match entry["layout"].as_str() {
        None | Some("desktop") => (
            SizeClass::Regular,
            Size {
                width: 1600.0,
                height: 900.0,
            },
        ),
        Some("phone") => (
            SizeClass::Compact,
            beryl_inspect::pages::Windows::Class(SizeClass::Compact).windows()[0].size,
        ),
        Some(other) => panic!("{name}: `layout` is \"desktop\" or \"phone\", not {other:?}"),
    };
    let seat = match entry["seat"].as_str() {
        None | Some("you") => Side::You,
        Some("opponent") => Side::Opponent,
        Some(other) => panic!("{name}: `seat` is \"you\" or \"opponent\", not {other:?}"),
    };
    let still = entry["still"].as_bool().unwrap_or(true);
    let mut page = beryl_inspect::pages::page(recording, seat, layout, size)
        .unwrap_or_else(|why| panic!("{name}: {why}"));
    let moment = Moment::from_str(at).unwrap_or_else(|why| panic!("{name}: {at}: {why:?}"));
    page.seek(&moment)
        .unwrap_or_else(|why| panic!("{name}: {why}"));
    if still {
        page.still();
    }
    let actions = entry["do"].as_array().map_or(&[][..], Vec::as_slice);
    for action in actions {
        let action = action
            .as_str()
            .unwrap_or_else(|| panic!("{name}: each action of `do` is a string"));
        let parsed =
            Action::from_str(action).unwrap_or_else(|why| panic!("{name}: {action}: {why:?}"));
        page.try_act(&parsed)
            .unwrap_or_else(|why| panic!("{name}: {action}: {why}"));
        if still {
            page.still();
        }
    }
    // The short tree names only the first few cards of a crowded place.
    let options = kit_probe::agent::TreeOptions {
        full: true,
        ..kit_probe::agent::TreeOptions::default()
    };
    let tree =
        kit_probe::agent::tree(page.frame(), &options).expect("the whole tree needs no locator");
    let bare = without_ids_and_boxes(&tree);
    assert!(
        tree.contains(shows) || bare.lines().any(|line| line.contains(shows)),
        "{name}: {recording} at {at} shows no {shows}:\n{tree}"
    );
}

/// `tree` with each line's numeric id (`#8`) and box (`140,573 121x133`) taken out.
fn without_ids_and_boxes(tree: &str) -> String {
    let lines: Vec<String> = tree
        .lines()
        .map(|line| {
            let indent = &line[..line.len() - line.trim_start().len()];
            let words: Vec<&str> = line
                .split(' ')
                .filter(|word| {
                    !(word.is_empty() || is_id(word) || is_pair(word, ',') || is_pair(word, 'x'))
                })
                .collect();
            indent.to_owned() + &words.join(" ")
        })
        .collect();
    lines.join("\n")
}

/// Whether `word` is a tree line's numeric id, as `#8`.
fn is_id(word: &str) -> bool {
    word.strip_prefix('#').is_some_and(is_number)
}

/// Whether `word` is two numbers joined by `between`: a box's corner (`140,-573`) or its size
/// (`121x133`).
fn is_pair(word: &str, between: char) -> bool {
    word.split_once(between).is_some_and(|(a, b)| {
        is_number(a.trim_start_matches('-')) && is_number(b.trim_start_matches('-'))
    })
}

/// Whether `text` is one or more decimal digits.
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

#[test]
fn the_lobby_recording_shows_the_loaded_deck() {
    let tree = Page::fixture("screen/lobby", Side::You, SizeClass::Regular).tree();
    assert!(tree.contains("Deck loaded"), "{tree}");
}
