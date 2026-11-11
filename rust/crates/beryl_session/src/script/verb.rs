//! The closed lists a script draws on: the verbs, the marks and the arrows. Each name here is
//! also an entry of `data/ui-conventions.json`, which holds its look and values (plan K28), and
//! a test fails when the two disagree (`tests/script.rs`).
//!
//! Arena evidence is given as `<recording id> <time>`, for `just video film`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A fieldless enum whose wire name, text name and data name are one string, with the list of
/// them all: a `const fn` cannot enumerate an enum's variants.
macro_rules! names {
    ($(#[$meta:meta])* $name:ident { $($(#[$doc:meta])* $variant:ident = $text:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        pub enum $name {
            $($(#[$doc])* #[serde(rename = $text)] $variant),*
        }

        impl $name {
            /// Every one, in the order `data/ui-conventions.json` lists them.
            pub const ALL: &[$name] = &[$($name::$variant),*];

            /// Its name on the wire, in a script's text and in the data.
            pub const fn name(self) -> &'static str {
                match self {
                    $($name::$variant => $text),*
                }
            }
        }
    };
}

names! {
    /// A way of moving (plan K23): what the stage does to show one step. A verb is not a game
    /// event: a land played, a spell cast and a card drawn are all `fly`. The list is closed,
    /// and each verb's duration, delay, stagger and curve are data, measured from Arena (K27).
    Verb {
        /// A card travels from where it is to its new place, turning face up or down on the way if
        /// its face changes, and flashes as it lands on the battlefield. A land played
        /// (`UrGalw_f35w 16:14.8`, 16:46.3), a spell cast, the opponent's turning face up before it
        /// reaches the stack (`UrGalw_f35w 16:47.15`, `pE4XSNi-nZc 22:00.0`), a permanent spell
        /// resolving (`pE4XSNi-nZc 23:23.5`, `UrGalw_f35w 17:01.3`), a card drawn, shown large
        /// beside the library first (`UrGalw_f35w 16:15.8`, 16:36.6), three drawn a third of a
        /// second apart (`WQiYBv23buw 29:14.3`), a discard (`3sEoP_s4DQE 39:04.3`, 27:28.9), an
        /// aura onto its permanent (`5UHzpVvUkfI 1:01:50.3`).
        Fly = "fly",
        /// A card comes to be at its place with no flight: a token, whole and with a short glow, a
        /// batch as one group (`UrGalw_f35w 17:38.4`, `WQiYBv23buw 23:57.0`), the opening hand
        /// (`UrGalw_f35w 15:48`, `0VsYGs88NXA 54:16.6`), a card a look put in the graveyard
        /// (`iRr-GeSF0lI 21:23.9`).
        Appear = "appear",
        /// A card or ability goes from where it is with no flight, and a card is then at its new
        /// place: a destroyed permanent falls to ash (`RwKoGGUmw2k 27:36.4`,
        /// `IEXcjBn3BXk 22:25.4`), an exiled one becomes a vortex (`5UHzpVvUkfI 26:06.7`), a
        /// sacrificed one flashes (`UrGalw_f35w 19:28.5`), a returned one glows and is gone
        /// (`3sEoP_s4DQE 24:49.4`), a countered spell crumbles (`I3Te0m1bBFw 39:50.0`), a resolved
        /// instant darkens and dissolves (`WQiYBv23buw 29:15.67`, `pE4XSNi-nZc 22:01.3`), a
        /// finished ability is removed at once (`IHPV6yRaFlk 39:46.9`).
        Vanish = "vanish",
        /// An ability lifts out of its source: a small copy appears on the source, which stays as
        /// it is, and travels to the stack growing (`IEXcjBn3BXk 20:39.3`, `5UHzpVvUkfI 19:48.5`;
        /// activated, `5UHzpVvUkfI 19:58.25`, `krskVfn2v5Q 46:07.2`). Two triggers of one event
        /// leave a third of a second apart (`WQiYBv23buw 49:08.0`). A dies trigger leaves before
        /// its source breaks up (`I3Te0m1bBFw 26:56.75`).
        Emerge = "emerge",
        /// A card turns over where it lies. Not seen in Arena: built as UI Conventions' "A card
        /// turning face up" says.
        Flip = "flip",
        /// A permanent turns as it taps or untaps. Beryl's own (exception 2, full cards that tap):
        /// Arena tilts an art crop, with no motion caught (`IEXcjBn3BXk 19:35.6`,
        /// `0VsYGs88NXA 31:13.3`).
        Tap = "tap",
        /// A creature lunges at what it deals combat damage to and drifts back, the blocked before
        /// the unblocked (`IEXcjBn3BXk 22:25.4`, 25:55.6); first strike is a strike of its own
        /// (`sMpAJ0HKDdg 54:54.0`, 54:57.5).
        Strike = "strike",
        /// A streak leaves one thing and lands on another, which flashes: mana from a tapped land
        /// into the spell it pays for (`TbrYIs2juCw 33:32.45`, `pE4XSNi-nZc 22:00.6`), damage from
        /// a spell or ability to what it hits (`IEXcjBn3BXk 20:40.88`, 38:36.7,
        /// `I3Te0m1bBFw 31:37.8`).
        Streak = "streak",
        /// A number changes and shows that it did: a life total jumps, with the change floating up
        /// from the avatar, red for a loss and green for a gain (`IEXcjBn3BXk 22:25.7`,
        /// `sMpAJ0HKDdg 54:54.5`); a counter badge settles onto its permanent after a flash
        /// (`0VsYGs88NXA 31:13.25`); loyalty changes once its ability is on the stack
        /// (`5UHzpVvUkfI 19:59.25`).
        Count = "count",
        /// The table changes with nothing moving: a step of the turn begins (Arena animates
        /// nothing, `IEXcjBn3BXk 19:36.5`), a library is shuffled (nothing shows on the pile,
        /// `IEXcjBn3BXk 28:10.5`), a permanent's characteristics change.
        Set = "set",
        /// A card is held up to be read by both players: it joins the "Revealed Cards" pile at the
        /// top left, which opens into a browser (`I3Te0m1bBFw 22:08.5`, 29:42.9).
        Showcase = "showcase",
        /// Nothing moves while a spell or ability is read: the opponent's spell sits on the stack
        /// about a second before it resolves unanswered (`pE4XSNi-nZc 22:00.4`, 23:22.5).
        Dwell = "dwell",
        /// A notice passes over the table while play goes on under it: the turn banner, about a
        /// second (`IEXcjBn3BXk 19:35.7`, `0VsYGs88NXA 41:27.3`), the pill above an avatar that
        /// says what a scry did (`IHPV6yRaFlk 52:54.6`, `SN9QFe2kM4Y 14:52.7`), a coin tossed
        /// mid-table (`WCknTZNbraE 16:42.0`).
        Announce = "announce",
        /// An arrow or line between two things, from when it begins to when it ends ([`Arrow`]).
        Point = "point",
        /// A mark on a card, an ability or a player, from when it begins to when it ends
        /// ([`Mark`]).
        Mark = "mark",
        /// Cards the opponent is handling come away from their pile, face down, from when the look
        /// begins to when it ends (`IHPV6yRaFlk 52:54.6`, `qF4GF-vfc8Y 52:59.9`).
        Lift = "lift",
    }
}

names! {
    /// What a mark on a card, an ability or a player means (plan K26). The list is closed and
    /// each mark has one look, which `data/ui-conventions.json` describes: scripts and the
    /// session's prompt state both draw on it, and the stage draws nothing else on a card.
    /// What a card's own record says (tapped, summoning sick, counters, attacking) is its
    /// face, not a mark.
    Mark {
        /// The player may pick or use it now: a card to play, a permanent with an ability, a legal
        /// target, an attacker or blocker to declare, an option. Arena's cyan outline for all of
        /// them (`IEXcjBn3BXk 17:59.5`, 19:43.5, 22:20.5, `UrGalw_f35w 26:55.0`, 19:51.0).
        Eligible = "eligible",
        /// The player has picked it in the open prompt. Arena's gold outline
        /// (`SswBO8aOOgc 33:30.5`, `UrGalw_f35w 42:50.0`).
        Selected = "selected",
        /// What the offered default would use: the permanents Auto Pay would tap, the blocker
        /// the session would deal lethal damage to next. Not seen in Arena.
        Suggested = "suggested",
        /// Picked to pay a cost: it turns and carries a tap mark. Not seen in Arena.
        Paying = "paying",
        /// A target its chooser controls (exception 4).
        FriendlyTarget = "friendly-target",
        /// A target its chooser does not control (exception 4).
        HostileTarget = "hostile-target",
    }
}

names! {
    /// What an arrow or line says.
    Arrow {
        /// From a spell or ability to a target its controller controls: Arena's cyan arc, drawn
        /// until the item resolves (`I3Te0m1bBFw 31:35.05`, `bJQDi6NIrZA 55:08.5`), green in Beryl
        /// (exception 4).
        FriendlyTarget = "friendly-target",
        /// To a target its controller does not control: red in Beryl.
        HostileTarget = "hostile-target",
        /// From an ability on the stack to its source, thin and faint (`IEXcjBn3BXk 20:39.3`).
        Source = "source",
        /// From an attacker to the planeswalker or player it attacks (`SswBO8aOOgc 33:30.5`,
        /// `UrGalw_f35w 48:33.5`).
        Attack = "attack",
        /// From a blocker to the attacker it blocks, until combat damage is done
        /// (`IEXcjBn3BXk 22:21.5`, `sMpAJ0HKDdg 54:50`).
        Block = "block",
    }
}

/// Which end of an arrow, a mark or a lift a step is. The end repeats its beginning exactly:
/// what was begun and not ended is still shown, across scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Span {
    Begin,
    End,
}
