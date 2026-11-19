//! The "drawn" check (docs/beryl-3d.md, "Interface checks"): every glyph of an element's text
//! and every icon has an atlas cell at its own size. A tree line lists a text element's string
//! whether or not its glyphs drew, so only this check sees text with letters missing.

use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::{Fault, Layout, Rule};
use beryl_scene::driver::{Mode, Page, Recording};
use beryl_scene::input::{AssetReady, GlyphCell, GlyphKey, Size};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// The sample table, whose life totals, names and labels are text and whose corner has the gear
/// and bug buttons' icons, at `scale` physical pixels to a point (2 on a Retina display), with
/// the driver's stub atlas or, if `host`, nothing delivered until the test gives it.
fn table(scale: f32, host: bool) -> Page {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let recording = Recording(vec![FixtureMessage::game_update(&view.update())]);
    let mut page = Page::new(recording.into(), Side::You, Layout::Desktop, SIZE)
        .expect("the sample view opens")
        .named("sample")
        .with_scale(scale);
    if host {
        page = page.with_host_atlas();
    }
    page.still();
    page
}

fn drawn(faults: &[Fault]) -> Vec<&Fault> {
    faults
        .iter()
        .filter(|fault| fault.rule == Rule::Drawn)
        .collect()
}

/// A cell for `key` somewhere on atlas page 1, as a host's glyph worker would place it.
fn cell(key: GlyphKey) -> GlyphCell {
    GlyphCell {
        key,
        rect: [0, 0, key.size / 2 + 1, key.size],
        bearing: [0, -10],
    }
}

#[test]
fn with_every_glyph_and_icon_delivered_nothing_is_undrawn() {
    for scale in [1.0, 2.0] {
        let mut page = table(scale, false);
        let checked = page.check_all(Mode::AtCheckTimes);
        let faults = drawn(&checked.faults);
        assert!(faults.is_empty(), "at scale {scale}: {faults:#?}");
        assert!(
            checked.judged.of(Rule::Drawn).elements > 0,
            "at scale {scale}, \"drawn\" judged no glyph or icon"
        );
    }
}

#[test]
fn a_glyph_the_host_never_delivered_is_a_fault_naming_its_text() {
    let mut page = table(2.0, true);
    page.frame();
    let mut wanted = page.wanted_glyphs();
    assert!(wanted.len() > 1, "the table's text wants glyphs");
    // Every glyph but one arrives, as when the renderer refused the atlas page one was on.
    let lost = wanted.pop().expect("one glyph to lose");
    page.give_glyphs(AssetReady::Glyphs {
        page: 1,
        glyphs: wanted.into_iter().map(cell).collect(),
    });
    let faults = page.faults();
    let faults = drawn(&faults);
    assert!(!faults.is_empty(), "the glyph {lost:?} never arrived");
    for fault in &faults {
        assert_eq!(fault.elements.len(), 1, "{fault:?}");
        assert!(
            fault.message.starts_with("1 glyph of text ") || fault.message.contains("'s icon "),
            "{}",
            fault.message
        );
    }
    // Once the glyph arrives, the text draws in full.
    page.give_glyphs(AssetReady::Glyphs {
        page: 2,
        glyphs: vec![cell(lost)],
    });
    let faults = page.faults();
    assert!(
        drawn(&faults)
            .iter()
            .all(|fault| fault.message.contains("'s icon ")),
        "{faults:#?}"
    );
}

#[test]
fn an_icon_the_host_never_delivered_is_a_fault() {
    let mut page = table(1.0, true);
    page.frame();
    let glyphs: Vec<GlyphCell> = page.wanted_glyphs().into_iter().map(cell).collect();
    page.give_glyphs(AssetReady::Glyphs { page: 1, glyphs });
    let faults = page.faults();
    let faults = drawn(&faults);
    assert!(
        faults
            .iter()
            .any(|fault| fault.message.contains("'s icon ")),
        "the table's icons were never delivered, so some must be undrawn: {faults:#?}"
    );
}
