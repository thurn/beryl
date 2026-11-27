//! The catalog prompt and the steps it makes on the table.
//!
//! Long catalogs such as card names and creature types include a search field that filters the
//! offered entries as the player types, keeps a highlighted row under the up and down keys, and
//! lets Return pick the highlighted entry. Short catalogs such as colors or Magical Hack's land
//! types stay a plain column of entries.

mod view;

use std::convert::Infallible;

use beryl_protocol::prompt::{Click, PromptView, Purpose, ShapeBody};
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use kit_ui::widgets::FieldState;

use super::feature::{self, Command, Details, Inputs};
use super::{Shown, Steps};

/// The search field's name and placeholder, matching Arena's card-name filter (`xc8o0M3H124 8:47.0`).
pub const FIELD: &str = "Filter...";

/// The fallback entry names the engine appends when a player may name a value not in the game.
const FALLBACK_ENTRIES: [&str; 2] = ["Another card", "Another creature type"];

/// Whether `prompt` shows a search field above its catalog entries.
#[must_use]
pub fn is_searchable(prompt: &PromptView) -> bool {
    matches!(prompt.body, ShapeBody::Catalog { .. })
        && matches!(prompt.purpose, Purpose::CardName | Purpose::CreatureType)
}

/// The search and keyboard-highlight state of an open catalog prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub query: String,
    pub highlight: usize,
}

impl State {
    /// Updates the filter query, returning the highlight to the first matching entry when the
    /// query changes.
    pub fn set_query(&mut self, query: String) -> bool {
        if self.query == query {
            return false;
        }
        self.query = query;
        self.highlight = 0;
        true
    }

    /// Moves the keyboard highlight one row down (or up when `!down`), wrapping around `count`.
    pub fn move_highlight(&mut self, down: bool, count: usize) -> bool {
        if count == 0 {
            return false;
        }
        let current = self.highlight.min(count - 1);
        let next = if down {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        let changed = self.highlight != next;
        self.highlight = next;
        changed
    }

    /// Clears the filter query and highlight when a catalog prompt opens or closes.
    pub fn reset(&mut self) -> bool {
        let changed = !self.query.is_empty() || self.highlight != 0;
        self.query.clear();
        self.highlight = 0;
        changed
    }
}

/// Actions routed to the catalog prompt feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Click(Click),
    Pick { entry: u16, row: usize },
    Query(String),
    Highlight { down: bool, count: usize },
    Reset,
}

/// Projected inputs for caching the catalog prompt's view.
#[derive(PartialEq)]
pub struct ViewKey {
    base: feature::ViewKey,
    field: FieldState,
    highlight: usize,
}

/// The controls and presentation for the catalog prompt shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catalog;

impl Catalog {
    #[must_use]
    pub const fn message(click: Click) -> Message {
        Message::Click(click)
    }

    /// The marks and arrows the client draws itself for the prompt `shown`, which was `was`: none,
    /// since a catalog is about no object.
    pub(super) fn own(_shown: Option<Shown>, _was: Option<Shown>, _steps: &mut Steps) {}
}

impl Feature for Catalog {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Command;
    type Request = Infallible;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Command, Infallible>,
    ) -> bool {
        match message {
            Message::Click(click) => {
                context.command(Command::Click(*click));
                false
            }
            Message::Pick { entry, row } => {
                let changed = state.highlight != *row;
                state.highlight = *row;
                context.command(Command::Click(Click::Entry { index: *entry }));
                changed
            }
            Message::Query(query) => state.set_query(query.clone()),
            Message::Highlight { down, count } => state.move_highlight(*down, *count),
            Message::Reset => state.reset(),
        }
    }

    fn view_key(state: &State, inputs: &Inputs<'_>) -> ViewKey {
        ViewKey {
            base: feature::ViewKey::new(inputs, Details::None),
            field: inputs.hud.fields.get(FIELD),
            highlight: state.highlight,
        }
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        let prompt = inputs
            .hud
            .prompt
            .expect("only the open prompt's shape is mounted");
        let bar = inputs
            .bar
            .expect("the bar is drawn before the shape")
            .clone();
        view::view(state, prompt, &inputs.hud, bar)
    }
}

/// The indices into `entries` that `query` matches, in display order: entries where a word starts
/// with a query word first, then substring matches, keeping the session's order within each group.
///
/// Matching is case-insensitive and folds common Latin diacritics so typing `dandan` matches
/// `Dandân`. When `query` is non-empty and matches no entry, any generic fallback entry such as
/// `"Another card"` is returned so a player can still name a card not in the game.
#[must_use]
pub fn matching(query: &str, entries: &[String]) -> Vec<usize> {
    let normalized = fold_diacritics(query);
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if words.is_empty() {
        return (0..entries.len()).collect();
    }
    let mut prefixes = Vec::new();
    let mut substrings = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let label = fold_diacritics(entry);
        if !words.iter().all(|word| label.contains(word)) {
            continue;
        }
        let prefix = label
            .split(|c: char| !c.is_alphanumeric())
            .any(|label_word| words.iter().any(|word| label_word.starts_with(word)));
        if prefix {
            prefixes.push(index);
        } else {
            substrings.push(index);
        }
    }
    prefixes.extend(substrings);
    if prefixes.is_empty()
        && let Some(fallback) = entries
            .iter()
            .position(|entry| FALLBACK_ENTRIES.contains(&entry.as_str()))
    {
        prefixes.push(fallback);
    }
    prefixes
}

/// Lowercases `text` and folds common Latin diacritics to ASCII so keyboard search matches card
/// names like `Dandân` or `Lim-Dûl` without requiring accented characters.
fn fold_diacritics(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        match ch {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::matching;

    fn entries() -> Vec<String> {
        [
            "Dandân",
            "Island",
            "Memory Lapse",
            "Brainstorm",
            "Predict",
            "Another card",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn empty_query_returns_all_entries_in_order() {
        let list = entries();
        assert_eq!(matching("   ", &list), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn folds_diacritics_and_prefers_word_prefixes() {
        let list = entries();
        assert_eq!(matching("dandan", &list), vec![0]);
        assert_eq!(matching("br", &list), vec![3]);
        // "la" starts "Lapse" (prefix) and is inside "Island" (substring).
        assert_eq!(matching("la", &list), vec![2, 1]);
    }

    #[test]
    fn falls_back_to_another_card_when_nothing_matches() {
        let list = entries();
        assert_eq!(matching("Black Lotus", &list), vec![5]);
    }
}
