use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::common::TEST;
use beryl_cli::specs::serve::*;

/// A view shown at once starts the page's stage over, from each seat; one that opens with
/// an ability on the stack numbers it as the scripts after the setup do, so the stage has
/// a node for what the next script resolves.
#[test]
fn the_view_shown_at_once_starts_over_with_the_stack_numbered() {
    let instances = Arc::new(Mutex::new(Vec::new()));
    let pages = Arc::new(Mutex::new(HashMap::new()));
    let card = "the-wandering-emperor";
    let moment = "1-grows-a-creature-and-gives-it-first-strike-pass";
    for (view, abilities) in [("yours", json!([1])), ("theirs", json!([]))] {
        let key = format!("{card}/{moment}/{view}/reset-test");
        let feed = instance(&TEST, &instances, &pages, &key, card, moment, view).unwrap();
        let shown = current(&feed);
        assert_eq!(
            shown["ok"]["view"]["script"]["reset"],
            json!({ "abilities": abilities }),
            "{view}: {}",
            shown["ok"]["view"]["script"]
        );
    }
}

#[test]
fn playground_priority_needs_no_play_button() {
    let instances = Arc::new(Mutex::new(Vec::new()));
    let pages = Arc::new(Mutex::new(HashMap::new()));
    let card = "aangs-iceberg";
    let moment = "playground-exiles-a-permanent-until-it-leaves-waterbend-taps-a-creature-to-pay-4";
    let key = format!("{card}/{moment}/yours/priority-test");
    let feed = instance(&TEST, &instances, &pages, &key, card, moment, "yours").unwrap();
    let shown = current(&feed);
    assert_eq!(shown["ok"]["view"]["status"]["kind"], "deciding");
    assert!(
        !holds(&lock(&feed.0)),
        "a human decision is already playable"
    );
}
