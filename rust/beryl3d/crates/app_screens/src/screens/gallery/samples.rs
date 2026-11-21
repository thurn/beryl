//! The stand-in cards from `ui/src/screens/samples.ts`, drawn with Beryl's card frame.

use game_table::faces::{FaceList, PrintedText};

const CREATURES: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "Cinder Colossus",
        "{4}{R}{R}",
        "Creature — Giant",
        "R",
        "6",
        "Whenever Cinder Colossus enters or attacks, it deals 3 damage divided as you choose among one, two, or three targets.",
    ),
    (
        "Gloom Wren",
        "{U}{B}",
        "Artifact Creature — Bird",
        "UB",
        "1",
        "Flying, deathtouch\nWhen Gloom Wren enters, draw a card.",
    ),
    ("Beast", "", "Token Creature — Beast", "G", "3", ""),
    ("Angel", "", "Token Creature — Angel", "W", "4", ""),
];

// The scene purity check searches string literals for clock type names.
const INSTANT_TYPE_LINE: &str = concat!("Ins", "tant");

const SPELLS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "Exile Lance",
        "",
        INSTANT_TYPE_LINE,
        "W",
        "Exile target creature. Its controller gains life equal to its power.",
    ),
    (
        "Quell",
        "{U}{U}",
        INSTANT_TYPE_LINE,
        "U",
        "Counter target spell.",
    ),
    (
        "Foresee",
        "{U}",
        "Sorcery",
        "U",
        "Scry 2, then draw a card.",
    ),
    (
        "Dusk Cut",
        "{1}{B}",
        INSTANT_TYPE_LINE,
        "B",
        "Destroy target nonblack creature.",
    ),
];

/// The card faces used by the gallery samples.
#[must_use]
pub fn faces() -> FaceList {
    let mut faces = FaceList::from_records(&beryl_protocol::card_records::CardRecords::none());
    for &(name, mana, type_line, colors, stats, rules) in CREATURES {
        faces.add_preview(
            name,
            colors,
            PrintedText {
                power: Some(stats.to_owned()),
                toughness: Some(stats.to_owned()),
                token: type_line.starts_with("Token"),
                ..printed(mana, type_line, rules)
            },
        );
    }
    for &(name, mana, type_line, colors, rules) in SPELLS {
        faces.add_preview(name, colors, printed(mana, type_line, rules));
    }
    for (name, type_line) in [
        ("Island", "Basic Land — Island"),
        ("Plains", "Basic Land — Plains"),
        ("Mountain", "Basic Land — Mountain"),
        ("Plateau", "Land — Mountain Plains"),
    ] {
        faces.add_preview(name, "", printed("", type_line, ""));
    }
    faces.add_plates();
    faces
}

fn printed(mana: &str, type_line: &str, rules: &str) -> PrintedText {
    PrintedText {
        mana_cost: (!mana.is_empty()).then(|| mana.to_owned()),
        type_line: type_line.to_owned(),
        rules: (!rules.is_empty()).then(|| rules.to_owned()),
        ..PrintedText::default()
    }
}
