//! The list of faults the interface tests may have (`data/client/test-faults.json`).

use beryl_inspect::faults::Listed;
use beryl_inspect::runner::{Outcome, Test};
use beryl_protocol::protocol::Side;
use kit_probe::agent::{Fault, Rule};
use kit_ui::input::Size;

fn outcome(recording: &str, rules: &[Rule]) -> Outcome {
    Outcome {
        test: Test {
            profile: None,
            recording: recording.to_owned(),
            seat: Side::You,
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 1.0,
        },
        faults: rules
            .iter()
            .map(|&rule| Fault {
                rule,
                elements: Vec::new(),
                at: 0,
                message: String::new(),
            })
            .collect(),
        frames: 1,
        visibility_only_frames: 0,
        judged: kit_probe::agent::Judged::default(),
        error: None,
        seconds: 0.0,
    }
}

fn listed(text: &str) -> Listed {
    Listed::parse(text).expect("a list")
}

const A: &str = "client:a/you/1600x900";

#[test]
fn a_fault_over_its_entry_or_not_listed_fails_and_one_under_it_is_too_high() {
    let list = listed(&format!(r#"{{"tests": {{"{A}": {{"no-jumps": 2}}}}}}"#));
    let same = [outcome("a", &[Rule::Jump, Rule::Jump])];
    let compared = list.compare(&same, true, "just client test --update");
    assert!(compared.over.is_empty() && compared.high.is_empty());

    let more = [outcome("a", &[Rule::Jump, Rule::Jump, Rule::Jump])];
    assert_eq!(list.compare(&more, true, "u").over.len(), 1);
    let other = [outcome("a", &[Rule::Jump, Rule::Jump, Rule::Overlap])];
    let compared = list.compare(&other, true, "u");
    assert!(compared.over[0].1.contains("`overlap` faults, and none"));

    let fewer = [outcome("a", &[Rule::Jump])];
    let compared = list.compare(&fewer, true, "just client test --update");
    assert!(compared.over.is_empty());
    assert!(compared.high[0].1.contains("`just client test --update`"));
    // Only the tests that ran are held to their entries, unless they are all the tests.
    assert!(list.compare(&[], false, "u").high.is_empty());
    assert_eq!(list.compare(&[], true, "u").high.len(), 1);

    // The faults the list does not allow are the ones after the listed count.
    assert_eq!(list.excess(&more[0]).len(), 1);
}

#[test]
fn update_lowers_and_removes_and_raises_only_when_asked() {
    let text = format!(r#"{{"tests": {{"{A}": {{"no-jumps": 3, "overlap": 1}}}}}}"#);
    let better = [outcome("a", &[Rule::Jump])];

    let mut list = listed(&text);
    let changes = list.update(&better, true, false);
    assert_eq!((changes.lowered, changes.removed), (1, 1));
    assert_eq!(list.tests[A]["no-jumps"], 1);
    assert!(!list.tests[A].contains_key("overlap"));

    // A fault that got worse or is new changes nothing without `raise`.
    let worse = [outcome("a", &[Rule::Jump; 4]), outcome("b", &[Rule::Clips])];
    let mut list = listed(&text);
    assert!(!list.update(&worse, false, false).any() || list.tests[A]["no-jumps"] == 3);
    assert_eq!(list.tests[A]["no-jumps"], 3);
    assert!(!list.tests.contains_key("client:b/you/1600x900"));
    let changes = list.update(&worse, false, true);
    assert_eq!((changes.raised, changes.added), (1, 1));
    assert_eq!(list.tests[A]["no-jumps"], 4);

    // Nothing improved: the text is the same.
    let mut list = listed(&text);
    let same = [outcome(
        "a",
        &[Rule::Jump, Rule::Jump, Rule::Jump, Rule::Overlap],
    )];
    assert!(!list.update(&same, true, false).any());
    assert_eq!(listed(&list.text()), listed(&text));
}
