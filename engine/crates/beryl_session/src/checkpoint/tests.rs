use beryl_engine::state::CounterKind;

use super::CounterName;

/// A save names a kind of counter as the rules do, and every kind of the list is read
/// back from its name.
#[test]
fn every_kind_of_counter_round_trips_through_its_name_in_a_save() {
    for &kind in CounterKind::ALL {
        let saved = serde_json::to_string(&CounterName(kind)).unwrap();
        assert_eq!(saved, format!("{:?}", kind.name()));
        let read: CounterName = serde_json::from_str(&saved).unwrap();
        assert_eq!(read, CounterName(kind));
    }
    assert!(serde_json::from_str::<CounterName>("\"no such\"").is_err());
}

/// The names saves were written with, which no change to the list may move.
#[test]
fn the_saved_names_of_the_kinds_stay() {
    let names: Vec<&str> = CounterKind::ALL.iter().map(|k| k.name()).collect();
    assert!(
        ["+1/+1", "-1/-1", "loyalty", "time", "story", "chorus"]
            .iter()
            .all(|name| names.contains(name))
    );
}
