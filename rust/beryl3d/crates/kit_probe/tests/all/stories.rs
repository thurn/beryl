use std::sync::Arc;

use kit_base::data::effect::Effects;
use kit_base::geometry::{Insets, Window};
use kit_probe::agent::{TreeOptions, tree};
use kit_probe::story::{Player, Story};

const ELEMENT: &str = concat!(
    "story element 360x240 desktop\n",
    "region \"Control | panel\" #1 12,12 320x160 | fill=#272b32ff radius=8 border=2,#999999ff\n",
    "  button \"Say \\\"yes\\\"\" #2 layer 22,34 160x48 interactive primary | widget=button text=\"Save it\"\n"
);
const CONTAINER: &str = concat!(
    "story container 360x240 desktop\n",
    "container row 12,12 320x160 gap=6 max=40\n",
    "item 40x56 group=first\n",
    "item 44x56 group=first\n",
);
const EFFECT: &str = "story effect 360x240 desktop\neffect glow 114,60 132x120\n";

#[test]
fn every_kind_round_trips_as_text() {
    for text in [ELEMENT, CONTAINER, EFFECT] {
        let story: Story = text.parse().expect("valid story");
        assert_eq!(story.to_string(), text);
    }
}

#[test]
fn diagnoses_bad_stories_at_their_source_line() {
    rejects([
        ("", 1, "story <element|container|effect>"),
        (
            "story board 360x240 desktop\n",
            1,
            "story <element|container|effect>",
        ),
        ("story element infx240 desktop\n", 1, "<width>x<height>"),
        ("story element 360x240 wall\n", 1, "<desktop|phone>"),
        ("story element 360x240 desktop\n", 2, "role"),
        (
            "story element 360x240 desktop\n  region #1 0,0 1x1\n",
            2,
            "parent",
        ),
        (
            "story element 360x240 desktop\n region #1 0,0 1x1\n",
            2,
            "indent",
        ),
        (
            "story element 360x240 desktop\nregion \"open #1 0,0 1x1\n",
            2,
            "quote",
        ),
        (
            "story element 360x240 desktop\nthing #1 0,0 1x1\n",
            2,
            "role",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1\nregion #1 0,0 2x2\n",
            3,
            "unique #id",
        ),
        (
            "story element 360x240 desktop\nregion #1 stack 0,0 1x1\n",
            2,
            "omit the place",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 0x1\n",
            2,
            "positive dimensions",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1 faded(101%)\n",
            2,
            "faded(0% to 100%)",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1 hovered hovered\n",
            2,
            "at most once",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1 | fill=red\n",
            2,
            "fill=#rrggbbaa",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1 | radius=1 radius=2\n",
            2,
            "repeated",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1 | text=\"open\n",
            2,
            "quoted",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1 | widget=button\n",
            2,
            "button role",
        ),
        (
            "story element 360x240 desktop\nregion #1 0,0 1x1 | level=50\n",
            2,
            "widget=slider",
        ),
    ]);
}

#[test]
fn diagnoses_invalid_container_and_effect_lines() {
    rejects([
        (
            "story container 360x240 desktop\ncontainer grid 0,0 320x160 gap=0 max=1\n",
            2,
            "<row|fan>",
        ),
        (
            "story container 360x240 desktop\ncontainer row 0,0 320x160 gap=-1 max=1\n",
            2,
            "gap=2",
        ),
        (
            "story container 360x240 desktop\ncontainer row 0,0 320x160 gap=0 max=1\nitem 40x56 group=\n",
            3,
            "group=name",
        ),
        (
            "story container 360x240 desktop\ncontainer row 0,0 320x160 gap=0 max=1\nitem 1x1 group=a\nitem 1x1 group=a\n",
            4,
            "max item lines",
        ),
        (
            "story effect 360x240 desktop\neffect  114,60 132x120\n",
            2,
            "effect <name>",
        ),
        (
            "story effect 360x240 desktop\neffect glow 114,60 132x120\nextra\n",
            3,
            "end of the file",
        ),
        (
            "story effect 360x240 desktop\neffect glow 114,60 132x120",
            2,
            "end the line with LF",
        ),
    ]);
}

fn rejects<const COUNT: usize>(cases: [(&str, usize, &str); COUNT]) {
    for (source, line, message) in cases {
        let error = source.parse::<Story>().expect_err(source);
        assert_eq!(error.line, line, "{error}");
        assert!(error.to_string().contains(message), "{error}");
    }
}

fn player(text: &str) -> Player {
    let story: Story = text.parse().expect("valid story");
    let mut player = Player::new(&story, window_for(&story), crate::test_data::data()).unwrap();
    player.stub_atlas();
    player
}

fn window_for(story: &Story) -> Window {
    Window {
        size_class: None,
        size: story.header.window,
        scale: 1.0,
        safe_area: Insets::default(),
    }
}

#[test]
fn a_story_draws_an_effect_added_to_the_tuning() {
    let source = EFFECT.replace("glow", "test-glint");
    let story: Story = source.parse().unwrap();
    assert_eq!(story.to_string(), source);
    let mut data = crate::test_data::data();
    let error = Player::new(&story, window_for(&story), Arc::clone(&data))
        .err()
        .unwrap();
    assert_eq!(error, "line 2: no loaded effect is named \"test-glint\"");

    let definition = include_str!("../../../../../../data/beryl3d/effects/glow.json")
        .replace("\"glow\"", "\"test-glint\"");
    Arc::make_mut(&mut data).effects =
        Effects::parse([("test-glint", definition.as_str())]).unwrap();
    let mut player = Player::new(&story, window_for(&story), data).unwrap();
    player.look(true);
    player.seek(player.end() / 2);
    assert!(
        player
            .frame()
            .looks
            .effects
            .iter()
            .any(|look| look.name.as_ref() == "test-glint")
    );
    player.seek(player.end());
    assert!(player.frame().looks.effects.is_empty());
}

#[test]
fn playback_uses_authored_ids_parents_and_widget_paint() {
    let mut player = player(ELEMENT);
    let tree = tree(player.frame(), &TreeOptions::default()).expect("unfiltered tree");
    assert!(
        tree.contains("    region \"Control | panel\" #1 12,12 320x160"),
        "{tree}"
    );
    assert!(
        tree.contains("      button \"Say \\\"yes\\\"\" #2 22,34 160x48 interactive primary"),
        "{tree}"
    );
    let frame = player.frame();
    assert!(!frame.draws.is_empty());
    assert!(!frame.lines.is_empty());
    assert!(frame.undrawn.is_empty());
}

#[test]
fn effect_changes_its_draw_records_on_the_story_clock() {
    let mut player = player(EFFECT);
    player.seek(player.end() / 2);
    let during = player.frame().draws.len();
    player.seek(player.end());
    assert!(during > player.frame().draws.len());
    player.seek(player.end() / 2);
    assert_eq!(during, player.frame().draws.len());
}

#[test]
fn hover_requires_an_uncovered_part_of_the_element() {
    let mut player = player(concat!(
        "story element 360x240 desktop\n",
        "button \"Behind\" #1 12,12 160x48 interactive | widget=button\n",
        "region \"Above\" #2 12,12 160x48 | fill=#272b32ff\n",
    ));
    let error = player.hover("button #1").expect_err("covered control");
    assert!(error.contains("covered"));
}
