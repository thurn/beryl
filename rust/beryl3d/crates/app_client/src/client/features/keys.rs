//! The keyboard shortcuts of the registered features (docs/beryl-3d-architecture.md, "Toolkit
//! capabilities"). Each feature declares its shortcuts as data on its registry entry, and this
//! router sends a key that no focused element took to the feature that declared it. `Client`
//! matches no chord of its own: the focused element's keys (Tab, Return, the arrows and
//! Escape) belong to the flat layer, and every other key is one a feature declared.

use app_screens::screens::bug_report::{self, BugStage};
use app_screens::screens::{Screen, Sheet, start};
use game_hud::prompts::bar;
use game_manual::features::{lobby, table};
use game_manual::manual::commands::COMMANDS;
use game_manual::manual::link;
use kit_scene::input::KeyResponse;
use kit_scene::{Keys, Pressed, Reach, Shortcut};
use kit_ui::element::{Action, Chord};
use kit_ui::input::{Key, KeyPhase};

use super::{Client, REGISTRY, TESTING, TestingScreen};
use game_manual::manual::bindings::Bindings;

/// What a press of one of a feature's declared shortcuts sends: an action, the key taken with
/// nothing sent, or `None` where the shortcut does nothing now, which leaves the key to the
/// shortcuts after it.
pub(super) type OnKey = fn(&mut Client, Pressed) -> Option<KeyResponse<Action>>;

/// The response of a feature that declares no shortcut.
pub(super) fn none(_: &mut Client, _: Pressed) -> Option<KeyResponse<Action>> {
    None
}

/// The bug report's keys: Cmd+B opens it on every screen but the manual table, where Cmd+B is
/// a table command and Cmd+Shift+B opens it instead (`isBugReportShortcut` in
/// `ui/src/components/bugReportKeys.ts`), and Cmd+Return saves the description.
pub(super) const BUG_REPORT_KEYS: [Shortcut; 3] = [
    Shortcut::binding("Cmd+B", "Report bug").on_every_screen(),
    Shortcut::binding("Cmd+Shift+B", "Report bug").on_every_screen(),
    Shortcut::binding("Cmd+Return", "Save report"),
];

/// The index in `BUG_REPORT_KEYS` of the key that saves the description.
const SAVE: usize = 2;

pub(super) fn bug_report(client: &mut Client, pressed: Pressed) -> Option<KeyResponse<Action>> {
    let table = client.features.screen == Screen::Table;
    match pressed.index {
        0 | 1 if pressed.first && table == (pressed.index == 1) => {
            Some(KeyResponse::Action(Action::new(bug_report::Message::Open)))
        }
        SAVE if client.bug_open()
            && matches!(client.features.bug().stage, BugStage::Describe { .. }) =>
        {
            Some(if pressed.first {
                KeyResponse::Action(Action::new(bug_report::Message::Save(client.window.size)))
            } else {
                KeyResponse::Handled
            })
        }
        _ => None,
    }
}

/// The game's undo, the platform's undo key as the web's board takes it (`StageBoard.tsx`).
pub(super) const PROMPT_BAR: [Shortcut; 1] = [Shortcut::binding("Cmd+Z", "Undo").on_every_screen()];

pub(super) fn prompt_bar(client: &mut Client, pressed: Pressed) -> Option<KeyResponse<Action>> {
    (pressed.first && client.features.screen == Screen::Game)
        .then(|| KeyResponse::Action(Action::new(bar::Message::Undo)))
}

/// The manual table's keys: Escape clears the selection, and the keys of every table command
/// of `data/table-commands.json` (`table::shortcut`).
pub(super) const TABLE: [Shortcut; 2] = [
    Shortcut::binding("Escape", "Clear selection"),
    Shortcut::commands("table", "Table command"),
];

pub(super) fn table(client: &mut Client, pressed: Pressed) -> Option<KeyResponse<Action>> {
    if pressed.index == 0 {
        return (pressed.first && client.table_escape()).then_some(KeyResponse::Handled);
    }
    // A frozen table answers no key of its own (`manual::link`).
    let frozen = (client.show.latest()).is_some_and(|update| link::frozen(&update.view));
    if !client.screen_shown(Screen::Table) || frozen {
        return None;
    }
    let message = table::shortcut(&client.settings.bindings, pressed.chord, pressed.mac)?;
    // While a context menu or an overlay is open, its keys move through its rows. Table
    // commands take each key once even when the host sends repeats.
    if client.features.menu().is_some() || client.features.overlay_open() {
        return None;
    }
    Some(if pressed.first {
        KeyResponse::Action(Action::new(message))
    } else {
        KeyResponse::Handled
    })
}

pub(super) fn lobby(client: &mut Client, pressed: Pressed) -> Option<KeyResponse<Action>> {
    if !client.screen_shown(Screen::Lobby) {
        return None;
    }
    let Keys::Command(id) = lobby::SHORTCUTS.get(pressed.index)?.keys else {
        return None;
    };
    let message = lobby::shortcut(id, client.features.lobby_view.as_ref())?;
    Some(KeyResponse::Action(Action::new(message)))
}

pub(super) fn start(client: &mut Client, pressed: Pressed) -> Option<KeyResponse<Action>> {
    let screen = client.features.screen;
    if !client.screen_shown(screen) {
        return None;
    }
    let in_progress = start::in_progress(client.show.latest().map(|update| &update.view));
    let input = start::KeyInputs {
        screen,
        in_progress,
        session: client.session,
        has_recordings: !(client.features)
            .state_of::<TestingScreen>(TESTING)
            .recordings
            .is_empty(),
        desktop: client.show.table().profile().screen_column().is_none(),
        gallery: client.features.gallery,
    };
    start::shortcut_press(pressed.index, input).map(KeyResponse::Action)
}

impl Client {
    /// Whether the screen `screen` is shown and takes keys: it is the current screen, and
    /// neither the settings sheet nor the bug report has taken its place.
    pub(in crate::client) fn screen_shown(&self, screen: Screen) -> bool {
        self.features.screen == screen
            && self.features.sheet != Some(Sheet::Settings)
            && !self.bug_hides_screen()
    }

    /// Gives `key` to the declared shortcuts of `reach`, in the registry's order, and returns
    /// the response of the first one that `key` presses and that does something now.
    pub(in crate::client) fn declared_shortcut(
        &mut self,
        key: Key,
        reach: Reach,
    ) -> Option<KeyResponse<Action>> {
        REGISTRY.iter().find_map(|entry| {
            self.answer(key, entry.on_key, entry.shortcuts, |shortcut| {
                shortcut.reach == reach
            })
        })
    }

    /// Gives `key` to every declared shortcut of the feature `id`, as the open bug report
    /// takes its keys before anything else.
    pub(in crate::client) fn feature_shortcut(
        &mut self,
        id: &str,
        key: Key,
    ) -> Option<KeyResponse<Action>> {
        let entry = REGISTRY.iter().find(|entry| entry.id == id)?;
        self.answer(key, entry.on_key, entry.shortcuts, |_| true)
    }

    fn answer(
        &mut self,
        key: Key,
        on_key: OnKey,
        shortcuts: &[Shortcut],
        asked: impl Fn(&Shortcut) -> bool,
    ) -> Option<KeyResponse<Action>> {
        let (chord, mac) = (Chord::of(&key), self.mac());
        let first = key.phase == KeyPhase::Down;
        for (index, shortcut) in shortcuts.iter().enumerate() {
            let pressed = Pressed {
                index,
                chord,
                mac,
                first,
            };
            if asked(shortcut)
                && presses(&self.settings.bindings, shortcut.keys, pressed)
                && let Some(response) = on_key(self, pressed)
            {
                return Some(response);
            }
        }
        None
    }

    /// Every keyboard shortcut a registered feature declares, with the feature's id, in the
    /// registry's order.
    #[must_use]
    pub fn shortcuts() -> Vec<(&'static str, Shortcut)> {
        (REGISTRY.iter())
            .flat_map(|entry| entry.shortcuts.iter().map(|shortcut| (entry.id, *shortcut)))
            .collect()
    }
}

/// Whether the key of `pressed` presses `keys`: a binding's chord, or one of the keys of a
/// command in the player's `bindings`. Every key may press a group of commands, which the
/// feature looks up among its commands.
fn presses(bindings: &Bindings, keys: Keys, pressed: Pressed) -> bool {
    let chord = Some(pressed.chord);
    match keys {
        Keys::Binding(binding) => Chord::parse(binding, pressed.mac) == chord,
        Keys::Command(id) => (COMMANDS.iter())
            .filter(|command| command.id == id)
            .any(|command| {
                bindings
                    .keys(command)
                    .any(|key| Chord::parse(key, pressed.mac) == chord)
            }),
        Keys::Commands(_) => true,
    }
}
