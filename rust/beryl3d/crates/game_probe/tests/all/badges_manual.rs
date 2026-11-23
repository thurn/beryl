//! The manual table's badges on a permanent (docs/ui-conventions.md, "Manual counters and
//! untap flags"): a named counter wears its shortened name before its count, in a row above the
//! plaque, and a permanent that doesn't untap wears a lock in its top left corner.

use crate::badges::{badged, glyphs_of, table_records};
use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{CardType, NamedCounter, ObjectId, Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_table::table::badges::{name_style, style};
use glam::Vec2;
use kit_frame::frame::{Frame, Material};

/// A relic on the viewer's battlefield with `counters` and, if `locked`, flagged as skipping
/// its untap, at rest, and its slot.
fn relic(counters: &[(&str, i32)], locked: bool) -> (GameUpdate, ObjectId) {
    let mut builder = ViewBuilder::new();
    let relic = builder.card(ZoneName::Battlefield, Side::You, "Relic");
    relic.types = vec![CardType::Artifact];
    relic.named_counters = (counters.iter())
        .map(|&(name, count)| NamedCounter {
            name: name.to_owned(),
            count,
        })
        .collect();
    relic.doesnt_untap = locked;
    let slot = relic.slot;
    let mut update = builder.update();
    update.view.script = Some(ScriptBuilder::reset().build());
    (update, slot)
}

/// The corners on the screen of the card in `slot` in `frame`.
fn corners(frame: &Frame, slot: ObjectId) -> [Vec2; 4] {
    (frame.resolved.iter())
        .find(|element| element.id.0 == u32::from(slot.0))
        .expect("the card is drawn")
        .corners
}

#[test]
fn a_named_counter_wears_its_shortened_name_before_its_count() {
    let (update, _) = relic(&[("Corruption", 2), ("Ki", 4)], false);
    let mut show = badged(show(), &update);
    let drawn = frame(&mut show, &mut empty_frame(), 100 * MS);
    // Records are drawn farthest first, so read them as rows: the lower row first, each left
    // to right on the screen.
    let camera = show.table().camera();
    let mut records: Vec<(u16, Vec2)> = table_records(&drawn, Material::Glyph)
        .into_iter()
        .map(|(glyph, at)| (glyph, camera.to_screen(at)))
        .collect();
    records.sort_by(|a, b| {
        let row = |at: Vec2| (at.y / 10.0).round();
        row(b.1).total_cmp(&row(a.1)).then(a.1.x.total_cmp(&b.1.x))
    });
    let glyphs: Vec<u16> = records.into_iter().map(|(glyph, _)| glyph).collect();
    let mut expected = glyphs_of(
        "Corru\u{2026}",
        name_style(&crate::test_data::data().badges),
    );
    expected.extend(glyphs_of("2", style(&crate::test_data::data().badges)));
    expected.extend(glyphs_of(
        "Ki",
        name_style(&crate::test_data::data().badges),
    ));
    expected.extend(glyphs_of("4", style(&crate::test_data::data().badges)));
    assert_eq!(
        glyphs, expected,
        "a name past six letters is cut to five and an ellipsis, a shorter one is whole"
    );
    let panels = table_records(&drawn, Material::Panel).len();
    assert_eq!(panels, 2, "a badge for each named counter");
}

#[test]
fn a_permanent_that_doesnt_untap_wears_a_lock_in_its_top_left_corner() {
    let (update, slot) = relic(&[], true);
    let mut locked = badged(show(), &update);
    let drawn = frame(&mut locked, &mut empty_frame(), 100 * MS);
    let icons = table_records(&drawn, Material::Icon);
    assert_eq!(icons.len(), 1, "one lock");
    let camera = locked.table().camera();
    let lock = camera.to_screen(icons[0].1);
    let card = corners(&drawn, slot);
    let (low, high) = card.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(low, high), corner| (low.min(*corner), high.max(*corner)),
    );
    let share = (lock - low) / (high - low);
    assert!(
        share.x > 0.0 && share.x < 0.3 && share.y > 0.0 && share.y < 0.3,
        "the lock at {lock} lies in the top left of the card, from {low} to {high}"
    );

    let (update, _) = relic(&[("Corruption", 2)], true);
    let mut locked = badged(show(), &update);
    let drawn = frame(&mut locked, &mut empty_frame(), 100 * MS);
    assert_eq!(table_records(&drawn, Material::Icon).len(), 1);
    assert_eq!(
        table_records(&drawn, Material::Glyph).len(),
        7,
        "the lock leaves room for a named counter's badge"
    );

    let (update, _) = relic(&[], false);
    let mut unlocked = badged(show(), &update);
    let drawn = frame(&mut unlocked, &mut empty_frame(), 100 * MS);
    assert!(
        table_records(&drawn, Material::Icon).is_empty(),
        "a permanent that untaps wears no lock"
    );
}
