//! The components and the Markdown renderer against the outputs of the Python original, kept in
//! `fixtures/cirrus.json`.
#![allow(clippy::format_collect)]

use std::collections::BTreeMap;

use ignite_hub::dashboard::assets::Assets;
use ignite_hub::dashboard::components::{self as c, Feed, PageOptions, Tone};
use ignite_hub::dashboard::markdown::{inline, render};
use serde_json::Value;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/cirrus.json")).unwrap()
}

#[test]
fn components_match_the_python_renderer() {
    let fixtures = fixtures();
    let assets = Assets {
        css: fixtures["assets"]["cirrus.css"]
            .as_str()
            .unwrap()
            .to_owned(),
        js: fixtures["assets"]["cirrus.js"].as_str().unwrap().to_owned(),
    };
    let text = "A & <B> \"C\" 'D'";
    let body = "<strong>Example</strong>";
    let url = "/ticket?x=\"one\"&y=two";
    let plain = PageOptions {
        title: text.into(),
        body: body.into(),
        assets: Some(assets.clone()),
        ..PageOptions::default()
    };
    let full = PageOptions {
        title: text.into(),
        body: body.into(),
        root: "../".into(),
        assets: Some(assets),
        crumbs: vec![(text.into(), Some(url.into())), ("Here".into(), None)],
        brand: "Enact & Co.".into(),
        description: text.into(),
        base: Some("https://example.com/enact/".into()),
        ..PageOptions::default()
    };
    let document = |options: &PageOptions| c::document(options, &c::page_regions(options));
    let both = || [body.to_owned(), body.to_owned()];
    let mut outputs: BTreeMap<&str, String> = BTreeMap::new();
    outputs.insert("esc", c::esc(text));
    outputs.insert("escNull", c::esc(""));
    outputs.insert("page", document(&plain));
    outputs.insert("pageWithOptions", document(&full));
    outputs.insert("title", c::title(text, body, body));
    outputs.insert("titlePlain", c::title(text, "", ""));
    outputs.insert("chip", c::chip(text, Tone::Neutral, None));
    outputs.insert("chipWithTip", c::chip(text, Tone::Green, Some(text)));
    outputs.insert("dot", c::dot(Tone::Red, text, Some("")));
    outputs.insert("mono", c::mono(text));
    outputs.insert("muted", c::muted(body));
    outputs.insert("tip", c::tip(body, text));
    outputs.insert("time", c::time(Some("2026-11-16T12:35:00Z"), ""));
    outputs.insert("timeFallback", c::time(None, text));
    outputs.insert("link", c::link(url, body, Some(text), "c-extra"));
    outputs.insert("template", c::template(text, body));
    outputs.insert("part", c::part(text, body, "active", Some(0)));
    outputs.insert("section", c::section(text, body, Some(2), Some("open"), 3));
    outputs.insert("sectionUntitled", c::section("", body, None, None, 2));
    outputs.insert("subsection", c::subsection(text, body, Some(0), 4));
    outputs.insert("grid", c::grid(both()));
    outputs.insert("card", c::card(url, body, Some(text)));
    outputs.insert("rows", c::rows(both()));
    outputs.insert("row", c::row(body, body, Some(url)));
    outputs.insert("rowPlain", c::row(body, "", None));
    outputs.insert(
        "meta",
        c::meta(&[(text.into(), body.into()), ("Next".into(), body.into())]),
    );
    outputs.insert("progress", c::progress(1, 8, Some(text)));
    outputs.insert("progressEmpty", c::progress(0, 0, None));
    outputs.insert("disclosure", c::disclosure(body, body, false));
    outputs.insert("disclosureOpen", c::disclosure(body, body, true));
    outputs.insert("skeleton", c::skeleton(8));
    outputs.insert("include", c::include(url, text, 2));
    outputs.insert("empty", c::empty(text));
    outputs.insert("prose", c::prose(body));
    outputs.insert("layout", c::layout(body, body));
    outputs.insert("timeline", c::timeline(both()));
    outputs.insert(
        "timelineEntry",
        c::timeline_entry(body, "today", body, true),
    );
    outputs.insert(
        "timelineEntryPlain",
        c::timeline_entry(body, "today", "", false),
    );
    outputs.insert("comment", c::comment("  𐐨 writer", body, "today", body));
    outputs.insert("commentAnonymous", c::comment("", body, "today", body));
    outputs.insert(
        "toc",
        c::toc(&[
            (text.into(), "active".into(), Some(0), 0),
            ("Next".into(), text.into(), None, 1),
        ]),
    );
    outputs.insert("withToc", c::with_toc(body, body));

    let expected = fixtures["components"].as_object().unwrap();
    assert_eq!(outputs.len(), expected.len(), "every fixture has a case");
    for (name, output) in &outputs {
        assert_eq!(
            output,
            expected[*name].as_str().unwrap(),
            "{name} matches the Python renderer"
        );
    }
}

#[test]
fn markdown_matches_the_python_renderer() {
    for case in fixtures()["markdown"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let (kind, source) = (
            case["kind"].as_str().unwrap(),
            case["source"].as_str().unwrap(),
        );
        let expected = case["html"].as_str().unwrap();
        let autolink = |text: &str| text.replace("by-x", "<a>by-x</a>");
        let identity = |text: &str| text.to_owned();
        let actual = match kind {
            "render" => render(source, &identity, &identity),
            "autolink" => inline(source, &autolink),
            _ => inline(source, &identity),
        };
        assert_eq!(actual, expected, "{name} matches the Python renderer");
    }
}

#[test]
fn literal_private_use_characters_survive_alongside_code_spans() {
    assert_eq!(
        inline("\u{E000}0\u{E000} and `code`", &|text| text.to_owned()),
        "\u{E000}0\u{E000} and <code>code</code>"
    );
}

#[test]
fn feed_attributes_name_the_position_and_the_tickets() {
    let options = PageOptions {
        root: "/p/".into(),
        feed: Some(Feed {
            seq: "7".into(),
            tickets: Some(vec!["a".into(), "a.1".into()]),
        }),
        ..PageOptions::default()
    };
    assert!(c::page_regions(&options).contains(
        "<main class=\"c-main\" id=\"c-main\" data-feed-seq=\"7\" data-feed-tickets=\"a a.1\">"
    ));
}

/// An attachment of a ticket, as the store returns it.
fn attachment(name: &str, removed: Option<&str>) -> ignite_core::protocol::attachments::Attachment {
    ignite_core::protocol::attachments::Attachment {
        ticket: "T1".parse().unwrap(),
        name: name.to_owned(),
        sha256: "ab".repeat(32),
        media_type: "image/png".to_owned(),
        bytes: 4096,
        width: 640,
        height: 480,
        caption: None,
        created_at: ignite_core::time::Timestamp::parse("2026-11-20T06:00:00Z").unwrap(),
        created_by: "tester".to_owned(),
        removed_at: removed.map(|at| ignite_core::time::Timestamp::parse(at).unwrap()),
    }
}

#[test]
fn attachment_images_render_only_for_the_tickets_own_attachments() {
    use ignite_hub::dashboard::attachments::embedded;
    use ignite_hub::dashboard::markdown::render_with_images;
    let list = [
        attachment("shot.png", None),
        attachment("old.png", Some("2026-12-01T00:00:00Z")),
    ];
    let render = |text: &str, private: bool| {
        let images = |alt: &str, name: &str| embedded(&list, name, alt, private);
        render_with_images(
            text,
            &|t: &str| t.to_owned(),
            &|t: &str| t.to_owned(),
            &images,
        )
    };
    let sha = "ab".repeat(32);
    assert_eq!(
        render("See ![the <board>](attachment:shot.png).", true),
        format!(
            "<p>See <a class=\"c-attachment\" href=\"/attachments/{sha}\" target=\"_blank\" \
             rel=\"noopener\"><img src=\"/attachments/{sha}\" alt=\"the &lt;board&gt;\" \
             loading=\"lazy\" width=\"640\" height=\"480\"></a>.</p>"
        )
    );
    assert_eq!(
        render("![x](attachment:shot.png)", false),
        "<p><span class=\"c-muted\">Image, visible on the private dashboard</span></p>"
    );
    assert_eq!(
        render("![x](attachment:nope.png)", true),
        "<p>[missing attachment: nope.png]</p>"
    );
    assert_eq!(
        render("![x](attachment:old.png)", true),
        "<p>[image removed 2026-12-01: old.png]</p>"
    );
    // An image from elsewhere stays a link, so a ticket cannot make the page load it.
    assert_eq!(
        render("![x](https://example.com/a.png)", true),
        "<p>!<a href=\"https://example.com/a.png\">x</a></p>"
    );
    // Inside code it is text, and without the hook it is never an image.
    assert_eq!(
        render("`![x](attachment:shot.png)`", true),
        "<p><code>![x](attachment:shot.png)</code></p>"
    );
    assert!(!render_html("![x](attachment:shot.png)").contains("<img"));
}

fn render_html(text: &str) -> String {
    render(text, &|t: &str| t.to_owned(), &|t: &str| t.to_owned())
}
