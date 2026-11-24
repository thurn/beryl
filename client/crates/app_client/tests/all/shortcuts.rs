//! Declared keyboard shortcuts: every shortcut the web interface answers is declared by a
//! registered feature, and none is matched by the client itself.

use app_client::Client;
use game_manual::features::table;
use game_manual::manual::bindings::Bindings;
use game_manual::manual::commands::{COMMANDS, Scope};
use kit_scene::{Keys, Reach};
use kit_ui::element::Chord;

/// Whether a registered feature declares the binding `binding`.
fn declares_binding(binding: &'static str) -> bool {
    (Client::shortcuts().iter()).any(|(_, shortcut)| shortcut.keys == Keys::Binding(binding))
}

/// Whether a registered feature declares the keys of the command `id`.
fn declares_command(id: &'static str) -> bool {
    (Client::shortcuts().iter()).any(|(_, shortcut)| shortcut.keys == Keys::Command(id))
}

#[test]
fn every_shortcut_of_the_web_interface_is_declared_by_a_feature() {
    // The game's keys: the bug report (`ui/src/components/bugReportKeys.ts`), undo and Escape
    // (`ui/src/stage/StageBoard.tsx`), and Enter on the start screen
    // (`ui/src/screens/StartScreen.tsx`).
    for binding in ["Cmd+B", "Cmd+Shift+B", "Cmd+Z", "Enter"] {
        assert!(declares_binding(binding), "{binding} is declared");
    }
    // The lobby's keys (`ui/src/screens/Lobby.tsx`) and the start screen's host key
    // (`ui/src/table/lobby/TableEntries.tsx`), which `ui/src/table/shortcuts.ts` resolves from
    // `data/table-commands.json`.
    for id in [
        "lobby.paste_deck",
        "lobby.change_deck",
        "lobby.unload_deck",
        "lobby.sideboard",
        "lobby.ready",
        "table.leave",
        "lobby.host",
    ] {
        assert!(declares_command(id), "{id} is declared");
    }
    // Every table command's own keys (`ui/src/table/keyboard.ts`): the table declares the
    // whole group, and its own keys send that command.
    assert!(
        (Client::shortcuts().iter()).any(|(_, shortcut)| shortcut.keys == Keys::Commands("table"))
    );
    let bindings = Bindings::new();
    for (index, command) in COMMANDS.iter().enumerate() {
        if command.scope != Scope::Table {
            continue;
        }
        for key in command.key.iter().chain(command.key_aliases) {
            let chord = Chord::parse(key.mac, true).expect("a readable binding");
            let sent = table::shortcut(&bindings, chord, true);
            assert_eq!(
                sent,
                Some(table::Message::Shortcut(
                    u16::try_from(index).unwrap(),
                    false
                )),
                "{} sends {}",
                key.mac,
                command.id
            );
        }
    }
}

#[test]
fn the_bug_report_and_undo_answer_on_every_screen() {
    let reach = |binding: &'static str| {
        (Client::shortcuts().iter())
            .find(|(_, shortcut)| shortcut.keys == Keys::Binding(binding))
            .map(|(_, shortcut)| shortcut.reach)
    };
    assert_eq!(reach("Cmd+B"), Some(Reach::App));
    assert_eq!(reach("Cmd+Z"), Some(Reach::App));
    assert_eq!(reach("Enter"), Some(Reach::Shown));
}

#[test]
fn a_declared_binding_describes_itself_for_menus_and_tooltips() {
    let (_, undo) = (Client::shortcuts().into_iter())
        .find(|(_, shortcut)| shortcut.keys == Keys::Binding("Cmd+Z"))
        .expect("undo is declared");
    assert_eq!(undo.describe(true), "Cmd+Z");
    assert_eq!(undo.describe(false), "Ctrl+Z");
}
