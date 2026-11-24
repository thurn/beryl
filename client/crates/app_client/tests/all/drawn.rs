//! The "drawn" check (docs/beryl-3d.md, "Interface checks"): every glyph of an element's text
//! and every icon has an atlas cell at its own size. A tree line lists a text element's string
//! whether or not its glyphs drew, so only this check sees text with letters missing.

use app_client::driver::{Mode, Page, Recording};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::prompt::{ButtonId, Purpose, ShapeBody};
use beryl_protocol::protocol::{Kind, Side, Status, ZoneName};
use beryl_protocol::testing::{self, ScriptBuilder, ViewBuilder};
use kit_base::tokens::color;
use kit_frame::frame::StateBits;
use kit_probe::agent::{Fault, Rule};
use kit_ui::input::{AssetReady, GlyphCell, GlyphKey, Size};

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
    let mut page = Page::new(crate::test_data::data(), recording.into(), Side::You, SIZE)
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

#[test]
fn the_discard_prompt_wraps_beside_a_crowded_hand() {
    let mut view = ViewBuilder::new();
    view.script(ScriptBuilder::reset().build());
    for name in [
        "Portable Hole",
        "Snapcaster Mage",
        "Jace, Wielder of Mysteries",
        "Plains",
        "Jace, the Mind Sculptor",
        "Stock Up",
        "Subtlety",
        "Fractured Identity",
    ] {
        view.card(ZoneName::Hand, Side::You, name);
    }
    let mut update = view.update();
    let asks = "Discard a basic land card or two cards.";
    let mut submit = super::flat_layer::prompt_button(ButtonId::Confirm, "Submit 0", false);
    submit.enabled = false;
    let app_client::Input::Update(deciding) = super::flat_layer::deciding(testing::prompt(
        Purpose::Discard,
        asks,
        vec![submit],
        ShapeBody::Pick {
            tray: Vec::new(),
            least: 2,
            most: 2,
            picked: Vec::new(),
            warned: Vec::new(),
        },
    )) else {
        unreachable!("the sample prompt is a game update");
    };
    update.view.status = deciding.view.status;
    if let Status::Deciding { decision } = &mut update.view.status {
        decision.kind = Kind::YesNo;
        decision.cancellable = false;
    }

    let recording = Recording(vec![FixtureMessage::game_update(&update)]);
    let mut page = Page::new(
        crate::test_data::data(),
        recording.into(),
        Side::You,
        Size {
            width: 1728.0,
            height: 1117.0,
        },
    )
    .expect("the discard prompt opens")
    .with_scale(2.0);
    page.set_looking(true);
    page.still();

    let title = page.find(&format!("text \"{asks}\""));
    let submit = page.find("button \"Submit 0\"");
    let card = page.find("card \"Fractured Identity\"").rect();
    assert!(title.rect().x > card.x + card.width);
    assert!(title.rect().y + title.rect().height < submit.rect().y);
    assert!((title.rect().width - submit.rect().width).abs() < 0.01);
    assert!(submit.state.has(StateBits::DISABLED));

    let frame = page.frame();
    let lines: Vec<_> = frame
        .lines
        .iter()
        .filter(|line| line.element == title.index)
        .map(|line| frame.name(line.words).trim())
        .collect();
    assert_eq!(lines, ["Discard a basic land", "card or two cards."]);
    let look = |index| {
        frame
            .looks
            .elements
            .iter()
            .find(|look| look.element == index)
            .expect("the prompt's style is recorded")
    };
    assert_eq!(look(title.index).color, color::QUESTION_TITLE);
    assert_eq!(look(title.index).background, None);
    assert!(
        frame
            .looks
            .shadows
            .iter()
            .any(|shadow| shadow.element == title.index && shadow.drop)
    );
    assert_eq!(look(submit.index).color, color::DISABLED_INK);
    assert_eq!(look(submit.index).background, Some(color::DISABLED));
    assert!(drawn(&page.faults()).is_empty());
}
