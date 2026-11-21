//! Builds the manual table from its controls and the registered overlay elements.

use super::{Message, State};
use crate::manual::bindings::Bindings;
use crate::manual::commands::{COMMANDS, Scope};
use crate::manual::drop::Destination;
use crate::manual::pile_viewer::table_viewer;
use crate::manual::viewers::{PileViewer, SEARCH};
use crate::manual::{assists, drop, link, opening, pools, quit, selection, turn};
use beryl_protocol::{PlayerView, protocol::ObjectId};
use game_hud::hud::{HudIn, players, steps};
use game_hud::{hud::viewer::RevealedFace, inspect::Viewed};
use game_table::{faces::FaceList, profiles::Profile, table::Window};
use glam::Vec2;
use kit_base::data::profiles::ProfileData;
use kit_scene::SizeClass;
use kit_ui::element::{Action, Chord, Element, column};
use kit_ui::widgets::{FieldState, Fields};

pub struct Inputs<'a> {
    pub drag_threshold: f32,
    pub hud: HudIn<'a>,
    pub fields: &'a Fields,
    pub bindings: &'a Bindings,
    pub link_error: Option<&'a str>,
    pub log: Option<Element>,
    pub proposals: Option<Element>,
    pub menu: Option<Element>,
    pub overlay: Option<Element>,
    pub overlay_open: bool,
}

#[derive(PartialEq)]
pub struct ViewKey {
    drag_threshold: f32,
    window: Window,
    profile: ProfileData,
    tuning: kit_base::data::flat::Manual,
    hud_tuning: kit_base::data::hud::Hud,
    view: Option<PlayerView>,
    link_error: Option<String>,
    bindings: Bindings,
    viewing: Option<Viewed>,
    viewer: Option<(PileViewer, FieldState, Option<FaceList>, Vec<RevealedFace>)>,
    selection: Vec<ObjectId>,
    summary_over: Option<ObjectId>,
    sweep: Option<(Vec2, Vec2)>,
    landing: Option<Destination>,
    mana_sheet: Option<beryl_protocol::protocol::Side>,
    quitting: bool,
    overlay_open: bool,
    overlays: [Option<Element>; 4],
}

impl ViewKey {
    pub(super) fn new(state: &State, input: &Inputs<'_>) -> Self {
        let hud = &input.hud;
        Self {
            drag_threshold: input.drag_threshold,
            window: hud.window,
            profile: hud.profile.data().clone(),
            tuning: hud.data.flat.manual,
            hud_tuning: hud.data.hud,
            view: hud.view.cloned(),
            link_error: input.link_error.map(str::to_owned),
            bindings: input.bindings.clone(),
            viewing: hud.viewing,
            viewer: hud.viewing.map(|_| {
                (
                    state.viewer.clone(),
                    input.fields.get(SEARCH),
                    hud.faces.cloned(),
                    hud.revealed.to_vec(),
                )
            }),
            selection: state.selection.clone(),
            summary_over: state.summary_over,
            sweep: state.sweep,
            landing: state.landing,
            mana_sheet: state.mana_sheet,
            quitting: state.quitting,
            overlay_open: input.overlay_open,
            overlays: [
                input.log.clone(),
                input.proposals.clone(),
                input.menu.clone(),
                input.overlay.clone(),
            ],
        }
    }
}

/// What the flat layer shows on the table: a region with no box of its own, the log, and the
/// open context menu, command list, ask prompt, token search, custom token form or order tray,
/// if any. The table's keys are declared on its registry entry and found by `shortcut`.
#[must_use]
pub(super) fn table(state: &State, input: Inputs<'_>) -> Element {
    // The region has no box of its own, since everything in it stands at a point of the
    // window, so the pointer reaches the table's cards around what it shows. It does not pass
    // presses through, which every element in it would inherit.
    let screen = column().name("table");
    let hud = &input.hud;
    let window = hud.window;
    let profile = hud.profile;
    let metrics = hud.data.flat.manual;
    let view = hud.view;
    let frozen = view.is_some_and(link::frozen);
    let table = view.and_then(|view| view.table.as_ref());
    let labels = view
        .zip(table)
        .map(|(view, table)| assists::labels(&table.labels, view, profile))
        .unwrap_or_default();
    let sort = view.and_then(|view| assists::sort_tray(view, window.size, profile.card()));
    let sweep = (state.sweep)
        .filter(|(from, to)| from.distance(*to) >= input.drag_threshold)
        .map(|(from, to)| selection::rectangle(selection::sweep_box(from, to), metrics.sweep_rim));
    let landing = state
        .landing
        .as_ref()
        .map(|to| drop::target(to, metrics.drop));
    // The summary hides while a sweep is drawn, as the web's hides while selecting, and while
    // the command list, a prompt or the order tray is open over the table, or the table is frozen.
    let overlay_open = input.overlay_open;
    let summary = view
        .filter(|_| state.sweep.is_none() && !overlay_open && !frozen)
        .and_then(|view| {
            selection::summary(
                view,
                &state.selection,
                state.summary_over,
                metrics.selection,
            )
        });
    // The phone's sheet of a player's mana and counters, hidden while the table is frozen.
    let mana_sheet = view
        .zip(state.mana_sheet)
        .filter(|_| !frozen)
        .and_then(|(view, side)| pools::sheet(view, side, profile));
    let covered = input.menu.is_some() || overlay_open;
    let connection = view.and_then(|view| {
        let top = profile
            .slot("manual.connection")
            .map_or_else(|| link::top(window.size, metrics.link), |slot| slot.rect.y);
        link::banner(view, window.size, top, input.link_error, metrics.link)
    });
    screen
        .children(controls(hud, frozen, covered, profile))
        .children(input.log)
        .children(summary)
        .children(input.proposals)
        .children(labels)
        .children(sweep)
        .children(landing)
        .children(table_viewer(hud, input.fields, &state.viewer))
        .children(sort)
        .children(input.menu)
        .children(mana_sheet)
        .children(
            input
                .overlay
                .or_else(|| state.quitting.then(|| quit::question(window.size))),
        )
        .children(crate::screens::table_opening::table_opening(hud))
        .children(connection)
}

/// The life orbs, the step tracks, the rows of mana and player counters and the Next button,
/// none while the guided start's screen covers the table, as the game's opening covers its
/// controls. The Next button stands in the lower right, where the ask prompt docks, so it gives
/// way to whatever the table shows over itself, and a `frozen` table shows none.
fn controls(input: &HudIn, frozen: bool, covered: bool, profile: Profile<'_>) -> Vec<Element> {
    if input.view.and_then(opening::prompt).is_some() {
        return Vec::new();
    }
    let next = (input.view.and_then(|view| view.table.as_ref()))
        .filter(|_| !covered && !frozen)
        .map(|table| turn::next(table.phase, input.window, profile));
    let pools = input
        .view
        .map(|view| pools::pools(view, input.window, profile));
    [
        players::players(input),
        table_steps(input, profile.size_class()),
        pools,
        next,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// What the manual table shows for the steps: both players' step tracks, on the desktop
/// layout, while no pile's viewer is open, with the icon of the table's phase lit in the
/// active player's rows, as the web's `StepStops` stands on the manual table with no stops and
/// an `onStep`.
fn table_steps(input: &HudIn, size_class: SizeClass) -> Option<Element> {
    let view = input.view?;
    let table = view.table.as_ref()?;
    if size_class == SizeClass::Compact || input.viewing.is_some() {
        return None;
    }
    let lit = turn::icon_of(table.phase);
    Some(steps::table_tracks(
        view.turn.active,
        lit,
        input,
        |at, active| {
            Action::new(Message::Step {
                #[expect(clippy::cast_possible_truncation, reason = "twelve icons")]
                at: at as u8,
                active,
            })
        },
    ))
}

/// The message the table's key `chord` sends on the Mac if `mac`: `Message::Shortcut` with the
/// index in `COMMANDS` of the table command it is a key of, or `None`.
///
/// A command's keys are the one the player bound to it, or else its own and its aliases from
/// `data/table-commands.json` (`Bindings::keys`). Those are written as the Mac's bindings:
/// `Chord::parse` reads `Cmd` as Control and `Option` as Alt elsewhere, and the file's other
/// bindings differ from the Mac's only in writing those. With Shift held and no binding of its
/// own, a token command's key makes its token face down (`listenTableKeys` in
/// `ui/src/table/keyboard.ts`). The unshifted bindings are looked at first, so a command bound
/// to the shifted chord keeps it.
#[must_use]
pub fn shortcut(bindings: &Bindings, chord: Chord, mac: bool) -> Option<Message> {
    let table = || {
        (COMMANDS.iter().enumerate()).filter(|(_, command)| command.scope == Scope::Table)
    };
    let index = |at: usize| u16::try_from(at).ok();
    for (at, command) in table() {
        if (bindings.keys(command)).any(|key| Chord::parse(key, mac) == Some(chord)) {
            return Some(Message::Shortcut(index(at)?, false));
        }
    }
    for (at, command) in table().filter(|(_, command)| command.id.starts_with("token.")) {
        let shifted = (bindings.keys(command))
            .filter(|key| !key.split('+').any(|part| part == "Shift"))
            .any(|key| Chord::parse(&format!("Shift+{key}"), mac) == Some(chord));
        if shifted {
            return Some(Message::Shortcut(index(at)?, true));
        }
    }
    None
}
