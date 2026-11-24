//! The manual table's token search (docs/manual-multiplayer.md, "Tokens"): Create token opens a
//! searchable list of tokens over the table. Before anything is typed it lists the deck's own
//! tokens; typing searches the card store's tokens by name, which the session answers
//! (`SessionCommand::CardSearch`, as the web's `card_search` does), and the deck's tokens that
//! match stay first. The last row opens the custom token form, which asks for a name, a type
//! line, colors, a power and toughness and a note. Return or a press makes the highlighted
//! token, and Shift makes it face down. This is the web's `TokenSearch` and `CustomToken`
//! (`ui/src/table/TokenSearch.tsx`), with the deck's tokens as its related tokens.

use crate::features::tokens::Message as TokensMessage;
use beryl_protocol::PlayerView;
use beryl_protocol::ipc::decks::{CardSearchHit, CardSearchRequest, CardTokenLink};
use glam::Vec2;

use serde_json::Value;

use super::ask::parse;
use super::palette::list_row;
use kit_base::data::flat::Palette as PaletteMetrics;
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Anchor, Element, Overflow, column, row, text};
use kit_ui::input::Size;
use kit_ui::widgets::{Fields, button, field, primary};

/// The token search's field, by its name, which is also its placeholder.
pub const FIELD: &str = "Search tokens";

/// How many tokens a search asks the store for (`limit` in `TokenSearch`).
pub const LIMIT: usize = 36;

/// What the session asks the card store for a token search for `text` (`CardSearch`): its
/// tokens, as many as the web's token search asks for.
#[must_use]
pub fn request(text: String) -> CardSearchRequest {
    CardSearchRequest {
        text,
        tokens: true,
        limit: LIMIT,
    }
}

/// The open token search: the command that opened it (an index of `COMMANDS`), the last
/// answer of the store with the text it answered, the highlighted row and the query it was
/// chosen under.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenSearch {
    pub command: u16,
    /// Whether the token chosen is made face down, as Shift on the command that opened the
    /// search asks.
    pub face_down: bool,
    pub answer: Option<(String, Vec<CardSearchHit>)>,
    /// The last text the store was asked for, answered or not.
    pub asked: Option<String>,
    pub highlight: usize,
    pub query: String,
}

/// One row of the token search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A token of the deck or the store, with its group and what its card is.
    Token {
        token: CardTokenLink,
        group: &'static str,
        detail: Option<String>,
    },
    /// Make a token that is no card, named by the player.
    Custom,
}

impl Choice {
    /// What the row says.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Token { token, .. } => &token.name,
            Self::Custom => "Custom token",
        }
    }
}

impl TokenSearch {
    /// The token search opened by the command at `command` of `COMMANDS`, making its token
    /// face down if `face_down`.
    #[must_use]
    pub fn open(command: u16, face_down: bool) -> Self {
        Self {
            command,
            face_down,
            ..Self::default()
        }
    }

    /// The text the store should be searched for now, if any: `query` trimmed, unless it is
    /// empty or already asked for.
    #[must_use]
    pub fn wants(&self, query: &str) -> Option<String> {
        let text = query.trim();
        let asked = self.asked.as_deref() == Some(text);
        (!text.is_empty() && !asked).then(|| text.to_owned())
    }

    /// Whether the store's answer for `query` has not come yet.
    #[must_use]
    pub fn searching(&self, query: &str) -> bool {
        let text = query.trim();
        !text.is_empty() && self.answer.as_ref().is_none_or(|(asked, _)| asked != text)
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
    pub fn move_highlight(&mut self, down: bool, view: &PlayerView) {
        let count = self.choices(view).len();
        self.highlight = if down {
            (self.highlight + 1) % count
        } else {
            (self.highlight + count - 1) % count
        };
    }

    /// The rows for the query kept, at `view`: the deck's tokens whose name holds it, then the
    /// store's answer for it, each token once, then the custom token.
    #[must_use]
    pub fn choices(&self, view: &PlayerView) -> Vec<Choice> {
        let text = self.query.trim().to_lowercase();
        let hits: &[CardSearchHit] = match &self.answer {
            Some((asked, hits)) if asked.to_lowercase() == text => hits,
            _ => &[],
        };
        let detail = |hit: &CardSearchHit| {
            let stats = match (&hit.power, &hit.toughness) {
                (Some(power), Some(toughness)) => format!(" · {power}/{toughness}"),
                _ => String::new(),
            };
            format!("{}{stats}", hit.type_line)
        };
        let mut choices: Vec<Choice> = Vec::new();
        let mut add = |token: CardTokenLink, group, detail| {
            let seen = choices.iter().any(|choice| match choice {
                Choice::Token { token: other, .. } => other.scryfall_id == token.scryfall_id,
                Choice::Custom => false,
            });
            if !seen {
                choices.push(Choice::Token {
                    token,
                    group,
                    detail,
                });
            }
        };
        let deck = view.table.iter().flat_map(|table| &table.deck_tokens);
        for token in deck.filter(|token| token.name.to_lowercase().contains(&text)) {
            let hit = hits.iter().find(|hit| hit.scryfall_id == token.scryfall_id);
            add(token.clone(), "Related tokens", hit.map(detail));
        }
        for hit in hits {
            let token = CardTokenLink {
                name: hit.name.clone(),
                scryfall_id: hit.scryfall_id.clone(),
            };
            add(token, "Store tokens", Some(detail(hit)));
        }
        choices.push(Choice::Custom);
        choices
    }
}

/// The token search as it stands over a window of `window`'s size, where the command list
/// stands: the field, the rows under their groups' headings with the highlighted one drawn as
/// a menu's active row, and a notice when a search found nothing.
#[must_use]
pub fn panel(
    search: &TokenSearch,
    view: &PlayerView,
    fields: &Fields,
    window: Size,
    metrics: PaletteMetrics,
) -> Element {
    let mut state = fields.get(FIELD);
    FIELD.clone_into(&mut state.placeholder);
    let choices = search.choices(view);
    let mut rows: Vec<Element> = Vec::new();
    let mut group = None;
    for (at, choice) in choices.iter().enumerate() {
        let (heading, detail) = match choice {
            Choice::Token { group, detail, .. } => (*group, detail.as_deref()),
            Choice::Custom => ("", None),
        };
        if group != Some(heading) {
            group = Some(heading);
            if !heading.is_empty() {
                rows.push(
                    text(heading)
                        .style(type_style::MENU_HINT)
                        .color(color::TEXT_FAINT),
                );
            }
        }
        let at_u16 = u16::try_from(at).unwrap_or(u16::MAX);
        rows.push(list_row(
            choice.label(),
            (detail, None),
            at == search.highlight,
            TokensMessage::Choose(at_u16),
        ));
    }
    let text_typed = search.query.trim();
    let searching = search.searching(&search.query);
    if !text_typed.is_empty() && !searching && choices.len() == 1 {
        rows.insert(0, text("No matching tokens").color(color::TEXT_DIM));
    }
    let mut list = column()
        .name("Tokens")
        .gap(size::MENU_ROW_GAP)
        .overflow(Overflow::Scroll)
        .max_height(metrics.height)
        .children(rows);
    list.role = Role::List;
    let mut panel = column()
        .name("Token search")
        .holds_focus()
        .width(metrics.width)
        .gap(space::M)
        .padding(size::MENU_PADDING)
        .background(color::SURFACE)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::MENU_RADIUS)
        .child(field(FIELD, &state))
        .child(list);
    let top = metrics.top;
    panel.anchor = Some(Anchor::Point(
        Vec2::new(window.width / 2.0, top),
        Vec2::new(0.5, 0.0),
    ));
    panel
}

/// A token the player chose, as the table makes it: its card, as a token action names one
/// (`{"token": ...}` for a card of the store, `{"custom": ...}` for one the player described),
/// and its note, if any (the web's `TokenChoice` in `ui/src/table/cardState.ts`).
#[derive(Clone, Debug, PartialEq)]
pub struct TokenChoice {
    pub card: Value,
    pub note: Option<String>,
}

impl TokenChoice {
    /// The token `action` makes, if it is a token action.
    #[must_use]
    pub fn of(action: &beryl_protocol::protocol::TableAction) -> Option<Self> {
        let value = serde_json::to_value(action).ok()?;
        let token = value.get("token")?;
        Some(Self {
            card: token.get("card")?.clone(),
            note: token.get("note").and_then(Value::as_str).map(str::to_owned),
        })
    }
}

/// The colors a custom token may have, by the letter the table takes and the word the form
/// shows (`colors` in TokenSearch.tsx).
pub const COLORS: [(&str, &str); 5] = [
    ("W", "White"),
    ("U", "Blue"),
    ("B", "Black"),
    ("R", "Red"),
    ("G", "Green"),
];

/// The custom token form's fields, by their names, which are also their labels.
pub const NAME: &str = "Name";
pub const TYPES: &str = "Type line";
pub const POWER: &str = "Power";
pub const TOUGHNESS: &str = "Toughness";
pub const NOTE: &str = "Note";

/// Every field of the custom token form, in the order it shows them.
pub const CUSTOM_FIELDS: [&str; 5] = [NAME, TYPES, POWER, TOUGHNESS, NOTE];

/// The open custom token form: the token search's command (an index of `COMMANDS`), whether
/// the token is made face down, and which of `COLORS` are on. What its fields hold is kept in
/// `Fields`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomToken {
    pub command: u16,
    pub face_down: bool,
    pub colors: [bool; COLORS.len()],
}

impl CustomToken {
    /// The form for the token search's command at `command`, made face down if `face_down`,
    /// with no color on.
    #[must_use]
    pub fn open(command: u16, face_down: bool) -> Self {
        Self {
            command,
            face_down,
            colors: [false; COLORS.len()],
        }
    }

    /// The token the form describes with what `fields` hold, or `None` while it has no name or
    /// a power or toughness that is no number within the table's bounds. Power and toughness
    /// left both empty make a token with neither.
    #[must_use]
    pub fn choice(&self, fields: &Fields) -> Option<TokenChoice> {
        let text = |name: &str| fields.get(name).text.trim().to_owned();
        let name = text(NAME);
        if name.is_empty() {
            return None;
        }
        let (power, toughness) = (text(POWER), text(TOUGHNESS));
        let stat = |entry: &str| {
            parse(entry, None)
                .map(|(value, _)| value)
                .filter(|value| i32::try_from(*value).is_ok())
        };
        let stats = if power.is_empty() && toughness.is_empty() {
            Value::Null
        } else {
            serde_json::json!([stat(&power)?, stat(&toughness)?])
        };
        let colors: Vec<&str> = (COLORS.iter().zip(self.colors))
            .filter(|(_, on)| *on)
            .map(|((code, _), _)| *code)
            .collect();
        let note = text(NOTE);
        Some(TokenChoice {
            card: serde_json::json!({
                "custom": { "name": name, "types": text(TYPES), "colors": colors, "stats": stats }
            }),
            note: (!note.is_empty()).then_some(note),
        })
    }
}

/// The custom token form where the token search stands over a window of `window`'s size: its
/// heading, the name, the type line, a switch for each color, power and toughness side by
/// side, the note, and Cancel and Create token, which is enabled once the form describes a
/// token (`CustomToken::choice`).
#[must_use]
pub fn custom_panel(
    form: &CustomToken,
    fields: &Fields,
    window: Size,
    metrics: PaletteMetrics,
) -> Element {
    let input = |name: &str| {
        column()
            .gap(space::S)
            .child(
                text(name)
                    .style(type_style::MENU_HINT)
                    .color(color::TEXT_DIM),
            )
            .child(field(name, &fields.get(name)))
    };
    let colors = (COLORS.iter().zip(form.colors).enumerate()).map(|(at, ((_, word), on))| {
        let mut toggle = button(word).name(word).selected(on);
        toggle.role = Role::Toggle;
        let at = u8::try_from(at).unwrap_or(u8::MAX);
        toggle.on_press(TokensMessage::Color(at))
    });
    let stats = row()
        .name("stats")
        .gap(space::M)
        .child(input(POWER).grow(1.0))
        .child(input(TOUGHNESS).grow(1.0));
    let footer = row()
        .name("footer")
        .gap(space::M)
        .justify_content(kit_ui::element::Justify::End)
        .child(
            button("Cancel")
                .name("close")
                .on_press(TokensMessage::Close),
        )
        .child(
            primary("Create token")
                .name("confirm")
                .enabled(form.choice(fields).is_some())
                .on_press(TokensMessage::Create),
        );
    let mut panel = column()
        .name("Custom token")
        .width(metrics.width)
        .gap(space::M)
        .padding(size::MENU_PADDING)
        .background(color::SURFACE)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::MENU_RADIUS)
        .child(text("Custom token").style(type_style::MENU))
        .child(input(NAME))
        .child(input(TYPES))
        .child(
            row()
                .name("Colors")
                .gap(space::S)
                .wrap(true)
                .align_items(Align::Center)
                .children(colors),
        )
        .child(stats)
        .child(input(NOTE))
        .child(footer);
    let top = metrics.top;
    panel.anchor = Some(Anchor::Point(
        Vec2::new(window.width / 2.0, top),
        Vec2::new(0.5, 0.0),
    ));
    panel
}
