use super::*;

#[test]
fn every_topic_fits_on_one_screen() {
    for topic in TOPICS {
        let lines: Vec<&str> = topic.text.lines().collect();
        assert!(
            lines.len() <= MAX_LINES,
            "{} has {} lines",
            topic.name,
            lines.len()
        );
        for line in &lines {
            assert!(
                line.chars().count() <= MAX_COLUMNS,
                "{} has a line over {MAX_COLUMNS} columns: {line}",
                topic.name
            );
        }
    }
}

#[test]
fn the_list_has_one_line_per_topic_within_the_width() {
    let list = list();
    for topic in TOPICS {
        let line = list
            .lines()
            .find(|l| l.split_whitespace().next() == Some(topic.name))
            .unwrap_or_else(|| panic!("{} in {list}", topic.name));
        assert!(line.contains(topic.summary));
        assert!(line.chars().count() <= MAX_COLUMNS, "{line}");
    }
}

#[test]
fn a_topic_prints_its_text_and_an_unknown_one_lists_them_all() {
    let review = reply(Some("review"));
    assert_eq!(review.exit, Exit::Success);
    assert!(review.text.starts_with("REVIEW"));

    let unknown = reply(Some("reveiw"));
    assert_eq!(unknown.exit, Exit::Usage);
    assert!(unknown.text.contains("machine"));

    assert_eq!(reply(None).text, list());
}

#[test]
fn topic_names_are_unique() {
    for (i, topic) in TOPICS.iter().enumerate() {
        assert!(
            TOPICS[i + 1..].iter().all(|other| other.name != topic.name),
            "{} twice",
            topic.name
        );
    }
}

#[test]
fn the_old_name_of_the_flaky_checks_topic_still_prints_it() {
    let old = reply(Some("checks"));
    assert_eq!(old.exit, Exit::Success);
    assert_eq!(old.text, reply(Some("flaky-checks")).text);
}
