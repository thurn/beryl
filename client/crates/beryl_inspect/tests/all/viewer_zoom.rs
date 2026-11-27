//! Enlarging a card in a pile viewer, from a bug report's capture of a Dandân game.
//!
//! A shared-deck table keeps one library, graveyard and exile for both players. Their cards are
//! placed in a shared pile (`game_table::table::Place::SharedPile`), not on a side, and the
//! inspector once read only cards on a side, so pointing at a card in the shared Graveyard viewer
//! showed nothing. The story opens that viewer and points at a card through the same input path
//! the app uses (`--do press` and `--do hover`).

use beryl_inspect::cli;

const STORY: &str = "story/game_hud/shared-graveyard-zoom";
const WINDOW: (f32, f32) = (1728.0, 1117.0);

fn command(words: &[&str]) -> cli::Ran {
    let args: Vec<String> = words.iter().map(ToString::to_string).collect();
    let output = cli::main(&args);
    assert_eq!(output.code, 0, "{}", output.stderr);
    output
}

/// The box `x,y wxh` that a tree line gives after its place.
fn tree_box(line: &str) -> Option<(f32, f32, f32, f32)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let at = words.iter().position(|word| {
        word.split_once(',')
            .is_some_and(|(x, y)| x.parse::<f32>().is_ok() && y.parse::<f32>().is_ok())
    })?;
    let (x, y) = words[at].split_once(',')?;
    let (w, h) = words.get(at + 1)?.split_once('x')?;
    Some((
        x.parse().ok()?,
        y.parse().ok()?,
        w.parse().ok()?,
        h.parse().ok()?,
    ))
}

#[test]
fn pointing_at_a_card_in_the_shared_graveyard_viewer_enlarges_it_on_screen() {
    let tree = command(&["tree", STORY, "--full"]).stdout;
    assert!(
        tree.contains("region \"viewer\""),
        "the Graveyard viewer is not open:\n{tree}"
    );
    let line = tree
        .lines()
        .find(|line| line.contains("region \"Telling Time, inspected\""))
        .unwrap_or_else(|| panic!("pointing at Telling Time shows no inspector:\n{tree}"));
    let (x, y, width, height) =
        tree_box(line).unwrap_or_else(|| panic!("no box in the inspector's line: {line}"));
    assert!(
        x >= 0.0 && y >= 0.0 && x + width <= WINDOW.0 && y + height <= WINDOW.1,
        "the inspector is not wholly on screen: {line}"
    );

    let check = command(&["check", STORY, "--json"]);
    let checked: serde_json::Value = serde_json::from_str(&check.stdout).expect("check JSON");
    assert_eq!(checked["faults"], serde_json::json!([]), "{}", check.stdout);
}
