//! The game log's lines (`hud::log`), checked against the web interface's `logLines`
//! (`ui/src/game/log.ts`) over the views of five recordings.
//!
//! The expected lines are the web's own output, made by exporting the recordings with
//! `just b3d export <dir>` and running `logged` from `ui/src/game/log.ts` under
//! `node --experimental-strip-types` over each recording's `game-update` views in order, one view
//! at a time, printing each new entry as "<view index>\t<kind>\t<side>\t<text>". The view index
//! counts the recording's game updates from 0.

use std::fmt::Write as _;

use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use beryl_scene::hud::log::{LogKind, Logged};

/// What the web's `logged` gave, for each recording and seat.
const WEB: [(&str, Side, &str); 5] = [
    (
        "verbs-zones",
        Side::You,
        "\
3	action	you	You cast Thirst for Discovery\n\
5	action	opponent	Opponent cast Snapcaster Mage\n\
7	action	you	You cast Counterspell\n\
7	counter	opponent	Snapcaster Mage was countered\n\
7	action	you	Counterspell resolved\n\
7	action	you	You drew 3 cards\n\
9	action	you	You discarded Island\n\
9	action	you	Thirst for Discovery resolved\n\
10	action	you	You cast Damn\n\
10	action	opponent	Trinket Mage was destroyed\n\
10	action	opponent	Insect Token was destroyed\n\
10	action	you	Damn resolved\n\
11	action	you	You cast Portable Hole\n\
11	action	you	Portable Hole resolved\n\
11	action	you	Portable Hole triggered targeting Mana Crypt\n\
11	action	opponent	Mana Crypt was exiled\n\
19	turn	opponent	Turn 2: Opponent's turn\n\
23	action	opponent	Opponent cast Stock Up\n\
24	action	you	You cast Reprieve\n\
24	action	opponent	Stock Up was put into Opponent's hand\n\
24	action	you	You drew a card\n\
24	action	you	Reprieve resolved\n\
32	turn	you	Turn 3: your turn\n\
",
    ),
    (
        "verbs-zones",
        Side::Opponent,
        "\
3	action	opponent	Opponent cast Thirst for Discovery\n\
5	action	you	You cast Snapcaster Mage\n\
7	action	opponent	Opponent cast Counterspell\n\
7	counter	you	Snapcaster Mage was countered\n\
7	action	opponent	Counterspell resolved\n\
7	action	opponent	Opponent drew 3 cards\n\
9	action	opponent	Opponent discarded Island\n\
9	action	opponent	Thirst for Discovery resolved\n\
10	action	opponent	Opponent cast Damn\n\
10	action	you	Trinket Mage was destroyed\n\
10	action	opponent	Insect Token was destroyed\n\
10	action	opponent	Damn resolved\n\
11	action	opponent	Opponent cast Portable Hole\n\
11	action	opponent	Portable Hole resolved\n\
11	action	opponent	Portable Hole triggered targeting Mana Crypt\n\
11	action	you	Mana Crypt was exiled\n\
19	turn	you	Turn 2: your turn\n\
23	action	you	You cast Stock Up\n\
24	action	opponent	Opponent cast Reprieve\n\
24	action	you	Stock Up was put into your hand\n\
24	action	opponent	Opponent drew a card\n\
24	action	opponent	Reprieve resolved\n\
32	turn	opponent	Turn 3: Opponent's turn\n\
",
    ),
    (
        "pay-default",
        Side::You,
        "\
2	action	you	You cast Staff of the Storyteller\n\
",
    ),
    (
        // A face-down copy's token, whose view names it with an empty name.
        "abhorrent-oculus/fractured-identity-copies-a-face-down-permanent-as-a-22",
        Side::You,
        "\
2	turn	opponent	Turn 1: Opponent's turn\n\
3	action	you	Abhorrent Oculus triggered\n\
4	action	you	Wetland Sambar entered the battlefield\n\
4	action	you	Island was put into your graveyard\n\
8	action	opponent	Opponent cast Fractured Identity\n\
8	action	you	Wetland Sambar was exiled\n\
8	action	opponent	Opponent created a a card\n\
8	action	opponent	Fractured Identity resolved\n\
16	turn	you	Turn 2: your turn\n\
",
    ),
    (
        "verbs-combat",
        Side::You,
        "\
8	action	you	You attacked with Trinket Mage, Elite Spellbinder and Snapcaster Mage\n\
13	action	you	Trinket Mage dealt 2 damage to Beza, the Bounding Spring\n\
13	action	you	Snapcaster Mage dealt 2 damage to Opponent\n\
13	action	you	Elite Spellbinder dealt 3 damage to The Wandering Emperor\n\
13	action	opponent	Beza, the Bounding Spring dealt 4 damage to Trinket Mage\n\
14	action	you	Trinket Mage went to the graveyard\n\
14	action	opponent	The Wandering Emperor went to the graveyard\n\
18	turn	opponent	Turn 2: Opponent's turn\n\
23	action	opponent	Opponent sacrificed Treasure Token\n\
23	action	opponent	Opponent cast Oust\n\
23	action	you	Snapcaster Mage was put into your library\n\
23	action	you	You gained 3 life\n\
23	action	opponent	Oust resolved\n\
32	turn	you	Turn 3: your turn\n\
33	action	you	Mana Crypt triggered\n\
33	action	you	You won Mana Crypt's coin flip\n\
",
    ),
];

/// The log's entries as Beryl 3D makes them from the views of `name` from `seat`, one line per
/// entry, as the web's lines are written in `WEB`.
fn lines(name: &str, seat: Side) -> String {
    let views = pages::recording(name, seat)
        .and_then(|recording| recording.views())
        .unwrap_or_else(|why| panic!("{why}"));
    let mut logged = Logged::default();
    let mut out = String::new();
    for (index, update) in views.iter().enumerate() {
        let before = logged.entries().last().map(|entry| entry.key);
        logged.take(&update.view);
        for entry in logged.entries() {
            if before.is_some_and(|key| entry.key <= key) {
                continue;
            }
            let line = &entry.line;
            let kind = match line.kind {
                LogKind::Turn => "turn",
                LogKind::Counter => "counter",
                LogKind::Action => "action",
            };
            let side = match line.side {
                Side::You => "you",
                Side::Opponent => "opponent",
            };
            let _ = writeln!(out, "{index}\t{kind}\t{side}\t{}", line.text);
        }
    }
    out
}

#[test]
fn the_log_says_what_the_web_log_says_of_each_view() {
    for (name, seat, web) in WEB {
        assert_eq!(lines(name, seat), web, "{name} from {seat:?}");
    }
}
