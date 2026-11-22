use beryl_protocol::protocol::{CardType, ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::Act;
use beryl_protocol::testing::{ScriptBuilder, object};
use game_probe::story::{Body, Capture, SettingsStory, Story};

#[test]
fn a_table_story_expands_card_lines_and_plays_a_script_into_a_prompt() {
    let mut island = object(
        ObjectId(8),
        ZoneName::Battlefield,
        Side::You,
        Some("Island"),
    );
    island.types = vec![CardType::Land];
    island.tapped = true;
    let script = ScriptBuilder::default()
        .beat([Act::Tap { object: island }])
        .build();
    let script = serde_json::to_string(&script).unwrap();
    let source = format!(
        r#"story table 1600x900 desktop
life yours 14
life theirs 9
  card "Island" #8 battlefield.yours.lands 12,15 70x98 interactive note("two words")
card "Forest {{n}}" #100 battlefield.theirs.lands tapped x40
card #45 hand.theirs
card "Spell" #49 stack.theirs
ability 10 {{"kind":"ability","source":8,"ability":0,"granted":false,"name":"Island","triggered":true,"nth":0,"controller":"you","targets":[],"x":0}}
prompt {{"id":{{"session":1,"seq":1}},"kind":"priority","prompt":{{"revision":0,"purpose":"choose","part":"pick","title":"Choose a card","words":{{"confirm":"","counted":"","decline":"","yes":"","no":"","auto":"","cancel":"","piles":["",""]}},"about":[],"marks":[],"previews":[],"buttons":[],"body":{{"shape":"candidates"}}}},"candidates":[],"halt":false,"cancellable":false,"undo":0,"undoDepth":0,"refused":[]}}
script {script}
"#
    );
    let story = Story::parse(&source).unwrap();
    let Body::Table(table) = story.body else {
        panic!("expected a table story");
    };
    let [start, end] = table.views.as_slice() else {
        panic!("a script adds a view");
    };
    assert_eq!(start.players.you.life, 14);
    assert_eq!(start.players.opponent.life, 9);
    assert_eq!(start.objects[0].slot, ObjectId(8));
    assert!(!start.objects[0].tapped);
    assert_eq!(start.objects[0].note.as_deref(), Some("two words"));
    assert_eq!(start.objects[40].name.as_deref(), Some("Forest 40"));
    assert_eq!(start.objects[40].slot, ObjectId(139));
    assert_eq!(start.objects[41].name, None);
    assert_eq!(start.objects[42].controller, Side::Opponent);
    assert_eq!(start.stack.len(), 2);
    assert_eq!(
        start
            .script
            .as_ref()
            .unwrap()
            .reset
            .as_ref()
            .unwrap()
            .abilities,
        [10]
    );
    assert!(
        end.objects
            .iter()
            .any(|card| card.slot == ObjectId(8) && card.tapped)
    );
    let Status::Deciding { decision } = &end.status else {
        panic!("the final view should open the prompt");
    };
    assert_eq!(decision.prompt.title, "Choose a card");
    let capture = Capture {
        source: "authored".to_owned(),
        at: 101,
        seat: Side::You,
        scale: 1.0,
        catch_up: true,
        messages: table.messages(),
        actions: Vec::new(),
    };
    assert_eq!(capture.messages.len(), 3);
    let written = capture.write(story.header, None, None).unwrap();
    assert!(written.contains("card \"Island\" #8 battlefield.yours.lands tapped |"));
    assert_eq!(
        Story::parse(&written).unwrap().body,
        Body::Capture {
            feature: None,
            capture
        }
    );
}

#[test]
fn a_settings_story_round_trips_editable_feature_inputs() {
    let source = "story feature 393x759 phone\nfeature settings\nscreen game\nconfirm-concede true\ngame-over false\nmusic 19\nmuted true\ngraphics enhanced\n";
    let story = Story::parse(source).unwrap();
    let Body::Settings(settings) = &story.body else {
        panic!("expected settings");
    };
    assert_eq!(
        settings,
        &SettingsStory {
            screen: Some("game".to_owned()),
            confirm_concede: true,
            music: Some(19),
            muted: Some(true),
            graphics: Some("enhanced".to_owned()),
            ..SettingsStory::default()
        }
    );
    assert_eq!(
        Story::parse(&settings.write(story.header, None)).unwrap(),
        story
    );
}

#[test]
fn invalid_card_ids_report_the_responsible_line() {
    let source = "story table 1600x900 desktop\ncard \"Island\" #8 hand.yours x2\ncard \"Plains\" #9 stack\n";
    assert_eq!(
        Story::parse(source).unwrap_err(),
        "line 3: card id #9 is already used"
    );
}
