//! The manual table's token search (docs/manual-multiplayer.md, "Tokens"): Create token opens a
//! searchable list of tokens over the table. Before anything is typed it lists the deck's own
//! tokens; typing searches the card store's tokens by name, which the session answers
//! (`SessionCommand::CardSearch`, as the web's `card_search` does), and the deck's tokens that
//! match stay first. The last row makes a custom token, which asks for its name. Return or a
//! press makes the highlighted token, and Shift makes it face down. This is the web's
//! `TokenSearch` (`ui/src/table/TokenSearch.tsx`), with the deck's tokens as its related
//! tokens.

use beryl_protocol::PlayerView;
use beryl_protocol::ipc::decks::{CardSearchHit, CardSearchRequest, CardTokenLink};
use glam::Vec2;

use super::TablePress;
use super::palette::list_row;
use crate::element::{Anchor, Element, Overflow, Press, column, text};
use crate::frame::Role;
use crate::input::Size;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{Fields, field};

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
pub fn panel(search: &TokenSearch, view: &PlayerView, fields: &Fields, window: Size) -> Element {
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
            Press::Table(TablePress::Token(at_u16)),
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
        .max_height(crate::data::space::flat::table::palette::HEIGHT)
        .children(rows);
    list.role = Role::List;
    let mut panel = column()
        .name("Token search")
        .width(crate::data::space::flat::table::palette::WIDTH)
        .gap(space::M)
        .padding(size::MENU_PADDING)
        .background(color::SURFACE)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::MENU_RADIUS)
        .child(field(FIELD, &state))
        .child(list);
    let top = crate::data::space::flat::table::palette::TOP;
    panel.anchor = Some(Anchor::Point(
        Vec2::new(window.width / 2.0, top),
        Vec2::new(0.5, 0.0),
    ));
    panel
}
