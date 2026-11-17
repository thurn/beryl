//! The manual table's context menu (docs/manual-multiplayer.md, "Context menus"): the
//! commands of a target that have a menu placement, as the web's `menuRows`
//! (`ui/src/table/menuRows.ts`) lays them out, in order of placement, in groups, with rows
//! that share a submenu folded under one row, each with its key at its end.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Answer, Side, TableAction, TableObjectRef, ZoneName};
use glam::Vec2;

use super::commands::{self, Asks, Command, Entry, Values};
use super::{Context, Target, viewers};
use crate::element::Viewed;
use crate::widgets::{MenuGroup, MenuId, MenuRow};
use beryl_protocol::script::Subject;

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
    /// Answer the table: an ability from the permanent's menu (`card_rows`).
    Answer(Answer),
    /// Ask for X, then activate the ability at this row of the permanent the reference names.
    AskX(TableObjectRef, usize),
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
    /// Whose the target is, under the title (`whose`).
    pub whose: String,
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
    let kind = target.applies_to(view);
    let mut entries: Vec<Entry> = commands::commands_for(kind, view, context)
        .into_iter()
        .chain(commands::card_entries(kind, view))
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
        let rows = card_rows(entry.command, target, view).unwrap_or_else(|| {
            let act = act(entry.command, target, view, context, false);
            let mut row = MenuRow::new(&label(entry.command, target, view));
            row.key = context.key(entry.command);
            vec![(row, act)]
        });
        for (mut row, act) in rows {
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
        whose: whose(target, view),
        groups,
        acts,
        page: None,
    }
}

/// The rows of a command whose rows come from what the card's visible face prints, as the
/// web's `availableCommands` (`ui/src/table/menuRows.ts`) makes them: Activate ability makes a
/// row for each activated ability, labelled with its cost and text (`cardAbilities` in
/// `ui/src/table/abilities.ts`), and Scry, Surveil and Look at top cards make one for each
/// different count the face's library instructions print (`cardFlows` in
/// `ui/src/table/flows.ts`), as "Scry 2". `None` for any other command, which has one row.
fn card_rows(command: &Command, target: Target, view: &PlayerView) -> Option<Vec<(MenuRow, Act)>> {
    let kind = match command.when? {
        "activated_abilities" => None,
        "card_scry" => Some("scry"),
        "card_surveil" => Some("surveil"),
        "card_look_at_top" => Some("look_at_top"),
        _ => return None,
    };
    let Target::Card(slot) = target else {
        return Some(Vec::new());
    };
    // A card the viewer cannot act on, or whose face prints nothing, has no such rows, rather
    // than the command's own row, which would do nothing.
    let Some(table) = view.table.as_ref() else {
        return Some(Vec::new());
    };
    let action =
        (table.object_actions.iter()).find(|action| action.subject == Subject::Object { slot });
    let (Some(action), Some(facts)) = (action, table.card_commands.get(&slot)) else {
        return Some(Vec::new());
    };
    let Some(kind) = kind else {
        return Some(
            (facts.abilities.iter().enumerate())
                .map(|(row, ability)| {
                    let act = if ability.requires_x {
                        Act::AskX(action.reference.clone(), row)
                    } else {
                        Act::Answer(Answer::ActivateAbility {
                            source: action.reference.clone(),
                            row,
                            x: None,
                        })
                    };
                    (MenuRow::new(&ability.label), act)
                })
                .collect(),
        );
    };
    if view.players.you.library.is_empty() {
        return Some(Vec::new());
    }
    let mut counts: Vec<u16> = Vec::new();
    // A flow's kind is compared by the name it is written with, since this crate does not
    // name the table's own type.
    let named = |flow: &&beryl_protocol::protocol::TableFlowOption| {
        serde_json::to_value(flow.kind).is_ok_and(|name| name == kind)
    };
    for flow in facts.flows.iter().filter(named) {
        if !counts.contains(&flow.count) {
            counts.push(flow.count);
        }
    }
    let seat = commands::seat_of(view, Side::You);
    let rows = counts.into_iter().filter_map(|count| {
        let json = serde_json::json!({ "flow": {
            "seat": seat,
            "flow": kind,
            "count": count,
            "source": { "id": action.reference, "in": action.zone },
        }});
        let action: TableAction = serde_json::from_value(json).ok()?;
        Some((
            MenuRow::new(&flow_label(kind, count)),
            Act::Actions(vec![action]),
        ))
    });
    Some(rows.collect())
}

/// A card flow's row label, as the web's `flowLabel` writes it.
fn flow_label(kind: &str, count: u16) -> String {
    match kind {
        "scry" => format!("Scry {count}"),
        "surveil" => format!("Surveil {count}"),
        _ => {
            let cards = if count == 1 { "card" } else { "cards" };
            format!("Look at top {count} {cards}")
        }
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

/// The line under the menu's title, as the web's `menuRows` header writes it: a library's or
/// hand's count of cards (a library's with whose it is), else whose the card (by its owner),
/// pile or life total is, or nothing for the table.
#[must_use]
pub fn whose(target: Target, view: &PlayerView) -> String {
    let word = |side: Side| match side {
        Side::You => "Yours",
        Side::Opponent => "Opponent’s",
    };
    let count = |count: usize| format!("{count} {}", if count == 1 { "card" } else { "cards" });
    match target {
        Target::Card(slot) => (view.objects.iter())
            .find(|object| object.slot == slot)
            .map(|object| word(object.owner).to_owned())
            .unwrap_or_default(),
        Target::Pile(side, ZoneName::Library) => {
            let library = match side {
                Side::You => &view.players.you.library,
                Side::Opponent => &view.players.opponent.library,
            };
            let whose = match side {
                Side::You => "yours",
                Side::Opponent => "opponent’s",
            };
            format!("{} · {whose}", count(library.len()))
        }
        Target::Hand(side) => (view.table.as_ref())
            .map(|table| match side {
                Side::You => table.orders.you.hand.len(),
                Side::Opponent => table.orders.opponent.hand.len(),
            })
            .map_or_else(|| word(side).to_owned(), count),
        Target::Pile(side, _) | Target::Life(side) => word(side).to_owned(),
        Target::Table => String::new(),
    }
}
