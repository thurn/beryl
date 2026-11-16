//! The manual table's context menu (docs/manual-multiplayer.md, "Context menus"): the
//! commands of a target that have a menu placement, as the web's `menuRows`
//! (`ui/src/table/menuRows.ts`) lays them out, in order of placement, in groups, with rows
//! that share a submenu folded under one row, each with its key at its end.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Side, TableAction, ZoneName};
use glam::Vec2;

use super::commands::{self, Asks, Command, Entry, Values};
use super::{Context, Target, viewers};
use crate::element::Viewed;
use crate::widgets::{MenuGroup, MenuId, MenuRow};

/// The id of the table's context menu, which its rows' presses (`Press::Menu`) carry.
pub const MENU: MenuId = MenuId(1);

/// What giving a command does: send these table actions, leave the table, open the ask prompt
/// for the command at this index of `COMMANDS` (`super::ask`), open the token search for it
/// (`super::tokens`), open the command list (`super::palette`), open it with the row that
/// resets the keys at its top (Shortcuts), open a pile's viewer (`super::viewers`), wait for a
/// click on the object or player the command at this index acts toward (`super::arrows`), or
/// nothing yet (a command the client has not built, or one that makes no action for this
/// target).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Actions(Vec<TableAction>),
    Leave,
    Ask(u16),
    Tokens(u16),
    Palette,
    Shortcuts,
    View(Viewed),
    Pick(u16),
    Nothing,
}

/// What `command` does for `target` at `view`, with what the client knows (`context`): Create
/// another token makes the last token made again, with its note (`tokenChoice` in
/// `ui/src/table/commandActions.ts`). A card target among the selected cards (`context.selected`)
/// stands for all of them (`commands::actions_for`).
#[must_use]
pub fn act(
    command: &Command,
    target: Target,
    view: &PlayerView,
    context: &Context,
    shift: bool,
) -> Act {
    match command.client {
        Some("leave_table") => return Act::Leave,
        Some("open_command_list") => return Act::Palette,
        Some("open_shortcuts") => return Act::Shortcuts,
        Some(client) => {
            return viewers::opens(client, target).map_or(Act::Nothing, Act::View);
        }
        None => {}
    }
    if command.id == "token.repeat" {
        let Some(last) = context.last_token else {
            return Act::Nothing;
        };
        let mut values = Values::new();
        values.insert("card", last.card.clone());
        values.insert("text", last.note.clone().into());
        values.insert("face_down", shift.into());
        let actions = commands::actions_with(command, target, view, &values);
        return if actions.is_empty() {
            Act::Nothing
        } else {
            Act::Actions(actions)
        };
    }
    match command.asks {
        Asks::Number | Asks::Text | Asks::TextAndNumber | Asks::Stats => {
            return Act::Ask(commands::index_of(command));
        }
        Asks::Token => return Act::Tokens(commands::index_of(command)),
        Asks::Target => {
            return if matches!(target, Target::Card(_)) {
                Act::Pick(commands::index_of(command))
            } else {
                Act::Nothing
            };
        }
        Asks::None => {}
    }
    let mut values = Values::new();
    values.insert("face_down", shift.into());
    let actions = commands::actions_for(command, target, context.selected, view, &values);
    if actions.is_empty() {
        Act::Nothing
    } else {
        Act::Actions(actions)
    }
}

/// An open context menu: where it opened, on what, its title and rows, and what each row
/// does, in the order `widgets::menu` numbers the rows (every row of every group, then the
/// rows of each submenu).
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMenu {
    pub at: Vec2,
    pub target: Target,
    pub title: String,
    pub groups: Vec<MenuGroup>,
    pub acts: Vec<Act>,
    /// On the phone's sheet, the number of the row whose submenu the sheet shows in place of
    /// the menu's rows (`widgets::sheet_menu`).
    pub page: Option<u16>,
}

/// A run of a menu's rows under one heading, or none, each with what it does.
type Section<'a> = (Option<&'a str>, Vec<(MenuRow, Act)>);

/// The groups whose name the web shows no heading for: they are separated only by a line.
const UNHEADED: [&str; 8] = [
    "Primary",
    "Card",
    "Library",
    "Life",
    "Hand",
    "Graveyard",
    "Exile",
    "",
];

/// The context menu of `target` at `view`, opened at `at`, with each row's key as the player
/// bound it, written for the platform (`context`). A row is enabled when giving its command
/// does something here (`act`).
#[must_use]
pub fn open(target: Target, view: &PlayerView, at: Vec2, context: &Context) -> OpenMenu {
    let mut entries: Vec<Entry> = commands::commands_for(target.applies_to(view), view, context)
        .into_iter()
        .filter(|entry| entry.menu.is_some())
        .collect();
    entries.sort_by_key(|entry| entry.menu.map_or(0, |menu| menu.order));
    // Each section: its heading, its rows with what each does, and its submenus' rows.
    let mut sections: Vec<Section> = Vec::new();
    let mut submenu_acts: Vec<Vec<Act>> = Vec::new();
    let mut folders: Vec<(usize, usize, &str)> = Vec::new();
    for entry in &entries {
        let Some(placement) = entry.menu else {
            continue;
        };
        let heading = (!UNHEADED.contains(&placement.group)).then_some(placement.group);
        let new_section = sections
            .last()
            .is_none_or(|(last, _)| *last != heading || entry.command.id == "table.every_action");
        if new_section {
            sections.push((heading, Vec::new()));
        }
        let section = sections.len() - 1;
        let act = act(entry.command, target, view, context, false);
        let mut row = MenuRow::new(&label(entry.command, target, view));
        row.key = context.key(entry.command);
        row.enabled = act != Act::Nothing;
        match placement.submenu {
            Some(folder) => {
                let rows = &mut sections[section].1;
                let found =
                    (folders.iter()).position(|&(s, _, name)| s == section && name == folder);
                let folder_index = found.unwrap_or_else(|| {
                    rows.push((MenuRow::new(folder), Act::Nothing));
                    submenu_acts.push(Vec::new());
                    folders.push((section, rows.len() - 1, folder));
                    folders.len() - 1
                });
                let at = folders[folder_index].1;
                rows[at].0.submenu.push(row);
                submenu_acts[folder_index].push(act);
            }
            None => sections[section].1.push((row, act)),
        }
    }
    let mut groups = Vec::new();
    let mut acts = Vec::new();
    for (_, rows) in sections {
        let mut group = MenuGroup::default();
        for (row, act) in rows {
            group.rows.push(row);
            acts.push(act);
        }
        groups.push(group);
    }
    // Then the rows of the submenus. Folders are made in the order they stand in the menu,
    // since sections and their rows are only ever added at the end.
    for acts_of_folder in submenu_acts {
        acts.extend(acts_of_folder);
    }
    OpenMenu {
        at,
        target,
        title: title(target, view),
        groups,
        acts,
        page: None,
    }
}

/// A row's label: Untap in place of Tap on a tapped permanent, as the web shows it.
fn label(command: &Command, target: Target, view: &PlayerView) -> String {
    if command.id == "permanent.tap"
        && let Target::Card(slot) = target
        && view
            .objects
            .iter()
            .any(|object| object.slot == slot && object.tapped)
    {
        return "Untap".to_owned();
    }
    command.label.to_owned()
}

/// The menu's title: the card's name, the pile's or the player's word for what it is, or
/// "Table".
#[must_use]
pub fn title(target: Target, view: &PlayerView) -> String {
    match target {
        Target::Card(slot) => (view.objects.iter())
            .find(|object| object.slot == slot)
            .and_then(|object| object.name.clone())
            .unwrap_or_else(|| "Card".to_owned()),
        Target::Pile(_, ZoneName::Graveyard) => "Graveyard".to_owned(),
        Target::Pile(_, ZoneName::Exile) => "Exile".to_owned(),
        Target::Pile(..) => "Library".to_owned(),
        Target::Hand(_) => "Hand".to_owned(),
        Target::Life(Side::You | Side::Opponent) => "Life".to_owned(),
        Target::Table => "Table".to_owned(),
    }
}
