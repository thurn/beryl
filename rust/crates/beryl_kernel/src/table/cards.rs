//! The static card table: every card the program knows, by name.

use beryl_card::CardDef;

/// Every card the program knows, built once and shared by every thread as a `&'static`: the
/// cards written in files, which [`CardTable::cards`] lists, and the cards complete from the
/// card store, whose definitions the build writes with no file (`beryl_card_build`'s
/// `complete`). [`CardTable::get`] finds either. Tests, generators and sweeps walk only
/// [`CardTable::cards`], so a few hundred store cards add nothing to them; a game numbers its
/// cards from its decklists (`GameTable`), so neither list's order reaches a game.
#[derive(Debug)]
pub struct CardTable {
    cards: Listing,
    from_store: Listing,
    /// The table [`CardTable::overriding`] puts this one's cards in front of: it finds every
    /// name this table's cards do not have, and its store is this table's.
    base: Option<&'static CardTable>,
    /// Each name's first card, built on first use by [`CardTable::get`]. Never iterated: the order
    /// the engine sees is [`CardTable::cards`]'s.
    by_name: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static CardDef>>,
}

/// One of a [`CardTable`]'s two lists of cards: one slice, or the slices of several crates
/// joined the first time they are read.
///
/// Joining them into one array at compile time made `beryl_cards`, which Cargo rebuilds
/// whenever any set crate changes, spend 0.4 s evaluating 50,000 entries on every rebuild
/// after a card edit in the scale rehearsal (`just scale`). Joining them in a running program
/// takes microseconds.
#[derive(Debug)]
enum Listing {
    Whole(&'static [&'static CardDef]),
    Parts(
        &'static [&'static [&'static CardDef]],
        std::sync::OnceLock<&'static [&'static CardDef]>,
    ),
}

impl Listing {
    fn all(&self) -> &'static [&'static CardDef] {
        match self {
            Listing::Whole(cards) => cards,
            Listing::Parts([only], _) => only,
            Listing::Parts(parts, joined) => joined.get_or_init(|| {
                // Made once for a table that lives as long as the program, as a `static` does.
                Vec::leak(parts.iter().flat_map(|part| part.iter().copied()).collect())
            }),
        }
    }
}

impl CardTable {
    /// A table of the cards written in files alone.
    pub const fn new(cards: &'static [&'static CardDef]) -> Self {
        Self::with_store(cards, &[])
    }

    /// A table of the cards written in files, `cards`, and those complete from the card store,
    /// `from_store`. A card of both names is found as its file defines it.
    pub const fn with_store(
        cards: &'static [&'static CardDef],
        from_store: &'static [&'static CardDef],
    ) -> Self {
        Self::of(Listing::Whole(cards), Listing::Whole(from_store))
    }

    /// [`CardTable::with_store`] over each set crate's cards, `cards`, and each one's cards
    /// complete from the card store, `from_store`, in order: what a registry over many crates
    /// builds.
    pub const fn of_parts(
        cards: &'static [&'static [&'static CardDef]],
        from_store: &'static [&'static [&'static CardDef]],
    ) -> Self {
        Self::of(
            Listing::Parts(cards, std::sync::OnceLock::new()),
            Listing::Parts(from_store, std::sync::OnceLock::new()),
        )
    }

    const fn of(cards: Listing, from_store: Listing) -> Self {
        Self {
            cards,
            from_store,
            base: None,
            by_name: std::sync::OnceLock::new(),
        }
    }

    /// A table of `cards` in front of `base`: [`CardTable::get`] finds a name among `cards`
    /// first and in `base` after, and the store is `base`'s. A test that plays a changed
    /// definition of a card in place of the real one builds one of these for each change,
    /// without building a lookup over the whole card store each time.
    pub fn overriding(base: &'static CardTable, cards: &'static [&'static CardDef]) -> Self {
        Self {
            base: Some(base),
            ..Self::new(cards)
        }
    }

    /// The card called `name`: the first of that name among [`CardTable::cards`], as a scan
    /// would find it, else the card complete from the store of that name.
    pub fn get(&self, name: &str) -> Option<&'static CardDef> {
        if let Some(base) = self.base {
            return self
                .cards()
                .iter()
                .copied()
                .find(|card| card.name == name)
                .or_else(|| base.get(name));
        }
        self.by_name
            .get_or_init(|| {
                let (cards, from_store) = (self.cards(), self.from_store());
                let mut map =
                    std::collections::HashMap::with_capacity(cards.len() + from_store.len());
                for &card in cards.iter().chain(from_store) {
                    map.entry(card.name).or_insert(card);
                }
                map
            })
            .get(name)
            .copied()
    }

    /// The cards written in files, which tests, generators and sweeps walk.
    #[inline]
    pub fn cards(&self) -> &'static [&'static CardDef] {
        self.cards.all()
    }

    /// The cards complete from the card store, with no file: found by [`CardTable::get`] and
    /// walked by nothing that runs over every card.
    #[inline]
    pub fn from_store(&self) -> &'static [&'static CardDef] {
        match self.base {
            Some(base) => base.from_store(),
            None => self.from_store.all(),
        }
    }
}
