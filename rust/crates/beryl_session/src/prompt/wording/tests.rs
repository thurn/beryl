use std::collections::BTreeSet;

use beryl_engine::Zone;
use beryl_engine::card::{Part, Place, Prompt, Purpose, Shape};

use super::{
    AIMED, Holes, OWN_TARGET, PassLeads, Wording, cancel_word, pass_words, pile_word, wording,
};
use crate::protocol::Step;

/// The holes a template may have.
const HOLES: [&str; 10] = [
    "count",
    "most",
    "most:words",
    "cost",
    "source",
    "name",
    "from",
    "picked",
    "of",
    "s",
];

const PARTS: [Part; 8] = [
    Part::Pick,
    Part::Whether,
    Part::Instead,
    Part::Count,
    Part::First,
    Part::Second,
    Part::Aim,
    Part::Plan,
];
const PLACES: [Place; 8] = [
    Place::None,
    Place::Top,
    Place::OnTop,
    Place::Bottom,
    Place::Hand,
    Place::Graveyard,
    Place::Exile,
    Place::Battlefield,
];
const ZONES: [Option<Zone>; 4] = [
    None,
    Some(Zone::Library),
    Some(Zone::Hand),
    Some(Zone::Graveyard),
];

/// Every way the table words `purpose`.
fn entries(purpose: Purpose) -> impl Iterator<Item = Wording> {
    PARTS.into_iter().flat_map(move |part| {
        PLACES.into_iter().flat_map(move |place| {
            ZONES.into_iter().flat_map(move |zone| {
                (0..=3).map(move |most| {
                    let prompt = Prompt::new(purpose)
                        .part(part)
                        .places(place, Place::None)
                        .most(most);
                    wording(&prompt, zone)
                })
            })
        })
    })
}

fn templates(w: &Wording) -> [&'static str; 8] {
    [
        w.title, w.unnamed, w.confirm, w.counted, w.decline, w.yes, w.no, w.auto,
    ]
}

/// The holes of `template`, mana symbols aside.
fn holes(template: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').expect("a hole closes") + open;
        let hole = &rest[open + 1..close];
        if hole != "0" {
            found.push(hole);
        }
        rest = &rest[close + 1..];
    }
    found
}

#[test]
fn a_template_has_only_the_holes_the_table_fills() {
    for &purpose in Purpose::ALL {
        for entry in entries(purpose) {
            for template in templates(&entry) {
                for hole in holes(template) {
                    assert!(HOLES.contains(&hole), "{purpose:?}: {template:?}");
                }
            }
        }
    }
}

/// The table has words for every purpose and for each button its shape shows, in every way
/// it words the purpose.
#[test]
fn every_purpose_and_button_has_words() {
    for &purpose in Purpose::ALL {
        assert!(!cancel_word(purpose).is_empty(), "{purpose:?}");
        for entry in entries(purpose) {
            // The turn button is the priority prompt, and says where passing leads.
            assert_eq!(
                entry.title.is_empty(),
                purpose == Purpose::Priority,
                "{purpose:?}"
            );
            let shown: &[(&str, &str)] = match purpose.shape() {
                _ if purpose == Purpose::Priority => &[("decline", entry.decline)],
                Shape::Pick => &[("confirm", entry.confirm), ("counted", entry.counted)],
                Shape::Order | Shape::Split | Shape::Number | Shape::Catalog | Shape::Divide => {
                    &[("confirm", entry.confirm)]
                }
                Shape::YesNo => &[("yes", entry.yes), ("no", entry.no)],
                // A mode's buttons are the decision's candidates, worded with them.
                Shape::Mode => &[],
                Shape::Pay => &[("auto", entry.auto)],
                Shape::Declare => &[
                    ("confirm", entry.confirm),
                    ("counted", entry.counted),
                    ("decline", entry.decline),
                ],
            };
            for (button, words) in shown {
                assert!(!words.is_empty(), "{purpose:?} has no words for {button}");
            }
        }
        if purpose.shape() == Shape::Split {
            for place in PLACES {
                // No place has no word, but for the cards that stay where a split found them:
                // the hand cards are put back from.
                let nowhere = place == Place::None && purpose != Purpose::PutBack;
                assert_eq!(
                    pile_word(purpose, place).is_empty(),
                    nowhere,
                    "{purpose:?} {place:?}"
                );
            }
        }
    }
}

/// A title with a hole that may have nothing to fill it has another for then, and a yes
/// always comes with a no.
#[test]
fn a_title_never_comes_out_empty_for_want_of_a_name() {
    let nameless = Holes::default();
    for &purpose in Purpose::ALL {
        for entry in entries(purpose) {
            let named = ["{source}", "{name}", "{cost}"]
                .iter()
                .any(|hole| entry.title.contains(hole));
            assert_eq!(named, !entry.unnamed.is_empty(), "{purpose:?}");
            assert_eq!(
                nameless.title(&entry).is_empty(),
                entry.title.is_empty(),
                "{purpose:?}"
            );
            assert_eq!(entry.yes.is_empty(), entry.no.is_empty(), "{purpose:?}");
        }
    }
}

#[test]
fn holes_are_filled_from_the_record_and_the_view() {
    let holes = Holes {
        count: 3,
        most: 2,
        cost: Some("{2}{U}"),
        from: Some(Zone::Graveyard),
        ..Holes::default()
    };
    let fill = |template| holes.fill(template);
    assert_eq!(
        fill("Discard {most:words} cards.").as_deref(),
        Some("Discard two cards.")
    );
    assert_eq!(
        fill("Discard {most} cards.").as_deref(),
        Some("Discard 2 cards.")
    );
    assert_eq!(fill("Pay {cost}.").as_deref(), Some("Pay {2}{U}."));
    assert_eq!(
        fill("Exile that card{from}?").as_deref(),
        Some("Exile that card from your graveyard?")
    );
    // The state's holes stay for whoever holds the state.
    assert_eq!(
        fill("{picked} Attacker{s}").as_deref(),
        Some("{picked} Attacker{s}")
    );
    // A card with no name here leaves its title to the unnamed one, and so does no cost.
    assert_eq!(fill("{source}"), None);
    let pay_life = wording(&Prompt::new(Purpose::PayLife).count(3), None);
    assert_eq!(holes.title(&pay_life), "Pay 3 life?");
    let unless = wording(&Prompt::new(Purpose::UnlessPays), None);
    assert_eq!(holes.title(&unless), "Pay {2}{U}.");
    assert_eq!(Holes::default().title(&unless), "Pay {0}.");
}

const STEPS: [Step; 13] = [
    Step::Untap,
    Step::Upkeep,
    Step::Draw,
    Step::PrecombatMain,
    Step::BeginningOfCombat,
    Step::DeclareAttackers,
    Step::DeclareBlockers,
    Step::FirstStrikeDamage,
    Step::CombatDamage,
    Step::EndOfCombat,
    Step::PostcombatMain,
    Step::End,
    Step::Cleanup,
];

/// Every place a pass leads, on the viewer's turn and on an opponent's.
fn passes() -> impl Iterator<Item = (&'static str, &'static str)> {
    let leads = [
        PassLeads::Halt,
        PassLeads::Resolve,
        PassLeads::CombatDamage,
        PassLeads::NextTurn,
    ]
    .into_iter()
    .chain(STEPS.map(PassLeads::Step));
    leads.flat_map(|leads| [true, false].map(|yours| pass_words(leads, yours)))
}

/// The pass button always has a label, and a caption wherever passing leads on from the
/// stack.
#[test]
fn the_pass_button_has_words_wherever_it_leads() {
    for (label, caption) in passes() {
        assert!(!label.is_empty());
        assert_eq!(caption.is_empty(), label == "Resolve", "{label}");
    }
}

/// Every template of the table: the piles', the cancel's and the pass button's words, and
/// the target prompt's own, among them.
fn table() -> BTreeSet<&'static str> {
    let mut words = BTreeSet::new();
    words.extend(passes().flat_map(|(label, caption)| [label, caption]));
    words.extend(templates(&OWN_TARGET));
    words.extend(templates(&AIMED));
    for &purpose in Purpose::ALL {
        words.extend(entries(purpose).flat_map(|entry| templates(&entry)));
        words.extend(PLACES.map(|place| pile_word(purpose, place)));
        words.insert(cancel_word(purpose));
    }
    words
}

/// The words in double quotes in `text`.
fn quoted(text: &str) -> Vec<&str> {
    text.split('"').skip(1).step_by(2).collect()
}

/// The entries of `data/ui-conventions.json` that list the words Arena does not show quote
/// the table: a composed entry reworded here is reworded there.
#[test]
fn ui_conventions_quotes_the_tables_composed_words() {
    // Read at run time: a test binary from the shared target directory may have been built in
    // a worktree that no longer exists.
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../../../data/ui-conventions.json");
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the conventions' data"))
            .expect("the conventions are JSON");
    let table = table();
    let mut listed = 0;
    for entry in data["entries"].as_array().expect("entries") {
        let name = entry["name"].as_str().expect("a name");
        if entry["kind"] != "not-seen" || !name.starts_with("Words:") {
            continue;
        }
        for words in quoted(entry["rule"].as_str().expect("a rule")) {
            assert!(table.contains(words), "{name}: the table has no {words:?}");
            listed += 1;
        }
    }
    assert!(listed > 0, "no entry lists the composed words");
}
