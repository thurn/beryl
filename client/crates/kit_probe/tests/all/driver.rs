//! How the driver reads actions and moments.

use glam::Vec2;
use kit_probe::driver::{Action, Duration, KeyChord, Moment, Name, Offset};
use kit_ui::input::KeyCode;

fn ms(milliseconds: u64) -> Duration {
    Duration::from_millis(milliseconds)
}

#[test]
fn durations_parse_and_print_milliseconds() {
    assert_eq!("850ms".parse(), Ok(ms(850)));
    assert_eq!("1.25s".parse(), Ok(ms(1250)));
    assert_eq!("2s".parse::<Duration>().unwrap().to_string(), "2000ms");
    assert_eq!(Duration(1_500_000).to_string(), "1.5ms");
    for wrong in ["85", "ms", "-3s", "1.2.3s", "fast"] {
        let error = wrong.parse::<Duration>().unwrap_err().to_string();
        assert!(error.contains("850ms or 1.25s"), "{error}");
        assert!(error.contains(wrong), "{error}");
    }
}

#[test]
fn moments_parse_and_print_as_written() {
    for text in [
        "start",
        "end",
        "view:12",
        "prompt:1",
        "step:12.0.1",
        "view:12+250ms",
        "prompt:2-100ms",
        "850ms",
    ] {
        let moment: Moment = text.parse().unwrap();
        assert_eq!(moment.to_string(), text);
    }
    assert_eq!(
        "view:3+1.25s".parse(),
        Ok(Moment::Named {
            name: Name::View(3),
            offset: Offset {
                before: false,
                length: ms(1250),
            },
        })
    );
    for (wrong, says) in [
        ("view:0", "counted from 1"),
        ("step:1.2", "a view, a beat and a step"),
        ("frame:3", "not a moment's name"),
        ("view:x", "not a whole number"),
        ("view:2+soon", "850ms or 1.25s"),
    ] {
        let error = wrong.parse::<Moment>().unwrap_err().to_string();
        assert!(error.contains(says), "{wrong}: {error}");
        assert!(error.contains("view:12+250ms"), "{wrong}: {error}");
    }
}

#[test]
fn every_action_name_is_a_line_run_accepts() {
    for (line, action) in [
        (
            "press button \"Auto Pay\"",
            Action::Press("button \"Auto Pay\"".to_owned()),
        ),
        ("hover card #8", Action::Hover("card #8".to_owned())),
        ("menu card #8", Action::Menu("card #8".to_owned())),
        (
            "drag card \"Go to Sleep\" to region \"stack\"",
            Action::Drag(
                "card \"Go to Sleep\"".to_owned(),
                "region \"stack\"".to_owned(),
            ),
        ),
        (
            "hold card #8 over pile \"Bottom pile\"",
            Action::Hold("card #8".to_owned(), "pile \"Bottom pile\"".to_owned()),
        ),
        (
            "drag 500,400 to card #8",
            Action::Drag("500,400".to_owned(), "card #8".to_owned()),
        ),
        (
            "type field \"Deck name\" \"WU \\\"Control\\\"\"",
            Action::Type(
                "field \"Deck name\"".to_owned(),
                "WU \"Control\"".to_owned(),
            ),
        ),
        ("key escape", Action::Key(KeyChord::plain(KeyCode::Escape))),
        ("key Q", Action::Key(KeyChord::plain(KeyCode::Q))),
        (
            "key Cmd+Shift+B",
            Action::Key(KeyChord {
                cmd: true,
                shift: true,
                ..KeyChord::plain(KeyCode::B)
            }),
        ),
        ("move 400,300", Action::Move(Vec2::new(400.0, 300.0))),
        (
            "wheel list \"Opponent\" 0,-400",
            Action::Wheel("list \"Opponent\"".to_owned(), Vec2::new(0.0, -400.0)),
        ),
        ("step 1.25s", Action::Step(ms(1250))),
        ("still", Action::Still),
    ] {
        assert_eq!(line.parse(), Ok(action), "{line}");
    }
    assert_eq!(
        "type field \"Deck name\" \"WU Control\"".parse(),
        Ok(Action::Type(
            "field \"Deck name\"".to_owned(),
            "WU Control".to_owned()
        ))
    );
    let error = "jump card #3".parse::<Action>().unwrap_err().to_string();
    assert!(error.contains("press <locator>"), "{error}");
    let error = "key F13".parse::<Action>().unwrap_err().to_string();
    assert!(error.contains("not a key's name"), "{error}");
}
