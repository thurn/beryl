//! The manual table's badges on a permanent (docs/ui-conventions.md, "Manual counters and
//! untap flags"): a named counter wears its shortened name before its count, in a row above the
//! plaque, and a permanent that doesn't untap wears a lock in its top left corner.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{CardType, NamedCounter, ObjectId, Side, ZoneName};
use beryl_scene::frame::{Frame, Material};
use beryl_scene::input::{GlyphCell, GlyphKey, IconCell};
use beryl_scene::play::Show;
use beryl_scene::table::badges::{NAME_STYLE, STYLE};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use beryl_scene::text::{Glyphs, Text, TypeStyle, glyph_key};
use beryl_scene::widgets::icons::{LOCK, LOCK_SIDE};
use glam::{Vec2, Vec3};

/// The window's physical pixels per logical point, as `common::window` sets it.
const SCALE: f32 = 2.0;

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

/// A show that has taken `update` with its badges and named counters shaped, every glyph of
/// both badge styles in the atlas on page 1 at the x of its number, and the lock on page 2.
fn badged(update: &GameUpdate) -> Show {
    let mut show = show();
    show.push(Box::new(update.clone()), Vec::new());
    let mut wanted = Glyphs::default();
    show.shape_badges(&mut Text::new(), &mut wanted, SCALE);
    let _: Vec<GlyphKey> = wanted.drain_wanted().collect();
    let cells: Vec<GlyphCell> = [STYLE, NAME_STYLE]
        .into_iter()
        .flat_map(|style| {
            (0..2_000).map(move |glyph| GlyphCell {
                key: glyph_key(style, SCALE, glyph),
                rect: [glyph, 0, 16, 20],
                bearing: [0, -20],
            })
        })
        .collect();
    let badges = show.badges_mut();
    badges.add(1, &cells);
    badges.add_icons(
        2,
        &[IconCell {
            icon: LOCK,
            size: LOCK_SIDE,
            rect: [0, 0, 48, 48],
        }],
    );
    show
}

/// The records of the table in `frame` drawn with `material`: the x of each one's atlas cell
/// and where its middle is in the table's space.
fn table_records(frame: &Frame, material: Material) -> Vec<(u16, Vec3)> {
    frame.draws[..frame.layer as usize]
        .iter()
        .filter(|draw| draw.material == material as u8)
        .map(|draw| {
            let model = draw.model;
            (draw.uv[0], Vec3::new(model[3], model[7], model[11]))
        })
        .collect()
}

/// The glyphs `text` is shaped into in `style`.
fn glyphs_of(text: &str, style: TypeStyle) -> Vec<u16> {
    let mut shaper = Text::new();
    let block = shaper.block(text, style, &[], None);
    block.glyphs.iter().map(|glyph| glyph.glyph).collect()
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
    let mut show = badged(&update);
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
    let mut expected = glyphs_of("Corru\u{2026}", NAME_STYLE);
    expected.extend(glyphs_of("2", STYLE));
    expected.extend(glyphs_of("Ki", NAME_STYLE));
    expected.extend(glyphs_of("4", STYLE));
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
    let mut show = badged(&update);
    let drawn = frame(&mut show, &mut empty_frame(), 100 * MS);
    let icons = table_records(&drawn, Material::Icon);
    assert_eq!(icons.len(), 1, "one lock");
    let camera = show.table().camera();
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
    let mut show = badged(&update);
    let drawn = frame(&mut show, &mut empty_frame(), 100 * MS);
    assert_eq!(table_records(&drawn, Material::Icon).len(), 1);
    assert_eq!(
        table_records(&drawn, Material::Glyph).len(),
        7,
        "the lock leaves room for a named counter's badge"
    );

    let (update, _) = relic(&[], false);
    let mut show = badged(&update);
    let drawn = frame(&mut show, &mut empty_frame(), 100 * MS);
    assert!(
        table_records(&drawn, Material::Icon).is_empty(),
        "a permanent that untaps wears no lock"
    );
}
