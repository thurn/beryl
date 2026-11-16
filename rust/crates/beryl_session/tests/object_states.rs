//! What a view says of a permanent's state that its card does not: phased out, the cards
//! merged under it, and the card a copy really is.

use beryl_cards::names;
use beryl_session::protocol::{Facing, ObjectView, PlayerView, Side, ZoneName};

use crate::common;

/// The objects named `name` in `view`.
fn named<'a>(view: &'a PlayerView, name: &str) -> Vec<&'a ObjectView> {
    (view.objects.iter())
        .filter(|o| o.name.as_deref() == Some(name))
        .collect()
}

/// Gemrazer mutating under Blood Artist leaves Gemrazer's card in the merged zone, and both
/// seats' views send it as part of the permanent ([CR 731.2b]): "As an object merges with a
/// permanent, that object leaves its previous zone and becomes part of an object on the
/// battlefield".
///
/// [CR 731.2b]: https://yawgatog.com/resources/magic-rules/#R7312b
#[test]
fn a_merged_permanents_other_cards_are_sent_as_part_of_it() {
    let views = common::views(
        names::GEMRAZER,
        "Gemrazer under a creature: the creature's values, Gemrazer's abilities",
        |_, _, _| {},
    );
    for view in views.last().expect("a view") {
        let [artist] = named(view, "Blood Artist")[..] else {
            panic!("one Blood Artist");
        };
        let [gemrazer] = named(view, "Gemrazer")[..] else {
            panic!("one Gemrazer");
        };
        assert_eq!(artist.zone, ZoneName::Battlefield);
        assert_eq!(gemrazer.zone, ZoneName::Merged);
        assert_eq!(gemrazer.part_of, Some(artist.slot));
        assert!(!view.battlefield.contains(&gemrazer.slot));
    }
}

/// Glasspool Mimic entering as a copy of Gemrazer's merged creature shows Gemrazer, and says
/// it is really Glasspool Mimic ([CR 707.2]). The merged creature itself, though a copy effect
/// gives it its values, says nothing: its cards are drawn under it.
///
/// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
#[test]
fn a_copy_says_what_card_it_really_is() {
    let views = common::views(
        names::GEMRAZER,
        "Glasspool Mimic copying Gemrazer's merged creature has both cards' abilities",
        |_, _, _| {},
    );
    let copied = |view: &PlayerView| {
        named(view, "Gemrazer")
            .into_iter()
            .filter(|o| o.zone == ZoneName::Battlefield)
            .filter_map(|o| o.really.clone())
            .collect::<Vec<_>>()
    };
    let seen = views
        .iter()
        .find(|seats| seats.iter().all(|v| !copied(v).is_empty()))
        .expect("a view with the copy on the battlefield");
    for view in seen {
        assert_eq!(copied(view), vec!["Glasspool Mimic".to_owned()]);
    }
}

/// Gemrazer mutating over a creature Abhorrent Oculus manifested makes a face-up Gemrazer that
/// both seats see, while the card under it stays face down: its controller's view names it, and
/// the opponent's draws it face down with no name ([CR 731.2e]): "If a merged permanent contains face-up
/// and face-down components, the permanent's status is determined by its topmost component."
///
/// [CR 731.2e]: https://yawgatog.com/resources/magic-rules/#R7312e
#[test]
fn a_face_down_card_under_a_merged_permanent_stays_hidden() {
    let views = common::views(
        names::GEMRAZER,
        "Gemrazer over a face-down creature: a face-up Gemrazer, its card hidden",
        |_, _, _| {},
    );
    // The card drawn under the merged Gemrazer: its name and whether it is face down.
    let under = |view: &PlayerView| {
        let [gemrazer] = named(view, "Gemrazer")[..] else {
            panic!("one Gemrazer in {:?}", view.objects);
        };
        assert_eq!(gemrazer.zone, ZoneName::Battlefield);
        assert_eq!(gemrazer.facing, Facing::Up);
        let [card] = (view.objects.iter())
            .filter(|o| o.part_of == Some(gemrazer.slot))
            .collect::<Vec<_>>()[..]
        else {
            panic!("one card under Gemrazer");
        };
        (card.name.clone(), card.facing)
    };
    let [mine, theirs] = views.last().expect("a view");
    assert_eq!(
        under(mine),
        (Some("Wetland Sambar".to_owned()), Facing::Up),
        "its controller knows the card under it"
    );
    assert_eq!(
        under(theirs),
        (None, Facing::Down),
        "the opponent never saw the card under it"
    );
}

/// Teferi's Protection phases out its controller's permanents ([CR 702.26b]): they stay on
/// the battlefield, marked phased out in both seats' views.
///
/// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
#[test]
fn phased_out_permanents_stay_on_the_battlefield_marked() {
    let views = common::views(
        names::TEFERIS_PROTECTION,
        "a creature phased out in response to Lightning Bolt survives",
        |_, _, _| {},
    );
    let phased = |view: &PlayerView| {
        (view.objects.iter())
            .filter(|o| o.phased_out)
            .map(|o| (o.zone, o.controller))
            .collect::<Vec<_>>()
    };
    let [mine, theirs] = views
        .iter()
        .find(|seats| seats.iter().all(|v| !phased(v).is_empty()))
        .expect("a view with phased-out permanents");
    assert!(
        (phased(mine).iter())
            .all(|&(zone, side)| zone == ZoneName::Battlefield && side == Side::You)
    );
    assert!(
        (phased(theirs).iter())
            .all(|&(zone, side)| zone == ZoneName::Battlefield && side == Side::Opponent)
    );
}
