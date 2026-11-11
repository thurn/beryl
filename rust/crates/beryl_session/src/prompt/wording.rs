//! The words of every prompt: one table keyed by purpose ([`wording`]), and the filling of
//! its templates ([`Holes`]).
//!
//! An entry is a prompt's title and the labels of its buttons. The match on the purpose is
//! exhaustive, so a purpose without words does not compile. Where one purpose is worded two
//! ways, the entry turns on the rest of the record: the part asked, the place a card goes, the
//! zone the cards lie in.
//!
//! A template's holes are a closed set, filled from the record and the view:
//!
//! - `{count}` and `{most}`, the record's numbers, and `{Count}`, its count in words ("Three");
//! - `{cost}`, the record's mana cost as Oracle text writes it;
//! - `{source}`, the name of the spell or ability that asks;
//! - `{name}`, the name of the card asked about, and `{land}`, the name of its land face;
//! - `{Whose}`, whose cards the prompt shows ("Your", "Opponent's");
//! - `{from}`, where the card asked about lies when not on the battlefield (" from your
//!   graveyard"), or nothing;
//! - `{picked}` and `{of}`, how many are picked and how many the prompt takes: the prompt's
//!   state, left as written for whoever holds it.
//!
//! The strings are those the UI showed before the table (`choices.ts`, `tablePick.ts`,
//! `decisions.ts` and `Board.tsx`'s prompt), unchanged; each is still to be checked against
//! Arena (plan K22).

use beryl_engine::Zone;
use beryl_engine::card::{Part, Place, Prompt, Purpose};

/// A purpose's words: templates, each empty where the prompt has no such thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wording {
    /// What the prompt asks.
    pub title: &'static str,
    /// What it asks when a card its title names has no name the viewer knows.
    pub unnamed: &'static str,
    /// The button that answers with what is picked.
    pub confirm: &'static str,
    /// That button where the prompt takes several and counts them.
    pub counted: &'static str,
    /// The button that answers with nothing.
    pub decline: &'static str,
    /// The button that answers yes, and the one that answers no.
    pub yes: &'static str,
    pub no: &'static str,
    /// The button that pays as the session suggests.
    pub auto: &'static str,
}

const NONE: Wording = Wording {
    title: "",
    unnamed: "",
    confirm: "",
    counted: "",
    decline: "",
    yes: "",
    no: "",
    auto: "",
};

const fn titled(title: &'static str) -> Wording {
    Wording { title, ..NONE }
}

/// The words of the prompt `prompt` belongs to, whose cards lie in `zone`.
#[allow(clippy::too_many_lines)]
pub(crate) const fn wording(prompt: &Prompt, zone: Option<Zone>) -> Wording {
    let Prompt {
        purpose,
        part,
        most,
        places: [to, _],
        ..
    } = *prompt;
    match purpose {
        // The turn button passes, by where passing leads: the prompt has no title.
        Purpose::Priority => Wording {
            decline: "Pass",
            ..NONE
        },
        Purpose::Target => Wording {
            // A count of up to one target is taken with the target itself.
            title: match (part, most) {
                (Part::Count, 2..) => "How many targets?",
                _ => "Choose a target",
            },
            decline: "No Target",
            ..NONE
        },
        Purpose::Choose => Wording {
            title: "Choose a card",
            confirm: "Choose",
            ..NONE
        },
        Purpose::Discard => Wording {
            title: match part {
                Part::Instead => "Discard a basic land or {most} cards",
                _ => "Discard a card",
            },
            confirm: "Discard",
            counted: "Discard {picked}/{of}",
            ..NONE
        },
        // Down to the hand size, its picks counted even when one is left.
        Purpose::Cleanup => Wording {
            title: "Discard a card",
            confirm: "Discard {picked}/{of}",
            counted: "Discard {picked}/{of}",
            ..NONE
        },
        Purpose::PutOnBottom => titled("Put a card on the bottom of your library"),
        Purpose::LegendRule => Wording {
            title: "Choose the legend to keep",
            confirm: "Keep",
            ..NONE
        },
        Purpose::Look => {
            let title = match zone {
                Some(Zone::Library) => "Top {Count}",
                _ => "{Whose} Hand",
            };
            let (confirm, decline) = match to {
                Place::Hand => ("Put in Hand", "Take Nothing"),
                Place::Graveyard => ("Graveyard", "Graveyard Nothing"),
                Place::Exile => ("Exile", "Exile Nothing"),
                Place::Battlefield => ("Battlefield", "Battlefield Nothing"),
                Place::Top | Place::OnTop | Place::Bottom => ("Library", "Library Nothing"),
                Place::None => ("Choose", "Take Nothing"),
            };
            Wording {
                title,
                confirm,
                decline,
                ..NONE
            }
        }
        Purpose::Search => Wording {
            title: "Search",
            confirm: "Find",
            counted: "Find {picked}/{of}",
            decline: "Find Nothing",
            ..NONE
        },
        Purpose::Untap => Wording {
            title: "Untap",
            confirm: "Untap {picked}/{of}",
            counted: "Untap {picked}/{of}",
            decline: "Untap None",
            ..NONE
        },
        Purpose::ManifestDread => Wording {
            title: "Manifest Dread",
            confirm: "Manifest",
            ..NONE
        },
        Purpose::ExileCost => Wording {
            // A graveyard's cards are laid out under the one word; a hand's are picked where
            // they lie.
            title: match zone {
                Some(Zone::Graveyard) => "Exile",
                _ => "Exile a card",
            },
            confirm: "Exile",
            ..NONE
        },
        Purpose::CombatDamage => Wording {
            title: "Assign combat damage",
            confirm: "Assign Damage",
            decline: "Done",
            ..NONE
        },
        Purpose::TriggerOrder => titled("Put a trigger on the stack"),
        Purpose::Scry => titled("Scry"),
        Purpose::Arrange => titled("Top {Count}"),
        Purpose::PutBack => titled("Put Back"),
        Purpose::May => Wording {
            title: "{source}",
            unnamed: "Choose",
            yes: "Yes",
            no: "No",
            ..NONE
        },
        Purpose::MaySearch => Wording {
            title: "Search?",
            yes: "Search",
            no: "Don't Search",
            ..NONE
        },
        Purpose::Move => {
            let (title, yes) = match to {
                Place::Exile => ("Exile that card{from}?", "Exile"),
                Place::Hand => ("Put that card into your hand{from}?", "Put into hand"),
                Place::Graveyard => (
                    "Put that card into your graveyard{from}?",
                    "Put into graveyard",
                ),
                Place::Battlefield => (
                    "Put that card onto the battlefield{from}?",
                    "Put onto battlefield",
                ),
                Place::Top | Place::OnTop | Place::Bottom => {
                    ("Put that card into your library{from}?", "Put into library")
                }
                Place::None => ("Move that card{from}?", "Move"),
            };
            Wording {
                title,
                yes,
                no: "Decline",
                ..NONE
            }
        }
        Purpose::Fateseal => Wording {
            title: "Top of Library",
            yes: "Put on Bottom",
            no: "Keep on Top",
            ..NONE
        },
        Purpose::TopOrBottom => Wording {
            title: "Top or Bottom?",
            yes: "Top",
            no: "Bottom",
            ..NONE
        },
        Purpose::Explore => Wording {
            title: "Revealed",
            yes: "Graveyard",
            no: "Keep on Top",
            ..NONE
        },
        Purpose::UnlessPays => Wording {
            title: "Pay {cost}?",
            yes: "Pay {cost}",
            no: "Don't Pay",
            ..NONE
        },
        // Asked of the land by name.
        Purpose::PayLife => Wording {
            title: "{land}",
            unnamed: "Pay {count} Life?",
            yes: "Pay {count} Life",
            no: "Tapped",
            ..NONE
        },
        Purpose::X => titled("Choose X"),
        Purpose::PlayOrDraw => titled("Play or draw?"),
        Purpose::Mulligan => titled("Keep or mulligan?"),
        Purpose::CastOffer => Wording {
            title: "Cast {name}?",
            unnamed: "Cast It?",
            confirm: "Cast",
            decline: "Don't Cast",
            ..NONE
        },
        // The ways to use one card, under its name.
        Purpose::Act => Wording {
            title: "{name}",
            unnamed: "Choose",
            ..NONE
        },
        // No card asks the next five yet: their words are the worked examples' of
        // `tests/prompt.rs`.
        Purpose::Color => titled("Choose a Color"),
        Purpose::CreatureType => titled("Choose a Creature Type"),
        Purpose::CardName => titled("Name a Card"),
        Purpose::DivideDamage => titled("Divide {count} Damage"),
        Purpose::DistributeCounters => titled("Distribute {count} Counters"),
        Purpose::Mana => Wording {
            title: match part {
                Part::Plan => "Choose how to pay",
                _ => "Pay the mana",
            },
            auto: "Auto Pay",
            ..NONE
        },
        Purpose::Crew => Wording {
            title: "Crew",
            auto: "Auto Pay",
            ..NONE
        },
        Purpose::Waterbend => Wording {
            title: "Tap permanents to Waterbend",
            auto: "Auto Pay",
            ..NONE
        },
        Purpose::Attack => Wording {
            title: match part {
                Part::Aim => "Choose attack target",
                _ => "Declare attackers",
            },
            confirm: "Attack",
            decline: "No Attacks",
            ..NONE
        },
        Purpose::Block => Wording {
            title: match part {
                Part::Aim => "Choose what to block",
                _ => "Declare blockers",
            },
            confirm: "Block",
            decline: "No Blocks",
            ..NONE
        },
    }
}

/// What a place is called over its pile of a split.
pub(crate) const fn place_word(place: Place) -> &'static str {
    match place {
        Place::None => "",
        Place::Top | Place::OnTop => "Top",
        Place::Bottom => "Bottom",
        Place::Hand => "Hand",
        Place::Graveyard => "Graveyard",
        Place::Exile => "Exile",
        Place::Battlefield => "Battlefield",
    }
}

/// What fills a template's holes (the module comment lists them): the record's numbers and
/// cost, and the names and places the view gives.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Holes<'a> {
    pub count: u8,
    pub most: u8,
    pub cost: &'a str,
    pub source: Option<&'a str>,
    pub name: Option<&'a str>,
    pub land: Option<&'a str>,
    /// Whether the cards shown are the viewer's.
    pub yours: bool,
    /// The zone the card asked about lies in.
    pub from: Option<Zone>,
}

/// Small numbers in words, as rules text writes them.
const NUMBERS: [&str; 11] = [
    "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
];

impl Holes<'_> {
    /// `template` with its holes filled, or None if it names a card that has no name here.
    pub(crate) fn fill(&self, template: &str) -> Option<String> {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            let (before, hole) = rest.split_at(open);
            out.push_str(before);
            let close = hole.find('}')?;
            let (name, after) = (&hole[1..close], &hole[close + 1..]);
            match name {
                "count" => out.push_str(&self.count.to_string()),
                "most" => out.push_str(&self.most.to_string()),
                "Count" => match NUMBERS.get(usize::from(self.count)) {
                    Some(word) => out.push_str(word),
                    None => out.push_str(&self.count.to_string()),
                },
                "cost" => out.push_str(self.cost),
                "source" => out.push_str(self.source?),
                "name" => out.push_str(self.name?),
                "land" => out.push_str(self.land?),
                "Whose" => out.push_str(if self.yours { "Your" } else { "Opponent's" }),
                "from" => {
                    let zone = match self.from {
                        Some(Zone::Graveyard) => " from your graveyard",
                        Some(Zone::Hand) => " from your hand",
                        Some(Zone::Library) => " from your library",
                        Some(Zone::Exile) => " from your exile",
                        Some(Zone::Stack) => " from your stack",
                        Some(Zone::Battlefield | Zone::Nowhere) | None => "",
                    };
                    out.push_str(zone);
                }
                // The prompt's state, for whoever holds it.
                _ => out.push_str(&hole[..=close]),
            }
            rest = after;
        }
        out.push_str(rest);
        Some(out)
    }

    /// A button's label, empty if it names a card that has no name here.
    pub(crate) fn label(&self, template: &str) -> String {
        self.fill(template).unwrap_or_default()
    }

    /// The title of `wording`: its own, or the unnamed one where a card it names has no name.
    pub(crate) fn title(&self, wording: &Wording) -> String {
        self.fill(wording.title)
            .or_else(|| self.fill(wording.unnamed))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use beryl_engine::Zone;
    use beryl_engine::card::{Part, Place, Prompt, Purpose, Shape};

    use super::{Holes, Wording, wording};

    /// The holes a template may have.
    const HOLES: [&str; 11] = [
        "count", "most", "Count", "cost", "source", "name", "land", "Whose", "from", "picked", "of",
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
                ZONES.into_iter().map(move |zone| {
                    let prompt = Prompt::new(purpose)
                        .part(part)
                        .places(place, Place::None)
                        .most(2);
                    wording(&prompt, zone)
                })
            })
        })
    }

    fn templates(w: &Wording) -> [&'static str; 8] {
        [
            w.title, w.unnamed, w.confirm, w.counted, w.decline, w.yes, w.no, w.auto,
        ]
    }

    #[test]
    fn a_template_has_only_the_holes_the_table_fills() {
        for &purpose in Purpose::ALL {
            for entry in entries(purpose) {
                for template in templates(&entry) {
                    let mut rest = template;
                    while let Some(open) = rest.find('{') {
                        let close = rest[open..].find('}').expect("a hole closes") + open;
                        let hole = &rest[open + 1..close];
                        assert!(HOLES.contains(&hole), "{purpose:?}: {template:?}");
                        rest = &rest[close + 1..];
                    }
                }
            }
        }
    }

    /// A title that names a card has another for when the card has no name, and a yes always
    /// comes with a no.
    #[test]
    fn a_title_never_comes_out_empty_for_want_of_a_name() {
        for &purpose in Purpose::ALL {
            for entry in entries(purpose) {
                let named = ["{source}", "{name}", "{land}"]
                    .iter()
                    .any(|hole| entry.title.contains(hole));
                assert_eq!(named, !entry.unnamed.is_empty(), "{purpose:?}");
                assert_eq!(entry.yes.is_empty(), entry.no.is_empty(), "{purpose:?}");
                if purpose.shape() == Shape::YesNo {
                    assert!(!entry.yes.is_empty(), "{purpose:?}");
                }
            }
        }
    }

    #[test]
    fn holes_are_filled_from_the_record_and_the_view() {
        let holes = Holes {
            count: 3,
            most: 2,
            cost: "{2}{U}",
            land: Some("Its Land Face"),
            from: Some(Zone::Graveyard),
            ..Holes::default()
        };
        let fill = |template| holes.fill(template);
        assert_eq!(fill("Top {Count}").as_deref(), Some("Top Three"));
        assert_eq!(fill("Pay {cost}?").as_deref(), Some("Pay {2}{U}?"));
        assert_eq!(
            fill("Exile that card{from}?").as_deref(),
            Some("Exile that card from your graveyard?")
        );
        assert_eq!(fill("{Whose} Hand").as_deref(), Some("Opponent's Hand"));
        // The state's holes stay for whoever holds the state.
        assert_eq!(
            fill("Find {picked}/{of}").as_deref(),
            Some("Find {picked}/{of}")
        );
        // A card with no name here leaves its title to the unnamed one.
        assert_eq!(fill("Cast {name}?"), None);
        let pay_life = wording(&Prompt::new(Purpose::PayLife).count(3), None);
        assert_eq!(holes.title(&pay_life), "Its Land Face");
        let unnamed = Holes {
            land: None,
            ..holes
        };
        assert_eq!(unnamed.title(&pay_life), "Pay 3 Life?");
    }
}
