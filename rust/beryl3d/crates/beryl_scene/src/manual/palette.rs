//! The manual table's command list (docs/manual-multiplayer.md, "The command list"): Cmd+K
//! opens a list of every command over the table, filtered as the player types. It acts on
//! what the pointer was on and says which beside the field, lists that target's commands and
//! then those of the table, the player's library, hand and life total, grouped by what they act
//! on with their keys, and keeps the highlighted row under the up and down keys. Return runs
//! the highlighted command and Shift+Return its shifted form (a token face down); a command
//! that needs a value opens the ask prompt. Option+Return waits for a key and binds it to the
//! highlighted command (`super::bindings`), and the list Shortcuts opens has a row at its top
//! that puts every command back on its own keys. This is the web's `TablePalette` and
//! `CommandList` (`ui/src/table/TablePalette.tsx`, `CommandList.tsx`) with `match`
//! (`filter.ts`).

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Side, ZoneName};
use glam::Vec2;

use super::commands;
use super::menus::{self, Act};
use super::{Context, TablePress, Target};
use crate::element::{Align, Anchor, Element, Overflow, Press, column, row, text};
use crate::frame::Role;
use crate::input::Size;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{Fields, field};

/// The command list's field, by its name, which is also its placeholder.
pub const FIELD: &str = "Search commands";

/// The targets the command list offers commands for after the one it acts on, as the web's
/// `commandListDefaults` (`ui/src/table/commands.ts`) lists them for the viewer.
const DEFAULTS: [Target; 4] = [
    Target::Table,
    Target::Pile(Side::You, ZoneName::Library),
    Target::Hand(Side::You),
    Target::Life(Side::You),
];

/// The command a row names in place of an index of `COMMANDS` when it is the row that puts
/// every command back on its own keys.
pub const RESET: u16 = u16::MAX;

/// What the list shows once the keys are reset (`setNotice` in `TablePalette`).
pub const RESET_NOTICE: &str = "Shortcuts reset.";

/// One row of the command list: the command (an index of `COMMANDS`, or `RESET`), what it acts
/// on, its label, its group, its key written for the platform and, for a row acting on
/// something other than the list's target, what that is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub command: u16,
    pub target: Target,
    pub label: &'static str,
    pub group: &'static str,
    pub key: Option<String>,
    pub detail: Option<&'static str>,
}

/// The open command list: what it acts on, every row it offers, the row the keys highlight
/// among those the query matches, the query that highlight was chosen under, whether it was
/// opened by Shortcuts and so offers to reset the keys, whether it waits for a key to bind to
/// the highlighted row, and what it says of the last binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub target: Target,
    /// What the list says it acts on, as a context menu's title says it.
    pub title: String,
    pub rows: Vec<Row>,
    pub highlight: usize,
    pub query: String,
    pub settings: bool,
    pub capturing: bool,
    pub notice: Option<String>,
}

impl Palette {
    /// The command list acting on `target` at `view`, with what the client knows (`context`),
    /// and with the row that resets the keys at its top if `settings`.
    #[must_use]
    pub fn open(target: Target, view: &PlayerView, context: &Context, settings: bool) -> Self {
        Self {
            target,
            title: menus::title(target, view),
            rows: rows(target, view, context, settings),
            highlight: 0,
            query: String::new(),
            settings,
            capturing: false,
            notice: None,
        }
    }

    /// Builds the rows again, as the keys they show have changed, keeping the highlight.
    pub fn refresh(&mut self, view: &PlayerView, context: &Context) {
        self.rows = rows(self.target, view, context, self.settings);
    }

    /// The rows `query` matches, in the order the list shows them (`matching`).
    #[must_use]
    pub fn matched(&self, query: &str) -> Vec<&Row> {
        let grouped = matching(query, &self.rows);
        // The list groups its rows under their headings, in the order each group first comes,
        // and the keys move through the rows as they are shown.
        let mut groups: Vec<&str> = Vec::new();
        for row in &grouped {
            if !groups.contains(&row.group) {
                groups.push(row.group);
            }
        }
        groups
            .iter()
            .flat_map(|group| grouped.iter().filter(move |row| row.group == *group))
            .copied()
            .collect()
    }

    /// Keeps `query` as what the field holds, starting the highlight again at the first row if
    /// it changed.
    pub fn set_query(&mut self, query: &str) {
        if self.query != query {
            query.clone_into(&mut self.query);
            self.highlight = 0;
        }
    }

    /// Moves the highlight one row down, or up if not `down`, going round at the ends.
    pub fn move_highlight(&mut self, down: bool) {
        let count = self.matched(&self.query).len();
        if count == 0 {
            return;
        }
        self.highlight = if down {
            (self.highlight + 1) % count
        } else {
            (self.highlight + count - 1) % count
        };
    }
}

/// The rows the command list offers acting on `target`: the row that resets the keys if
/// `settings`, then the target's commands, then those of each default target of another
/// kind, each command once, leaving out the list's own command and those the client cannot
/// give yet (`TableCommands.list` with `#supports`).
#[must_use]
pub fn rows(target: Target, view: &PlayerView, context: &Context, settings: bool) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    if settings {
        rows.push(Row {
            command: RESET,
            target: Target::Table,
            label: "Reset shortcuts",
            group: "Shortcuts",
            key: None,
            detail: None,
        });
    }
    let kind = target.applies_to(view);
    let others = DEFAULTS
        .into_iter()
        .filter(|other| other.applies_to(view) != kind);
    for (at, each) in std::iter::once(target).chain(others).enumerate() {
        for entry in commands::commands_for(each.applies_to(view), view, context) {
            let command = entry.command;
            let act = menus::act(command, each, view, context);
            if matches!(act, Act::Palette | Act::Nothing)
                || (rows.iter()).any(|row| row.command == entry.index)
            {
                continue;
            }
            rows.push(Row {
                command: entry.index,
                target: each,
                label: command.label,
                group: command.group,
                key: context.key(command),
                detail: (at > 0).then(|| detail(each)).flatten(),
            });
        }
    }
    rows
}

/// What a row acting on a default target says it acts on (`detail` in `TablePalette`).
fn detail(target: Target) -> Option<&'static str> {
    match target {
        Target::Life(_) => Some("You"),
        Target::Pile(_, ZoneName::Library) => Some("Your library"),
        Target::Hand(_) => Some("Your hand"),
        _ => None,
    }
}

/// The rows whose label holds every word of `query`, those where some word starts a word of
/// the label first, each kept in its order (`match` in `ui/src/table/filter.ts`).
#[must_use]
pub fn matching<'a>(query: &str, rows: &'a [Row]) -> Vec<&'a Row> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return rows.iter().collect();
    }
    let mut prefixes = Vec::new();
    let mut substrings = Vec::new();
    for row in rows {
        let label = row.label.to_lowercase();
        if !words.iter().all(|word| label.contains(word)) {
            continue;
        }
        let prefix = (label.split(|c: char| !c.is_alphanumeric()))
            .any(|label_word| words.iter().any(|word| label_word.starts_with(word)));
        if prefix {
            prefixes.push(row);
        } else {
            substrings.push(row);
        }
    }
    prefixes.extend(substrings);
    prefixes
}

/// The command list as it stands over a window of `window`'s size, centred near its top: the
/// field, with what it acts on beside it, then the matching rows under their groups' headings,
/// the highlighted one drawn as a menu's active row, and the keys that work in it.
#[must_use]
pub fn panel(palette: &Palette, fields: &Fields, window: Size) -> Element {
    let mut state = fields.get(FIELD);
    FIELD.clone_into(&mut state.placeholder);
    let query = state.text.clone();
    let header = row()
        .name("header")
        .gap(space::M)
        .align_items(Align::Center)
        .child(field(FIELD, &state).grow(1.0))
        .child(text(&palette.title).name("acting").color(color::TEXT_DIM));
    let matched = palette.matched(&query);
    let mut results: Vec<Element> = Vec::new();
    let mut group = None;
    for (at, row_view) in matched.iter().enumerate() {
        if group != Some(row_view.group) {
            group = Some(row_view.group);
            if !row_view.group.is_empty() {
                results.push(
                    text(row_view.group)
                        .style(type_style::MENU_HINT)
                        .color(color::TEXT_FAINT),
                );
            }
        }
        let at_u16 = u16::try_from(at).unwrap_or(u16::MAX);
        results.push(list_row(
            row_view.label,
            (row_view.detail, row_view.key.as_deref()),
            at == palette.highlight,
            Press::Table(TablePress::Palette(at_u16)),
        ));
    }
    if matched.is_empty() {
        results.push(text("No matching commands").color(color::TEXT_DIM));
    }
    let capture = palette.capturing.then(|| {
        crate::widgets::button("Press a key · Esc cancels")
            .name("capture")
            .on_press(Press::Table(TablePress::CancelCapture))
    });
    let notice = (palette.notice.as_deref()).map(|words| text(words).color(color::TEXT_DIM));
    let metrics = crate::data::space::flat::table::palette::WIDTH;
    let mut list = column()
        .name("Commands")
        .gap(size::MENU_ROW_GAP)
        .overflow(Overflow::Scroll)
        .max_height(crate::data::space::flat::table::palette::HEIGHT)
        .children(results);
    list.role = Role::List;
    let hints = row().name("hints").gap(space::L).children(
        ["Enter run", "Option+Enter bind a key", "Esc close"].map(|hint| {
            text(hint)
                .style(type_style::MENU_HINT)
                .color(color::TEXT_DIM)
        }),
    );
    let mut panel = column()
        .name("Command list")
        .width(metrics)
        .gap(space::M)
        .padding(size::MENU_PADDING)
        .background(color::SURFACE)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::MENU_RADIUS)
        .child(header)
        .child(list)
        .children(capture)
        .children(notice)
        .child(hints);
    let top = crate::data::space::flat::table::palette::TOP;
    panel.anchor = Some(Anchor::Point(
        Vec2::new(window.width / 2.0, top),
        Vec2::new(0.5, 0.0),
    ));
    panel
}

/// A row of the command list or the token search labelled `label`, with `detail` and `key`
/// after it, highlighted as a menu's active row if `active`, pressing `press`.
pub(super) fn list_row(
    label: &str,
    (detail, key): (Option<&str>, Option<&str>),
    active: bool,
    press: Press,
) -> Element {
    let hint = |words: &str, name: &str| {
        text(words)
            .name(name)
            .style(type_style::MENU_HINT)
            .color(color::TEXT_DIM)
    };
    let mut item = Element::new(Role::MenuItem)
        .name(label)
        .min_height(size::MENU_ROW_HEIGHT)
        .align_items(Align::Center)
        .gap(size::MENU_ROW_GAP)
        .selected(active)
        .child(text(label).style(type_style::MENU).grow(1.0))
        .children(detail.map(|words| hint(words, "detail")))
        .children(key.map(|words| hint(words, "key")));
    item.style.padding = [
        size::MENU_ROW_PADDING_Y,
        size::MENU_ROW_PADDING_END,
        size::MENU_ROW_PADDING_Y,
        size::MENU_ROW_PADDING_START,
    ];
    item.style.radius = size::MENU_ROW_RADIUS;
    if active {
        item.style.background = Some(color::MENU_ACTIVE);
    }
    item.on_press(press)
}
