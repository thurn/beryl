//! The badges drawn for battlefield permanents and piles.

use crate::common::{MS, empty_frame, frame, window};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{PlayerView, SessionId};
use beryl_protocol::testing::timing;
use game_play::play::Show;
use game_probe::story::{Body, Story};
use game_table::table::Table;
use game_table::table::badges::{name_style, style};
use glam::{Vec2, Vec3};
use kit_frame::frame::{AnchorBox, ElementId, Frame, Material, outline_corners};
use kit_text::text::{Glyphs, Text, TypeStyle, glyph_key};
use kit_ui::input::{GlyphCell, GlyphKey, IconCell};
use kit_ui::widgets::icons::{LOCK, LOCK_SIDE};

/// The window's physical pixels per logical point, as `common::window` sets it.
const SCALE: f32 = 2.0;

/// A show that has taken `update` with its badges and named counters shaped, every glyph of
/// both badge styles in the atlas on page 1 at the x of its number, and the lock on page 2.
pub(super) fn badged(mut show: Show, update: &GameUpdate) -> Show {
    show.push(Box::new(update.clone()), Vec::new());
    let mut wanted = Glyphs::default();
    show.shape_badges(&mut Text::new(), &mut wanted, SCALE);
    let _: Vec<GlyphKey> = wanted.drain_wanted().collect();
    let cells: Vec<GlyphCell> = [
        style(&crate::test_data::data().badges),
        name_style(&crate::test_data::data().badges),
    ]
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
            shadow: 0,
        }],
    );
    show
}

/// The records of the table in `frame` drawn with `material`: the x of each one's atlas cell
/// and where its middle is in the table's space.
pub(super) fn table_records(frame: &Frame, material: Material) -> Vec<(u16, Vec3)> {
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
pub(super) fn glyphs_of(text: &str, style: TypeStyle) -> Vec<u16> {
    let mut shaper = Text::new();
    let block = shaper.block(text, style, &[], None);
    block.glyphs.iter().map(|glyph| glyph.glyph).collect()
}

/// Asserts that the projected badge panels are disjoint and each contains its label.
fn assert_badges(frame: &Frame, show: &Show, expected: &[(u32, &str)]) {
    let camera = show.table().camera();
    let panels: Vec<_> = frame.draws[..frame.layer as usize]
        .iter()
        .filter(|draw| draw.material == Material::Panel as u8)
        .map(|draw| {
            AnchorBox::around(outline_corners(
                draw.model,
                [0.5, 0.5],
                frame.view_projection,
                camera.window.size,
            ))
        })
        .collect();
    for (index, panel) in panels.iter().enumerate() {
        for other in &panels[..index] {
            let apart = (panel.centre - other.centre)
                .abs()
                .cmpge(panel.half + other.half);
            assert!(
                apart.any(),
                "overlapping badge panels {panel:?} and {other:?}"
            );
        }
    }
    assert_eq!(panels.len(), expected.len(), "one panel per badge");
    let glyphs: Vec<_> = table_records(frame, Material::Glyph)
        .into_iter()
        .map(|(id, at)| (id, camera.to_screen(at)))
        .collect();
    let inside = |bounds: &AnchorBox, at: Vec2| (at - bounds.centre).abs().cmple(bounds.half).all();
    let drawn: Vec<_> = panels
        .iter()
        .map(|panel| {
            let mut glyphs: Vec<_> = glyphs.iter().filter(|(_, at)| inside(panel, *at)).collect();
            glyphs.sort_by(|(_, a), (_, b)| a.x.total_cmp(&b.x));
            let label = glyphs.into_iter().map(|&(id, _)| id).collect::<Vec<_>>();
            (panel.centre, label)
        })
        .collect();
    let style = style(&crate::test_data::data().badges);
    for &(slot, text) in expected {
        let card = AnchorBox::around(frame.corners(ElementId(slot)).expect("the card is drawn"));
        let expected = glyphs_of(text, style);
        assert!(
            drawn
                .iter()
                .any(|(at, glyphs)| *glyphs == expected && inside(&card, *at)),
            "expected {text} on card #{slot}, found {drawn:?}",
        );
    }
}

#[test]
fn stacked_tokens_share_a_stat_badge_until_their_stats_differ() {
    let story = Story::parse(include_str!(
        "../../../game_table/stories/stacked-token-stats.story"
    ))
    .expect("a valid table story");
    let Body::Table(table) = story.body else {
        panic!("expected a table story");
    };
    let [single, stacked, split] = table.views.as_slice() else {
        panic!("expected the second token to appear and then leave the pile");
    };
    let update = |view: &PlayerView| GameUpdate {
        session: SessionId(1),
        view: view.clone(),
    };
    for (width, height) in [(1728.0, 1117.0), (393.0, 759.0)] {
        let mut show = badged(
            Show::new(Table::new(crate::test_data::data(), window(width, height))),
            &update(single),
        );
        let mut scratch = empty_frame();
        frame(&mut show, &mut scratch, 0);

        show.push(Box::new(update(stacked)), vec![timing(100.0)]);
        frame(&mut show, &mut scratch, MS);
        let arriving = frame(&mut show, &mut scratch, 51 * MS);
        assert!(!show.settled(), "the second token is still fading in");
        assert!(
            arriving.corners(ElementId(3)).is_some(),
            "both cards are drawn"
        );
        assert_badges(&arriving, &show, &[(1, "4/5"), (2, "1/1")]);
        let drawn = frame(&mut show, &mut scratch, 1_000 * MS);
        assert!(show.settled(), "both tokens have settled");
        assert_badges(&drawn, &show, &[(1, "4/5"), (2, "1/1"), (2, "×2")]);

        show.push(Box::new(update(split)), vec![timing(0.0)]);
        frame(&mut show, &mut scratch, 1_001 * MS);
        let drawn = frame(&mut show, &mut scratch, 2_000 * MS);
        assert!(show.settled(), "the split has finished");
        assert_badges(&drawn, &show, &[(1, "4/5"), (2, "1/1"), (3, "2/3")]);
    }
}
