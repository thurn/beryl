use super::{Kind, Record, Store, Value, parse};

#[derive(Debug, Default)]
pub(super) struct NameIndex<'a> {
    cards: Vec<Name<'a>>,
    tokens: Vec<Name<'a>>,
}

#[derive(Debug)]
struct Name<'a> {
    original: &'a str,
    lowercase: String,
    value: &'a str,
}

impl<'a> NameIndex<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        let mut index = Self::default();
        let text = std::str::from_utf8(bytes).expect("the card store is UTF-8");
        for line in text.lines() {
            let (key, value) = line
                .split_once('\t')
                .expect("a card store line has a value");
            let (names, original) = if let Some(name) = key.strip_prefix("card:") {
                (&mut index.cards, name)
            } else if let Some(name) = key.strip_prefix("token:") {
                (&mut index.tokens, name)
            } else {
                continue;
            };
            names.push(Name {
                original,
                lowercase: original.to_lowercase(),
                value,
            });
        }
        for names in [&mut index.cards, &mut index.tokens] {
            // Keep the store's order among printings of the same name.
            names.sort_by(|left, right| left.lowercase.cmp(&right.lowercase));
        }
        index
    }

    fn names(&self, kind: Kind) -> &[Name<'a>] {
        match kind {
            Kind::Card => &self.cards,
            Kind::Token => &self.tokens,
        }
    }
}

impl Store<'_> {
    /// Cards whose record names contain `text`, ignoring case. Prefix matches
    /// come first, and each group is sorted by name, ignoring case. Empty text
    /// returns nothing.
    pub fn search(&self, text: &str, limit: usize) -> Vec<Record> {
        self.search_names(Kind::Card, text, limit)
    }

    /// Tokens and emblems whose names contain `text`, in the same order as
    /// [`Store::search`]. Each printing counts toward the limit.
    pub fn search_tokens(&self, text: &str, limit: usize) -> Vec<Record> {
        self.search_names(Kind::Token, text, limit)
    }

    /// The card or face with this exact name, ignoring case. If more than one
    /// spelling matches, prefer the spelling supplied.
    pub fn card_ignoring_case(&self, name: &str) -> Option<Record> {
        self.card(name).or_else(|| {
            let lowercase = name.to_lowercase();
            self.with_names(Kind::Card, |names| {
                let first = names.partition_point(|entry| entry.lowercase < lowercase);
                names
                    .get(first)
                    .filter(|entry| entry.lowercase == lowercase)
                    .and_then(|entry| self.card(entry.original))
            })
        })
    }

    fn search_names(&self, kind: Kind, text: &str, limit: usize) -> Vec<Record> {
        if text.is_empty() || limit == 0 {
            return Vec::new();
        }
        let text = text.to_lowercase();
        self.with_names(kind, |names| {
            let start = names.partition_point(|entry| entry.lowercase < text);
            let end =
                start + names[start..].partition_point(|entry| entry.lowercase.starts_with(&text));
            names[start..end]
                .iter()
                .chain(
                    names[..start]
                        .iter()
                        .chain(&names[end..])
                        .filter(|entry| entry.lowercase.contains(&text)),
                )
                .filter_map(|entry| match parse(entry.value) {
                    Value::Record(record) => Some(*record),
                    Value::Alias { .. } => None,
                })
                .take(limit)
                .collect()
        })
    }

    fn with_names<T>(&self, kind: Kind, find: impl FnOnce(&[Name<'_>]) -> T) -> T {
        match self.names {
            Some(index) => find(index.names(kind)),
            None => find(NameIndex::new(self.bytes).names(kind)),
        }
    }
}
