//! The words of every prompt: one table keyed by purpose ([`wording`]), and the filling of
//! its templates ([`Holes`]).
//!
//! An entry is a prompt's title and the labels of its buttons. The match on the purpose is
//! exhaustive, so a purpose without words does not compile. Where one purpose is worded two
//! ways, the entry turns on the rest of the record: the part asked, how many it takes, the
//! place a card goes, the zone the cards lie in.
//!
//! # Arena's words
//!
//! The words are MTG Arena's, exactly, case and punctuation too (plan K22). Each entry cites
//! the moments of the recordings that show them, as `<recording id> <time>` for
//! `just video film`: two recordings where two show it. Three things hold throughout:
//!
//! - A browser has a heading and a line under it. The title is the one that asks: the line
//!   where it is a sentence of rules text ("Put two of them into your hand."), the heading
//!   where the line only says how to drag ("Scry", over "Drag cards to put them on the top or
//!   bottom of your library.").
//! - A prompt that picks is confirmed by "Submit" and the number picked, a browser that
//!   arranges by "Done", a "may" by "Take Action" or "Decline"; what a cast asks is taken back
//!   by "Cancel" ([`cancel_word`]).
//! - Arena words many prompts from the text of the card that asks ("Target a creature.",
//!   "Search for a basic land card."). The record carries no such text, so those entries say
//!   what Arena says of any card ("Search for a card.") or are composed from it.
//!
//! An entry marked *composed* is not in the recordings: it is put together from the nearest
//! one that is, and `data/ui-conventions.json` lists it under "Not seen in Arena"
//! (`ui_conventions_quotes_the_tables_composed_words` holds the two together).
//!
//! # Holes
//!
//! A template's holes are a closed set, filled from the record and the view:
//!
//! - `{count}` and `{most}`, the record's numbers, and `{most:words}`, its most as rules text
//!   writes a number ("two");
//! - `{cost}`, the record's mana cost as Oracle text writes it;
//! - `{source}`, the name of the spell or ability that asks;
//! - `{name}`, the name of the card asked about;
//! - `{from}`, where the card asked about lies when not on the battlefield (" from your
//!   graveyard"), or nothing;
//! - `{picked}` and `{of}`, how many are picked and how many the prompt takes, and `{s}`, the
//!   plural's letter unless one is picked: the prompt's state, left as written for whoever
//!   holds it.

use beryl_engine::Zone;
use beryl_engine::card::{Part, Place, Prompt, Purpose};

use crate::protocol::Step;

/// A purpose's words: templates, each empty where the prompt has no such thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wording {
    /// What the prompt asks.
    pub title: &'static str,
    /// What it asks when a hole of its title has nothing to fill it: a card with no name the
    /// viewer knows, a cost of no mana.
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
    /// The button that answers as the session suggests: the payment it worked out, every
    /// creature that can attack.
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

/// The button under a prompt that picks, which counts what is picked: "Submit 0"
/// (`0VsYGs88NXA 25:33.5`, `3sEoP_s4DQE 39:01.5`), "Submit 1" (`0VsYGs88NXA 45:18.0`,
/// `IHPV6yRaFlk 50:39.5`), "Submit 2" (`5UHzpVvUkfI 27:36.5`, `WCknTZNbraE 24:09.0`).
const SUBMIT: &str = "Submit {picked}";

/// That button with nothing picked, where picking nothing is an answer: "Target up to one
/// nonland permanent you control." (`IHPV6yRaFlk 35:30.0`), "Untap up to two lands."
/// (`gIhJjyoJ0_M 20:07.0`).
const SUBMIT_NONE: &str = "Submit 0";

/// The button under a browser that arranges: a scry (`IHPV6yRaFlk 19:58.0`), an order of
/// triggers (`UrGalw_f35w 56:30.5`), a mulligan's cards (`0VsYGs88NXA 54:21.5`).
const DONE: &str = "Done";

/// The buttons of a "may": "Exile that card from your graveyard?" (`3sEoP_s4DQE 17:44.0`,
/// `5UHzpVvUkfI 25:00.5`), "Search your library for an artifact card with mana value 1 or
/// less?" (`gIhJjyoJ0_M 47:25.0`, `SswBO8aOOgc 42:35.5`).
const TAKE_ACTION: &str = "Take Action";

/// The button that takes nothing: beside "Take Action" (as above), under a search
/// (`SswBO8aOOgc 42:37.0`, `pE4XSNi-nZc 34:00.5`), under a look that may take a card
/// (`IHPV6yRaFlk 28:19.5`, `qF4GF-vfc8Y 30:13.0`), under a card offered to cast
/// (`XYkigNjhINU 19:07.5`, `jD7VqaT8VC8 49:52.0`).
const DECLINE: &str = "Decline";

/// "Auto Pay", under a cost being paid (`0VsYGs88NXA 20:46.0`, `SswBO8aOOgc 23:48.5`).
const AUTO_PAY: &str = "Auto Pay";

/// A prompt that picks, with its title. Where picking nothing is an answer, submitting none
/// gives it, unless the purpose has a word of its own for that.
const fn picks(title: &'static str) -> Wording {
    Wording {
        title,
        confirm: SUBMIT,
        counted: SUBMIT,
        decline: SUBMIT_NONE,
        ..NONE
    }
}

/// A browser that arranges, with its title.
const fn arranges(title: &'static str) -> Wording {
    Wording {
        title,
        confirm: DONE,
        ..NONE
    }
}

/// A "may", with its question.
const fn may(title: &'static str) -> Wording {
    Wording {
        title,
        yes: TAKE_ACTION,
        no: DECLINE,
        ..NONE
    }
}

/// What a look at cards asks when it sends some of them to `to`: a question when taking none
/// is allowed. Arena has the hand's ("Put one of them into your hand.", `3sEoP_s4DQE 56:54.0`,
/// `qF4GF-vfc8Y 18:56.0`; "Put two of them into your hand.", `SN9QFe2kM4Y 27:05.0`), exile's
/// ("Exile one of them.", `3sEoP_s4DQE 1:03:15.5`, `jD7VqaT8VC8 37:36.5`) and the library's
/// ("Put one of them into your library.", `qF4GF-vfc8Y 40:12.5`), and ends a "may" with a
/// question mark ("Reveal a noncreature, nonland card?", `IHPV6yRaFlk 28:19.5`,
/// `iRr-GeSF0lI 31:13.0`; "Exile a nonland card?", `qF4GF-vfc8Y 30:13.0`). The graveyard's and
/// the battlefield's are composed, and so is each question.
const fn of_them(to: Place, asks: bool) -> &'static str {
    match (to, asks) {
        (Place::Hand, false) => "Put {most:words} of them into your hand.",
        (Place::Hand, true) => "Put {most:words} of them into your hand?",
        (Place::Exile, false) => "Exile {most:words} of them.",
        (Place::Exile, true) => "Exile {most:words} of them?",
        (Place::Top | Place::OnTop | Place::Bottom, false) => {
            "Put {most:words} of them into your library."
        }
        (Place::Top | Place::OnTop | Place::Bottom, true) => {
            "Put {most:words} of them into your library?"
        }
        (Place::Graveyard, false) => "Put {most:words} of them into your graveyard.",
        (Place::Graveyard, true) => "Put {most:words} of them into your graveyard?",
        (Place::Battlefield, false) => "Put {most:words} of them onto the battlefield.",
        (Place::Battlefield, true) => "Put {most:words} of them onto the battlefield?",
        // A browser's heading (`5UHzpVvUkfI 25:17.0`, `IHPV6yRaFlk 28:19.5`).
        (Place::None, _) => "Choose One",
    }
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
        // The turn button passes, and says where passing leads: the prompt has no title.
        // "Pass" on an opponent's turn (`0VsYGs88NXA 23:23.5`, `3sEoP_s4DQE 57:17.0`).
        Purpose::Priority => Wording {
            decline: "Pass",
            ..NONE
        },
        // Arena words a target from its card: "Target a creature." (`0VsYGs88NXA 25:33.5`,
        // `iRr-GeSF0lI 51:09.5`), "Target a player." (`0VsYGs88NXA 30:48.0`), and of a card
        // that names none, "Choose any target." (`I3Te0m1bBFw 32:12.0`,
        // `IEXcjBn3BXk 38:17.5`). The title is composed from that last. Up to one target is
        // declined by submitting none. Arena takes a variable number of targets with the
        // targets themselves ("Target any number of creatures.", `3sEoP_s4DQE 28:30.0`,
        // `TbrYIs2juCw 26:58.5`), and so does the prompt: the count the rules announce first
        // (CR 601.2c) is the number picked, never a question of its own.
        Purpose::Target => picks("Choose a target."),
        // A browser's heading, over a line from the card: "Choose One" over "Put a card
        // exiled with Currency Converter into its owner's graveyard." (`5UHzpVvUkfI 25:17.0`,
        // `3sEoP_s4DQE 27:04.5`), "Choose Two" (`I3Te0m1bBFw 29:36.5`, `SN9QFe2kM4Y 27:05.0`),
        // "Choose 3" (`XYkigNjhINU 29:38.0`, `krskVfn2v5Q 19:52.5`).
        Purpose::Choose => picks(match most {
            0 | 1 => "Choose One",
            2 => "Choose Two",
            _ => "Choose {most}",
        }),
        // "Discard a card." (`3sEoP_s4DQE 39:01.5`, `0VsYGs88NXA 45:18.0`), "Discard two
        // cards." (`5UHzpVvUkfI 27:36.5`, `SswBO8aOOgc 36:37.0`). One card in place of them
        // all is two screens in Arena, "Discard a basic land card?" with Decline
        // (`UrGalw_f35w 21:13.5`, 26:44.5) and then the cards: Beryl folds them (UI
        // Conventions, exception 3), and words the one prompt from both.
        Purpose::Discard => picks(match (part, most) {
            (Part::Instead, _) => "Discard a basic land card or {most:words} cards.",
            (_, 0 | 1) => "Discard a card.",
            _ => "Discard {most:words} cards.",
        }),
        // Composed: no discard to hand size is in the recordings. A number the game works
        // out is written in figures ("Discard 2 cards.", `WCknTZNbraE 24:09.0`; "Discard 3
        // cards.", `sMpAJ0HKDdg 36:38.5`).
        Purpose::Cleanup => picks(match most {
            0 | 1 => "Discard a card.",
            _ => "Discard {most} cards.",
        }),
        // "Return 1 Card", over "Drag 1 card to the bottom of your library."
        // (`0VsYGs88NXA 54:21.5`, `MnhyzykajXw 32:10.0`). The plural is composed.
        Purpose::PutOnBottom => Wording {
            counted: DONE,
            decline: DONE,
            ..arranges(match most {
                0 | 1 => "Return 1 Card",
                _ => "Return {most} Cards",
            })
        },
        // `krskVfn2v5Q 27:53.0`, the one time the recordings show it.
        Purpose::LegendRule => picks(
            "Choose the legendary permanent that you wish to keep. All others will go to their \
             owner's graveyard.",
        ),
        // Taking none is asked first (Part::Whether) and folded into the look, whose Decline
        // then stands beside Submit (`IHPV6yRaFlk 28:19.5`, `qF4GF-vfc8Y 30:13.0`).
        Purpose::Look => Wording {
            decline: DECLINE,
            ..picks(of_them(to, matches!(part, Part::Whether)))
        },
        // "Search for a card." (`0VsYGs88NXA 26:43.0`, `TbrYIs2juCw 33:40.5`) is Arena's for
        // a search that finds any card; it names what a narrower one finds ("Search for an
        // artifact card with mana value 1 or less.", `SswBO8aOOgc 42:37.0`,
        // `pE4XSNi-nZc 34:00.5`).
        Purpose::Search => Wording {
            decline: DECLINE,
            ..picks("Search for a card.")
        },
        // "Untap up to two lands." (`gIhJjyoJ0_M 20:07.0`, `iRr-GeSF0lI 25:17.5`), "Untap up
        // to six lands." (`yvNZVakbnrQ 21:53.5`). The purpose's one asker untaps lands.
        Purpose::Untap => picks("Untap up to {most:words} lands."),
        // Composed, as "Exile one of them." is said of cards looked at: only an opponent's
        // manifest dread is in the recordings (`pE4XSNi-nZc 28:43.5`).
        Purpose::ManifestDread => picks("Manifest one of them."),
        // From a graveyard, "Exile three other cards." (`XYkigNjhINU 29:38.0`,
        // `krskVfn2v5Q 19:52.5`). From a hand Arena names the card's color, "Exile a blue
        // card." (`TbrYIs2juCw 32:00.5`, `3sEoP_s4DQE 58:07.5`): composed without it.
        Purpose::ExileCost => picks(match (zone, most) {
            (_, 0 | 1) => "Exile a card.",
            (Some(Zone::Graveyard), _) => "Exile {most:words} other cards.",
            _ => "Exile {most:words} cards.",
        }),
        // "Assign Damage", over "Drag creatures to pre-assign damage." (`WCknTZNbraE 1:25:28.5`,
        // `krskVfn2v5Q 20:20.5`).
        Purpose::CombatDamage => Wording {
            decline: DONE,
            counted: DONE,
            ..arranges("Assign Damage")
        },
        // "Order Triggers", over "Drag triggers to choose resolution order"
        // (`UrGalw_f35w 56:30.5`, `iRr-GeSF0lI 1:01:01.0`).
        Purpose::TriggerOrder => arranges("Order Triggers"),
        // "Scry", over "Drag cards to put them on the top or bottom of your library."
        // (`IHPV6yRaFlk 19:58.0`, `SN9QFe2kM4Y 14:50.5`).
        Purpose::Scry => arranges("Scry"),
        // Two browsers in Arena, "Choose Two" over "Put two of them into your hand."
        // (`SN9QFe2kM4Y 27:05.0`) and then "Order Cards" over "Drag cards to order them on the
        // bottom of your library." (`SN9QFe2kM4Y 29:52.0`, `yvNZVakbnrQ 22:50.5`): one in
        // Beryl, worded by the part it is at.
        Purpose::Arrange => arranges(match part {
            Part::Second => "Order Cards",
            _ => of_them(to, false),
        }),
        // Composed from "Drag cards to order them on top of your library."
        // (`yvNZVakbnrQ 22:50.5`, 32:32.0): only an opponent's is in the recordings
        // (`pE4XSNi-nZc 27:18.0`).
        Purpose::PutBack => arranges(match most {
            0 | 1 => "Put a card on top.",
            _ => "Put {most:words} cards on top.",
        }),
        // Arena asks a "may" in its card's words ("Draw a card?", `XYkigNjhINU 1:17:11.0`;
        // "Create a Treasure token?", `jD7VqaT8VC8 1:09:12.5`), which the record does not
        // carry: the title is the asker's name, composed.
        Purpose::May => Wording {
            title: "{source}",
            unnamed: "Take Action?",
            ..may("")
        },
        // Composed from "Search your library for a basic land card?" (`MnhyzykajXw 28:13.5`,
        // `I3Te0m1bBFw 19:13.0`), less what the card finds.
        Purpose::MaySearch => may("Search your library?"),
        // "Exile that card from your graveyard?" (`3sEoP_s4DQE 17:44.0`,
        // `5UHzpVvUkfI 25:00.5`); the other places are composed on it.
        Purpose::Move => may(match to {
            Place::Exile => "Exile that card{from}?",
            Place::Hand => "Put that card into your hand{from}?",
            Place::Graveyard => "Put that card into your graveyard{from}?",
            Place::Battlefield => "Put that card onto the battlefield{from}?",
            Place::Top | Place::OnTop | Place::Bottom => "Put that card into your library{from}?",
            Place::None => "Move that card{from}?",
        }),
        // Composed: a fateseal is not in the recordings but as an opponent's
        // (`pE4XSNi-nZc 34:55.0`). Its answers are a scry's piles.
        Purpose::Fateseal => Wording {
            title: "Top of Library",
            yes: "BOTTOM",
            no: "TOP",
            ..NONE
        },
        // A scry's browser under the asker's name, with its piles: "Subtlety"
        // (`jD7VqaT8VC8 29:41.5`).
        Purpose::TopOrBottom => Wording {
            title: "{source}",
            unnamed: "Top or Bottom?",
            yes: "TOP",
            no: "BOTTOM",
            ..NONE
        },
        // "Explore", over "Drag the card to put it on top of your library or into your
        // graveyard.", its piles "Library" and "Graveyard" (`RwKoGGUmw2k 23:00.5`,
        // `gIhJjyoJ0_M 27:44.0`).
        Purpose::Explore => Wording {
            title: "Explore",
            yes: "Graveyard",
            no: "Library",
            ..NONE
        },
        // Arena opens the payment itself, "Pay {cost}." with Decline beside Auto Pay
        // (`0VsYGs88NXA 56:48.5`, `3sEoP_s4DQE 25:15.0`). Beryl asks first: the yes is
        // composed, as "Pay 2 Life" answers "Pay 2 life?".
        Purpose::UnlessPays => Wording {
            title: "Pay {cost}.",
            unnamed: "Pay {0}.",
            yes: "Pay {cost}",
            no: DECLINE,
            ..NONE
        },
        // "Pay 2 life?", over "Select whether to pay 2 life or to enter the battlefield
        // tapped." (`WCknTZNbraE 27:25.5`, `XYkigNjhINU 1:02:46.0`).
        Purpose::PayLife => Wording {
            title: "Pay {count} life?",
            yes: "Pay {count} Life",
            no: "Enters Tapped",
            ..NONE
        },
        // "Select a value for X.", its button "Pay" over the value (`0VsYGs88NXA 55:32.0`,
        // `3sEoP_s4DQE 25:01.0`).
        Purpose::X => Wording {
            title: "Select a value for X.",
            confirm: "Pay",
            ..NONE
        },
        // Composed: the recordings' games begin with the choice made ("You Go First",
        // `0VsYGs88NXA 16:58.0`).
        Purpose::PlayOrDraw => Wording {
            title: "Play or draw?",
            ..NONE
        },
        // Arena asks nothing: "Mulligan" and "Keep 7" stand under who goes first
        // (`0VsYGs88NXA 16:58.0`, `UrGalw_f35w 16:04.5`). The buttons are the decision's
        // candidates; the title is composed.
        Purpose::Mulligan => Wording {
            title: "Keep or mulligan?",
            ..NONE
        },
        // "Cast that card?", under "Select Card to Cast" (`jD7VqaT8VC8 49:52.0`, 50:00.5;
        // "Cast that card without paying its mana cost?", `XYkigNjhINU 19:07.5`,
        // `I3Te0m1bBFw 26:15.0`). Arena casts by a click on the card: "Cast" is composed.
        Purpose::CastOffer => Wording {
            title: "Cast that card?",
            confirm: "Cast",
            decline: DECLINE,
            ..NONE
        },
        // "Choose One", over "Click an option below to select it." (`0VsYGs88NXA 18:50.0`,
        // `3sEoP_s4DQE 17:48.0`).
        Purpose::Act => Wording {
            title: "Choose One",
            ..NONE
        },
        // "Choose Color", under "Choose One" (`UrGalw_f35w 45:18.5`). Arena takes a color at
        // a click; the other two catalogs, which no card asks yet, are composed on it.
        Purpose::Color => picks("Choose Color"),
        Purpose::CreatureType => picks("Choose Creature Type"),
        Purpose::CardName => picks("Choose Card Name"),
        // Composed from "Distribute damage among any number of target creatures."
        // (`3sEoP_s4DQE 28:32.5`, `bJQDi6NIrZA 21:42.5`), whose button is "Submit" alone.
        Purpose::DivideDamage => Wording {
            title: "Distribute damage.",
            confirm: "Submit",
            ..NONE
        },
        Purpose::DistributeCounters => Wording {
            title: "Distribute counters.",
            confirm: "Submit",
            ..NONE
        },
        // "Pay {cost}." (`SswBO8aOOgc 23:48.5`, `5UHzpVvUkfI 25:59.5`); of no mana, composed.
        Purpose::Mana => Wording {
            title: "Pay {cost}.",
            unnamed: "Pay {0}.",
            auto: AUTO_PAY,
            ..NONE
        },
        // `RwKoGGUmw2k 23:07.5`, `bJQDi6NIrZA 33:45.0`. Arena has no Auto Pay for a crew: UI
        // Conventions, "Paying with permanents".
        Purpose::Crew => Wording {
            title: "Tap any number of other creatures you control with total power {count} or \
                    more.",
            auto: AUTO_PAY,
            ..picks("")
        },
        // Composed: no waterbend is in the recordings (UI Conventions, "Paying with
        // permanents").
        Purpose::Waterbend => Wording {
            title: "Tap permanents to Waterbend",
            auto: AUTO_PAY,
            ..NONE
        },
        // "Choose attackers." with "No Attacks" and "All Attack" (`0VsYGs88NXA 27:29.0`,
        // `3sEoP_s4DQE 23:41.5`), then "1 Attacker" (`0VsYGs88NXA 27:29.5`) or "2 Attackers"
        // (`0VsYGs88NXA 37:14.0`, `IEXcjBn3BXk 38:13.0`). Arena aims an attacker with an
        // arrow and no words (`SswBO8aOOgc 33:30.5`): the aim's title is composed.
        Purpose::Attack => Wording {
            title: match part {
                Part::Aim => "Choose attack target",
                _ => "Choose attackers.",
            },
            confirm: "{picked} Attacker{s}",
            counted: "{picked} Attacker{s}",
            decline: "No Attacks",
            auto: "All Attack",
            ..NONE
        },
        // "Choose blockers." with "No Blocks" (`0VsYGs88NXA 18:12.0`, `5UHzpVvUkfI 1:01:22.0`),
        // then "1 Blocker" (`0VsYGs88NXA 18:13.0`) or "2 Blockers" (`0VsYGs88NXA 53:45.0`,
        // `IEXcjBn3BXk 24:05.0`). The aim's title is composed.
        Purpose::Block => Wording {
            title: match part {
                Part::Aim => "Choose what to block",
                _ => "Choose blockers.",
            },
            confirm: "{picked} Blocker{s}",
            counted: "{picked} Blocker{s}",
            decline: "No Blocks",
            ..NONE
        },
    }
}

/// What a target prompt asks before it takes one of the viewer's own permanents for a removal
/// (UI Conventions, exception 7): the question, and the buttons that answer and take it back.
/// Arena asks "Are You Sure?" over "Do you want to target Nadu, Winged Wisdom with this
/// ability?", answered "Yes" or "No" (`vPdoSA5H5xU 19:31.0`, the one time the recordings show
/// it). The record does not say whether a spell or an ability asks, so the question stops at
/// the name.
pub(crate) const OWN_TARGET: Wording = Wording {
    title: "Do you want to target {name}?",
    unnamed: "Are You Sure?",
    confirm: "Yes",
    decline: "No",
    ..NONE
};

/// What a target prompt among the spells on the stack says: Arena's browser of the stack,
/// "Target a spell." (`jD7VqaT8VC8 17:54.5`, `SN9QFe2kM4Y 26:54.0`), with nothing picked, and
/// the spell picked while it waits to be confirmed, which is composed.
pub(crate) const AIMED: Wording = Wording {
    title: "Target {name}",
    unnamed: "Target a spell.",
    ..NONE
};

/// Where a pass of priority leads, for the pass button's words ([`pass_words`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PassLeads {
    /// The session halts the viewer where passing is all they can do.
    Halt,
    /// The top of the stack resolves.
    Resolve,
    /// Combat damage is dealt.
    CombatDamage,
    /// On to this step of the turn.
    Step(Step),
    /// The turn ends.
    NextTurn,
}

/// What the button that pays for an X says under its label: the value set, `{n}`. Arena's
/// reads "Pay" over "X = 0" (`0VsYGs88NXA 55:32.0`).
pub(crate) const X_VALUE: &str = "X = {n}";

/// The pass button's label and the caption under it, by where passing leads and whether the
/// turn is the viewer's (`yours`).
///
/// The label is Arena's: "Next" on the viewer's own turn (`0VsYGs88NXA 17:05.0`,
/// `3sEoP_s4DQE 43:25.5`) and "Pass" on an opponent's (`0VsYGs88NXA 23:23.5`,
/// `3sEoP_s4DQE 57:17.0`), "Resolve" with something on the stack (`0VsYGs88NXA 22:57.5`,
/// `3sEoP_s4DQE 18:40.5`), "End Turn" where the viewer's turn ends (`0VsYGs88NXA 28:06.0`,
/// `IEXcjBn3BXk 35:37.5`), and "My Turn" over "End Turn" where an opponent's does
/// (`0VsYGs88NXA 25:31.5`, `3sEoP_s4DQE 22:22.0`).
///
/// The caption says where the pass leads: "To Combat" (`0VsYGs88NXA 17:05.0`,
/// `3sEoP_s4DQE 43:25.5`), "To Attackers" (`0VsYGs88NXA 23:23.5`, `IHPV6yRaFlk 45:44.0`),
/// "To Blockers" (`0VsYGs88NXA 55:14.5`, `qF4GF-vfc8Y 1:09:57.5`), "To First Strike Damage"
/// (`5UHzpVvUkfI 1:01:22.0`, `0VsYGs88NXA 1:06:59.0`), "To Damage" (`0VsYGs88NXA 27:40.0`,
/// `3sEoP_s4DQE 43:55.0`), "To End of Combat" (`3sEoP_s4DQE 23:41.5`, `qF4GF-vfc8Y 1:09:45.0`),
/// "To End" (`0VsYGs88NXA 28:06.5`, `IEXcjBn3BXk 35:37.5`), "To Draw" (`IEXcjBn3BXk 19:44.5`).
/// The captions of the untap step, the upkeep, the main phases and the cleanup step, a halt's
/// and the one under the viewer's own "End Turn" are composed.
pub(crate) const fn pass_words(leads: PassLeads, yours: bool) -> (&'static str, &'static str) {
    let pass = if yours { "Next" } else { "Pass" };
    match leads {
        PassLeads::Halt => ("End Turn", "Nothing to play"),
        PassLeads::Resolve => ("Resolve", ""),
        PassLeads::CombatDamage => (pass, "To Damage"),
        PassLeads::Step(step) => (
            pass,
            match step {
                Step::Untap => "To Untap",
                Step::Upkeep => "To Upkeep",
                Step::Draw => "To Draw",
                Step::PrecombatMain => "To Main 1",
                Step::BeginningOfCombat => "To Combat",
                Step::DeclareAttackers => "To Attackers",
                Step::DeclareBlockers => "To Blockers",
                Step::FirstStrikeDamage => "To First Strike Damage",
                Step::CombatDamage => "To Damage",
                Step::EndOfCombat => "To End of Combat",
                Step::PostcombatMain => "To Main 2",
                Step::End => "To End",
                Step::Cleanup => "To Cleanup",
            },
        ),
        PassLeads::NextTurn if yours => ("End Turn", "To Opponent's Turn"),
        PassLeads::NextTurn => ("My Turn", "End Turn"),
    }
}

/// The button that takes a prompt back: "Cancel" under what a cast asks (`0VsYGs88NXA 20:46.0`,
/// `3sEoP_s4DQE 39:01.5`) and under a browser of ways to use a card (`0VsYGs88NXA 18:50.0`),
/// "Cancel Attacks" (`0VsYGs88NXA 27:29.5`, `3sEoP_s4DQE 26:21.0`) and "Cancel Blocks"
/// (`0VsYGs88NXA 18:13.0`, `5UHzpVvUkfI 22:53.5`) once a creature is declared.
pub(crate) const fn cancel_word(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Attack => "Cancel Attacks",
        Purpose::Block => "Cancel Blocks",
        _ => "Cancel",
    }
}

/// What `place` is called over its pile of a split for `purpose`. A scry's piles are "BOTTOM"
/// and "TOP" (`IHPV6yRaFlk 19:58.0`, `jD7VqaT8VC8 29:41.5`); cards being ordered lie between
/// "Bottom" and "Top" (`yvNZVakbnrQ 22:50.5`, `SN9QFe2kM4Y 29:52.0`); a pile that is a zone
/// has the zone's name ("Graveyard" and "Library" under an explore, `RwKoGGUmw2k 23:00.5`,
/// `gIhJjyoJ0_M 27:44.0`). "Hand", "Exile" and "Battlefield" are composed on those, and so is
/// the pile of the cards that go nowhere: the hand cards are put back from.
pub(crate) const fn pile_word(purpose: Purpose, place: Place) -> &'static str {
    match (place, purpose) {
        // Cards put back come from the hand, where the rest of them stay.
        (Place::Hand, _) | (Place::None, Purpose::PutBack) => "Hand",
        (Place::None, _) => "",
        (Place::Top | Place::OnTop, Purpose::Scry) => "TOP",
        (Place::Bottom, Purpose::Scry) => "BOTTOM",
        (Place::Top | Place::OnTop, _) => "Top",
        (Place::Bottom, _) => "Bottom",
        (Place::Graveyard, _) => "Graveyard",
        (Place::Exile, _) => "Exile",
        (Place::Battlefield, _) => "Battlefield",
    }
}

/// What the two ends of an order for `purpose` are called, the last and then the first:
/// Arena writes "Last" under the left of its browser of triggers and "First" under the right
/// (`UrGalw_f35w 56:30.5`, `iRr-GeSF0lI 1:01:01.0`). None for a purpose that orders nothing.
pub(crate) const fn end_words(purpose: Purpose) -> Option<[&'static str; 2]> {
    match purpose {
        Purpose::TriggerOrder => Some(["Last", "First"]),
        _ => None,
    }
}

/// What fills a template's holes (the module comment lists them): the record's numbers and
/// cost, and the names and places the view gives.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Holes<'a> {
    pub count: u8,
    pub most: u8,
    /// The record's mana cost, if it has one.
    pub cost: Option<&'a str>,
    pub source: Option<&'a str>,
    pub name: Option<&'a str>,
    /// The zone the card asked about lies in.
    pub from: Option<Zone>,
}

/// Small numbers in words, as rules text writes them.
const NUMBERS: [&str; 11] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
];

impl Holes<'_> {
    /// `template` with its holes filled, or None if it has a hole with nothing to fill it: a
    /// card that has no name here, a cost of no mana.
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
                "most:words" => match NUMBERS.get(usize::from(self.most)) {
                    Some(word) => out.push_str(word),
                    None => out.push_str(&self.most.to_string()),
                },
                "cost" => out.push_str(self.cost?),
                "source" => out.push_str(self.source?),
                "name" => out.push_str(self.name?),
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
                // The prompt's state, for whoever holds it; and a mana symbol, which is
                // written in braces too.
                _ => out.push_str(&hole[..=close]),
            }
            rest = after;
        }
        out.push_str(rest);
        Some(out)
    }

    /// A button's label, empty if it has a hole with nothing to fill it.
    pub(crate) fn label(&self, template: &str) -> String {
        self.fill(template).unwrap_or_default()
    }

    /// The title of `wording`: its own, or the unnamed one where a hole of it has nothing to
    /// fill it.
    pub(crate) fn title(&self, wording: &Wording) -> String {
        self.fill(wording.title)
            .or_else(|| self.fill(wording.unnamed))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
