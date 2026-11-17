//! The static card table and the per-game table built from two decklists (architecture §6.4,
//! decision D20).

use beryl_card::{
    Ability, AbilityIx, AbilityKind, BasicLandTypes, CardDef, Condition, CostAction, CostPart,
    FACE_DOWN, FACE_DOWN_COPY, Face, FaceIx, ManaRestriction, RuleChange, Words, Zone, ZoneSet,
};

use crate::event::{Listens, Replaces};
use crate::ids::{CardId, CardIx, LargeCardIx, Seat};
use crate::layout::{Layout, PerLayout, PerSeat};
use crate::listener::{self, Listener, Unmatched};
use crate::mana_memory;
use crate::payable::{self, Unpayable};
use crate::replacement::{self, Unreplaced};
use crate::rule_changes::{self, RuleChanges, RuleId, RuleKind, Rules, Unchecked};
use crate::static_ability::{self, Inputs, Unapplied};
use crate::subtypes::{GameSubtypes, SubtypeNumbers};
use listeners::{Indexed, found_where_they_work};

mod capability;
mod cards;
mod listeners;

pub use crate::payable::{ANY_COLOR, is_instant_mana};
pub use capability::Capability;
pub use cards::CardTable;

/// A decklist: how many of each card, by name.
pub type Decklist = [(u8, &'static str)];

/// Why a [`GameTable`] could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError {
    /// A decklist names a card the [`CardTable`] does not have.
    UnknownCard(&'static str),
    /// A card of the game, or a token or emblem one makes, has a triggered ability the engine
    /// has no code to match ([`listener::listens_to`]): it would never trigger, so no game
    /// is played with it.
    Trigger {
        card: &'static str,
        unmatched: Unmatched,
    },
    /// A card of the game, or a token or emblem one makes, has an ability other than a
    /// triggered one that says it works in a zone its kind is not found in
    /// ([`listener::found_where_it_works`]): it would work only where its kind does, so no
    /// game is played with it.
    Zone { card: &'static str },
    /// A card of the game, or a token or emblem one makes, grants an ability the engine
    /// does not read when it is granted ([`listener::read_when_granted`]): anything but an
    /// activated ability or a static ability the layers apply. It would silently do nothing,
    /// so no game is played with it.
    Granted { card: &'static str },
    /// A card of the game, or a token or emblem one makes, has a static ability whose
    /// continuous effect the layers have no code to apply ([`static_ability::applied`]): it
    /// would change nothing, so no game is played with it.
    Static {
        card: &'static str,
        unapplied: Unapplied,
    },
    /// A card of the game, or a token or emblem one makes, has a replacement effect the
    /// engine has no code to apply ([`replacement`]): the event would happen as if it were
    /// not there, so no game is played with it.
    Replacement {
        card: &'static str,
        unreplaced: Unreplaced,
    },
    /// A card of the game, or a token or emblem one makes, names a rule change the rules
    /// have no check for ([`rule_changes`]): it would change nothing, so no game is played
    /// with it.
    Rule {
        card: &'static str,
        unchecked: Unchecked,
    },
    /// A filter of a card of the game, or of a token or emblem one makes, names players the
    /// engine does not read in a filter ([`crate::filter::reads`]): "that player controls",
    /// "its owner controls".
    Players { card: &'static str },
    /// A card of the game, or a token or emblem one makes, creates a token or an emblem it
    /// does not list among what it creates ([`CardDef::tokens`]): the game would have no
    /// card for it, so no game is played with it.
    Unlisted {
        card: &'static str,
        creates: &'static str,
    },
    /// A card of the game, or a token or emblem one makes, has a cost the engine has no
    /// code to pay ([`payable`]): it would be played without paying it, so no game is played
    /// with it.
    Cost {
        card: &'static str,
        unpayable: Unpayable,
    },
    /// The cards of the game, with the tokens and emblems they make, print, ask about or
    /// give more subtypes than a state numbers ([`crate::subtypes::GAME_SUBTYPES`]).
    Subtypes { count: usize },
}

/// The cards one game can contain, under dense game-local indices: what `State` stores for
/// an object's card, in the width its layout gives one ([`CardId`]). Built once per game and
/// shared, read-only, by every thread searching it, or cloned into an agent that outlives the
/// borrow.
///
/// The table is the same for every layout and holds any number of cards, so it takes a game
/// index of any width and gives one of whichever width its caller stores. Whether a layout's
/// index numbers all of them is asked where a state of that layout is made
/// ([`crate::State::deal`], [`crate::State::try_relayout`]): past there, every index of the
/// table is one of the layout's.
#[derive(Clone, Debug)]
pub struct GameTable {
    cards: &'static CardTable,
    defs: Vec<&'static CardDef>,
    /// Each deck as game indices, in decklist order.
    decks: PerSeat<Vec<usize>>,
    /// Per game index, the event kinds its triggered abilities listen to.
    listens: Vec<Listens>,
    /// Every kind any card of the game listens to.
    listens_any: Listens,
    /// Per game index, the kinds of event its static abilities replace, and
    /// [`Replaces::PLAYER_RULE_CHANGE`] if one of them changes a rule for players.
    replaces: Vec<Replaces>,
    /// Every kind any card of the game replaces, and whether any changes a rule for players.
    replaces_any: Replaces,
    /// Every triggered ability of the game's cards under the kind of event it listens to:
    /// each card's in turn, in the order its faces list them.
    listeners: Vec<Listener>,
    /// Per game index, where its listeners start in `listeners`, and after the last where
    /// they end.
    listeners_from: Vec<u32>,
    /// The triggered abilities that work in a zone of [`listener::AWAY`], as `listeners` holds
    /// those that work on the battlefield: an ability that works in both is in both.
    away_listeners: Vec<Listener>,
    /// Per game index, where its listeners start in `away_listeners`, and after the last
    /// where they end.
    away_from: Vec<u32>,
    /// Per game index, the zones of [`listener::AWAY`] in which some triggered ability of it
    /// works.
    works_away: Vec<ZoneSet>,
    /// Every zone any card of the game has a triggered ability that works in, but the
    /// battlefield.
    zones_away: ZoneSet,
    /// Every kind of event the abilities of `away_listeners` listen to, and those the triggered
    /// abilities copy exceptions give listen to ([`Capability::GivesTriggers`]): both are
    /// matched apart from the faces of the battlefield's definitions.
    listens_away: Listens,
    /// `listens_any` and `listens_away` together: whether to match an event at all, in one AND.
    listens_anywhere: Listens,
    /// Per game index, the facts of its first two faces.
    faces: Vec<[FaceFacts; 2]>,
    /// Per game index, the rule changes the static abilities of its first two faces make.
    /// Apart from the facts, which the scans of every permanent copy and which stay two
    /// words.
    face_rules: Vec<[Rules; 2]>,
    /// Per game index, the rule changes the static abilities of any of its faces make.
    rules: Vec<Rules>,
    /// The game index of [`FACE_DOWN_COPY`], in the width of each layout's
    /// ([`GameTable::face_down_copy`]); index 0, never read, in a width that does not number
    /// it.
    face_down_copy: (CardIx, LargeCardIx),
    /// Whether a card of the game has an activated ability that may do nothing
    /// ([`beryl_card::Ability::needs`]), so that no other game pays to check.
    needy: bool,
    /// A bit per [`Capability`] a card of the game has.
    capabilities: u32,
    /// The rule changes the cards of the game name, numbered.
    rule_changes: RuleChanges,
    /// The mana restrictions the cards of the game name, numbered ([`crate::mana_memory`]).
    pub(crate) restrictions: Vec<&'static ManaRestriction>,
    /// The subtypes the cards of the game name, numbered.
    subtypes: SubtypeNumbers,
    /// The game index of each name a player may choose that the game has a card of: the
    /// first index with each name among the cards that are not tokens or emblems
    /// ([`GameTable::names`]).
    names: Vec<usize>,
    /// Per game index, the printed subtypes of its first two faces by those numbers.
    face_subtypes: Vec<[GameSubtypes; 2]>,
}

/// A card's replacement mask ([`Replaces`]), with the rule changes its abilities name numbered
/// in `rule_changes`; or the first of its abilities the engine has no code for.
fn replaced(def: &CardDef, rule_changes: &mut RuleChanges) -> Result<Replaces, TableError> {
    let replaces = replacement::check(def).map_err(|unreplaced| TableError::Replacement {
        card: def.name,
        unreplaced,
    })?;
    let changes =
        rule_changes::number(def, rule_changes).map_err(|unchecked| TableError::Rule {
            card: def.name,
            unchecked,
        })?;
    Ok(replaces | changes)
}

/// The cards a game can contain, each once, and each deck as indices into them: the decks'
/// cards in decklist order, then the tokens they make, then `extra`, then [`FACE_DOWN_COPY`]
/// last.
fn gather(
    cards: &'static CardTable,
    decks: PerSeat<&Decklist>,
    extra: &[&'static CardDef],
) -> Result<(Vec<&'static CardDef>, PerSeat<Vec<usize>>), TableError> {
    let mut defs: Vec<&'static CardDef> = Vec::new();
    let mut indices: PerSeat<Vec<usize>> = Default::default();
    for (player, list) in Seat::BOTH.into_iter().zip(decks) {
        for &(count, name) in list {
            let def = cards.get(name).ok_or(TableError::UnknownCard(name))?;
            let index = if let Some(i) = defs.iter().position(|d| std::ptr::eq(*d, def)) {
                i
            } else {
                defs.push(def);
                defs.len() - 1
            };
            for _ in 0..count {
                indices[player.index()].push(index);
            }
        }
    }
    let mut i = 0;
    while i < defs.len() {
        for &token in defs[i].tokens {
            if !defs.iter().any(|d| std::ptr::eq(*d, token)) {
                defs.push(token);
            }
        }
        i += 1;
    }
    for &def in extra {
        if !defs.iter().any(|d| std::ptr::eq(*d, def)) {
            defs.push(def);
        }
    }
    defs.push(&FACE_DOWN_COPY);
    Ok((defs, indices))
}

/// The game index of each distinct name among `defs` that a card has ([`GameTable::names`]):
/// not a token's or an emblem's, which some card of `defs` lists among what it creates, and
/// not [`FACE_DOWN_COPY`]'s, which is no card's.
fn card_names(defs: &[&'static CardDef]) -> Vec<usize> {
    let made = |def: &CardDef| {
        defs.iter()
            .any(|d| d.tokens.iter().any(|&token| std::ptr::eq(token, def)))
    };
    let mut names: Vec<usize> = Vec::new();
    for (i, &def) in defs.iter().enumerate() {
        let card = !std::ptr::eq(def, &raw const FACE_DOWN_COPY) && !made(def);
        if card && !names.iter().any(|&n| defs[n].name == def.name) {
            names.push(i);
        }
    }
    names
}

/// Whether every token and emblem `def`'s effects create is one of `defs`, the cards of a
/// game: those its decks name and those they list among what they create.
fn listed(def: &CardDef, defs: &[&'static CardDef]) -> Result<(), TableError> {
    let mut unlisted = None;
    def.creates(&mut |made| {
        if !defs.iter().any(|d| std::ptr::eq(*d, made)) {
            unlisted.get_or_insert(made.name);
        }
    });
    match unlisted {
        Some(creates) => Err(TableError::Unlisted {
            card: def.name,
            creates,
        }),
        None => Ok(()),
    }
}

impl GameTable {
    /// Assigns game indices in order of first appearance, the first deck before the second,
    /// then the tokens and emblems those cards can create, then [`FACE_DOWN_COPY`], so the
    /// same decklists always give the same table.
    ///
    /// Decks of any size and any number of distinct cards make a table: whether a game
    /// between them fits a layout is asked where its state is made ([`crate::State::deal`]).
    pub fn new(cards: &'static CardTable, decks: PerSeat<&Decklist>) -> Result<Self, TableError> {
        Self::with_extra(cards, decks, &[])
    }

    /// [`GameTable::new`], with `extra` given game indices after the tokens and before
    /// [`FACE_DOWN_COPY`]: cards no deck holds that a tool puts into a game through the board
    /// door ([`crate::board::Board::set_card`]), as the present-cost benchmark puts a stand-in
    /// for each card it takes out of a position. A card already in the table keeps its index.
    pub fn with_extra(
        cards: &'static CardTable,
        decks: PerSeat<&Decklist>,
        extra: &[&'static CardDef],
    ) -> Result<Self, TableError> {
        let (defs, indices) = gather(cards, decks, extra)?;
        let last = defs.len() - 1;
        let face_down_copy = (
            CardIx::try_from_index(last).unwrap_or_default(),
            LargeCardIx::try_from_index(last).unwrap_or_default(),
        );
        for def in &defs {
            listed(def, &defs)?;
            payable::check(def).map_err(|unpayable| TableError::Cost {
                card: def.name,
                unpayable,
            })?;
        }
        let mut here = Indexed::default();
        let mut away = Indexed::default();
        let mut replaces = Vec::with_capacity(defs.len());
        let mut rule_changes = RuleChanges::default();
        let mut given = Listens::NONE;
        for def in &defs {
            let of = listener::of(def).map_err(|unmatched| TableError::Trigger {
                card: def.name,
                unmatched,
            })?;
            given |= listener::given(def).map_err(|unmatched| TableError::Trigger {
                card: def.name,
                unmatched,
            })?;
            found_where_they_work(def)?;
            static_ability::check(def).map_err(|unapplied| TableError::Static {
                card: def.name,
                unapplied,
            })?;
            if !crate::filter::reads_players_of(def) {
                return Err(TableError::Players { card: def.name });
            }
            replaces.push(replaced(def, &mut rule_changes)?);
            here.push(of.iter().filter(|l| l.zones.contains(Zone::Battlefield)));
            away.push(
                of.iter()
                    .filter(|l| !l.zones.without(ZoneSet::BATTLEFIELD).is_empty()),
            );
        }
        let (here, away) = (here.finish(), away.finish());
        let zones_away = away.zones.iter().fold(ZoneSet::NONE, |a, &b| a.union(b));
        let listens_away = away.kinds.iter().fold(given, |a, &b| a | b);
        let listens_any = here.kinds.iter().fold(Listens::NONE, |a, &b| a | b);
        let listens_anywhere = listens_any | listens_away;
        let replaces_any = replaces.iter().fold(Replaces::NONE, |a, &b| a | b);
        let needy = defs.iter().any(|d| {
            d.faces
                .iter()
                .flat_map(|f| f.abilities)
                .any(|a| a.needs().is_some())
        });
        let faces = defs
            .iter()
            .map(|d| {
                let of = |i: usize| d.faces.get(i).map_or(FaceFacts::NONE, FaceFacts::of);
                [of(0), of(1)]
            })
            .collect();
        let (face_rules, rules) = rule_changes.of_cards(&defs);
        let subtypes =
            SubtypeNumbers::new(&defs).map_err(|count| TableError::Subtypes { count })?;
        let face_subtypes = subtypes.of_cards(&defs);
        let names = card_names(&defs);
        let restrictions = mana_memory::number(&defs);
        let capabilities = defs
            .iter()
            .flat_map(|d| d.faces)
            .fold(Capability::of(&FACE_DOWN), |c, f| c | Capability::of(f));
        let listens_away = capability::merged_listen_away(capabilities, listens_away, listens_any);
        Ok(Self {
            cards,
            defs,
            decks: indices,
            listens: here.kinds,
            listens_any,
            replaces,
            replaces_any,
            listeners: here.listeners,
            listeners_from: here.from,
            away_listeners: away.listeners,
            away_from: away.from,
            works_away: away.zones,
            zones_away,
            listens_away,
            listens_anywhere,
            faces,
            face_rules,
            rules,
            face_down_copy,
            needy,
            capabilities,
            rule_changes,
            restrictions,
            subtypes,
            face_subtypes,
            names,
        })
    }

    /// The numbers of the subtypes the cards of the game name, which a permanent's
    /// [`GameSubtypes`] is read by.
    #[inline]
    pub fn subtypes(&self) -> &SubtypeNumbers {
        &self.subtypes
    }

    /// The game index of each card name of the game a player may choose, in game order
    /// ([CR 201.4](https://yawgatog.com/resources/magic-rules/#R2014)): "the player must
    /// choose the name of a card in the Oracle card reference". A token's name is among them
    /// only where a card has it: tokens and emblems are not cards in that reference. Every
    /// other name has the same consequences in this game, as no card of it has that name, so
    /// one choice ("another card") stands for them all.
    #[inline]
    pub fn names(&self) -> &[usize] {
        &self.names
    }

    /// The printed subtypes of the face `face` of the card at `card` by the game's numbers: found
    /// once per game for its first two faces, and from the face itself for any other. A face-down
    /// permanent has none ([CR 708.2a]): it "becomes a 2/2 face-down creature with no text, no
    /// name, no subtypes, and no mana cost."
    ///
    /// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
    #[inline]
    pub fn face_subtypes(&self, card: impl CardId, face: FaceIx) -> GameSubtypes {
        match self.face_subtypes[card.index()].get(usize::from(face.get())) {
            Some(&subtypes) => subtypes,
            None if face == FaceIx::DOWN => GameSubtypes::NONE,
            None => self
                .subtypes
                .of(self.def(card).face(face).subtypes.read(&Words::NONE)),
        }
    }

    /// The game index of [`FACE_DOWN_COPY`], the card a copy of a face-down permanent copies,
    /// as a state of layout `L` stores it.
    ///
    /// It is kept in each layout's width and read with one load, not converted: it is asked
    /// on the rare path of [`crate::Copiable::record`], which is inlined into the layers'
    /// scan of every permanent, where a conversion and its check cost the rollout benchmark
    /// 0.1% of its instructions.
    #[inline]
    pub fn face_down_copy<L: Layout>(&self) -> L::Card {
        L::of::<GameIndex>(self.face_down_copy.0, self.face_down_copy.1)
    }

    /// The game index of a card or token, if the game can contain it.
    pub fn index_of<C: CardId>(&self, def: &CardDef) -> Option<C> {
        self.defs
            .iter()
            .position(|d| std::ptr::eq(*d, def))
            .map(C::from_index)
    }

    /// The game index of the card or token called `name`, if the game can contain it.
    pub fn index_named<C: CardId>(&self, name: &str) -> Option<C> {
        self.defs
            .iter()
            .position(|d| d.name == name)
            .map(C::from_index)
    }

    /// The kinds of event the static abilities of the card at `card` replace. The crate's own:
    /// others ask [`crate::Copiable::may_replace`], which is nothing for a face-down permanent.
    #[inline]
    pub(crate) fn replaces(&self, card: impl CardId) -> Replaces {
        self.replaces[card.index()]
    }

    /// The facts of the face `face` of the card at `card`: found once per game for its first
    /// two faces, and read from the face itself for any other, or face down.
    #[inline]
    pub fn facts(&self, card: impl CardId, face: FaceIx) -> FaceFacts {
        match self.faces[card.index()].get(usize::from(face.get())) {
            Some(&facts) => facts,
            None => FaceFacts::of(self.def(card).face(face)),
        }
    }

    /// The rule changes the static abilities of any face of the card at `card` make: more
    /// than the face that is up may, so a set to skip a card by and nothing else. The
    /// crate's own: others ask [`crate::Copiable::rules`] and
    /// [`crate::Copiable::player_rules`].
    #[inline]
    pub(crate) fn may_have_rules(&self, card: impl CardId) -> Rules {
        self.rules[card.index()]
    }

    /// The rule changes the static abilities of `face` of the card at `card` make: found
    /// once per game for its first two faces, and read from the face itself for any other,
    /// or face down.
    #[inline]
    pub(crate) fn face_rules(&self, card: impl CardId, face: FaceIx) -> Rules {
        match self.face_rules[card.index()].get(usize::from(face.get())) {
            Some(&rules) => rules,
            None => self.rule_changes.of_face(self.def(card).face(face)),
        }
    }

    /// The rule change the game numbered `id`.
    #[inline]
    pub fn rule(&self, id: RuleId) -> &'static RuleChange {
        self.rule_changes.get(id)
    }

    /// The condition of the static ability that makes the rule change `id` for its own
    /// permanent, if it has one: its "as long as".
    #[inline]
    pub fn rule_condition(&self, id: RuleId) -> Option<&'static Condition> {
        self.rule_changes.condition(id)
    }

    /// The number of `rule` in the game, if a card of it names that rule change
    /// ([`beryl_card::Ability::rule`]).
    #[inline]
    pub fn rule_id(&self, rule: &'static RuleChange) -> Option<RuleId> {
        self.rule_changes.id(rule)
    }

    /// The rule changes of `kind` the game's cards can make: what a check of that kind
    /// holds against the rule changes in force, in one AND.
    #[inline]
    pub fn rules_of(&self, kind: RuleKind) -> Rules {
        self.rule_changes.of_kind(kind)
    }

    /// The rule changes that govern casting every spell, whatever it is: a check of one of
    /// them in force reads no filter.
    #[inline]
    pub fn rules_of_every_spell(&self) -> Rules {
        self.rule_changes.of_every_spell()
    }

    /// The rule changes that apply to players, which the layers find
    /// ([`RuleKind::of_players`]).
    #[inline]
    pub fn player_rules(&self) -> Rules {
        self.rule_changes.of_players()
    }

    /// The kinds of event any card of this game replaces, and whether any changes a rule for
    /// players.
    #[inline]
    pub fn replaces_any(&self) -> Replaces {
        self.replaces_any
    }

    /// Whether a card of the game has an activated ability that may do nothing.
    #[inline]
    pub fn needy(&self) -> bool {
        self.needy
    }

    /// Whether a card of the game has `capability`.
    #[inline]
    pub fn can(&self, capability: Capability) -> bool {
        self.capabilities & capability.bit() != 0
    }

    /// Whether a card of the game has `one` or `other`, in one AND.
    #[inline]
    pub fn can_either(&self, one: Capability, other: Capability) -> bool {
        self.capabilities & (one.bit() | other.bit()) != 0
    }

    #[inline]
    pub fn cards(&self) -> &'static CardTable {
        self.cards
    }

    /// The definition behind a game index.
    #[inline]
    pub fn def(&self, card: impl CardId) -> &'static CardDef {
        self.defs[card.index()]
    }

    /// How many distinct cards the game has.
    #[inline]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Every game index, in order: the cards of the first deck by first appearance, then
    /// the second's, then their tokens and emblems.
    pub fn indices<C: CardId>(&self) -> impl ExactSizeIterator<Item = C> + use<C> {
        (0..self.defs.len()).map(C::from_index)
    }

    /// A player's deck as game indices, in decklist order.
    #[inline]
    pub fn deck<C: CardId>(&self, player: Seat) -> impl ExactSizeIterator<Item = C> + '_ {
        self.decks[player.index()].iter().map(|&i| C::from_index(i))
    }

    /// How many cards a player's deck has.
    #[inline]
    pub fn deck_len(&self, player: Seat) -> usize {
        self.decks[player.index()].len()
    }
}

/// Each layout's game index ([`Layout::Card`]), as a family [`Layout::of`] chooses from.
struct GameIndex;

impl PerLayout for GameIndex {
    type Of<L: Layout> = L::Card;
}

/// What the engine's scans over every permanent read of a face, found once per game rather
/// than from its abilities each time.
#[derive(Clone, Copy, Debug)]
pub struct FaceFacts {
    /// What it makes as a mana source.
    pub mana: ManaFace,
    /// The kinds of mana the intrinsic abilities of its basic land types make that its
    /// abilities do not write out, a bit per [`beryl_card::ManaColor`] ([`land_mana`]).
    pub land_mana: u8,
    /// What its static abilities read as the layers apply that no layer writes.
    pub inputs: Inputs,
    /// A bit per [`AbilityIx`] of its activated abilities.
    activated: u64,
}

impl FaceFacts {
    /// No face: never read, as no object has it up.
    const NONE: FaceFacts = FaceFacts {
        mana: ManaFace::Read,
        land_mana: 0,
        inputs: Inputs::NONE,
        activated: 0,
    };

    /// Whether it has an activated ability ([CR 602.1]): "Activated abilities have a cost and an
    /// effect."
    ///
    /// [CR 602.1]: https://yawgatog.com/resources/magic-rules/#R6021
    #[inline]
    pub fn activates(self) -> bool {
        self.activated != 0
    }

    /// Its activated abilities, in the face's order.
    #[inline]
    pub fn activated(self) -> impl Iterator<Item = AbilityIx> {
        let mut left = self.activated;
        std::iter::from_fn(move || {
            (left != 0).then(|| {
                let i = u8::try_from(left.trailing_zeros()).expect("an ability bit fits in u8");
                left &= left - 1;
                AbilityIx::new(i)
            })
        })
    }

    /// Out of line: [`GameTable::facts`] is inlined into the engine's scans of every
    /// permanent, and this is its rare path (a face past the second, or face down). Inlined
    /// there it costs the rollout benchmark 1.6% of its instructions.
    #[inline(never)]
    fn of(face: &Face) -> FaceFacts {
        assert!(face.abilities.len() <= 64, "a face of at most 64 abilities");
        let mut facts = FaceFacts {
            mana: ManaFace::of(face),
            land_mana: land_mana(face, &Words::NONE),
            inputs: Inputs::of(face),
            activated: 0,
        };
        for (i, a) in face.abilities.iter().enumerate() {
            if a.kind() == AbilityKind::Activated {
                facts.activated |= 1 << i;
            }
        }
        facts
    }
}

/// What a face makes as a source, found once per game ([`GameTable::facts`]) for the
/// many faces the mana solver reads: most implemented permanents are not sources, or sources whose
/// mana depends on nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManaFace {
    /// No mana ability the solver taps for: never a source.
    Not,
    /// Mana abilities the solver taps for, none limited to a kind of permanent you control:
    /// the kinds they make, how much, and whether the face has another ability.
    Plain { kinds: u8, amount: u8, other: bool },
    /// Anything else, which the solver reads from the face.
    Read,
}

impl ManaFace {
    /// What `face` makes as a source, with its basic land types' mana ([`land_mana`]).
    pub fn of(face: &Face) -> ManaFace {
        let intrinsic = land_mana(face, &Words::NONE);
        let simple = || face.abilities.iter().filter(|a| is_simple(a));
        if intrinsic == 0
            && !face
                .abilities
                .iter()
                .any(|a| is_simple(a) || is_sacrificed(a))
        {
            return ManaFace::Not;
        }
        if (intrinsic == 0 && simple().next().is_none())
            || simple().any(|a| a.only_if_you_control().is_some())
        {
            return ManaFace::Read;
        }
        let kinds = simple().fold(intrinsic, |k, a| k | a.produces().colors);
        let amount = simple()
            .next_back()
            .map_or(u8::from(intrinsic != 0), |a| a.produces().amount);
        if kinds == 0 || (intrinsic != 0 && amount != 1) {
            return ManaFace::Read;
        }
        ManaFace::Plain {
            kinds,
            amount,
            other: has_other_ability(face),
        }
    }
}

/// The mana of the intrinsic abilities of `face`'s basic land types ([CR 305.6]), a bit per
/// color, less what it writes out as Plains does, read through `words`: "Island" makes {U}.
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
#[inline]
pub fn land_mana(face: &Face, words: &Words) -> u8 {
    if !face.types.intersects(beryl_card::Types::LAND) {
        return 0;
    }
    let written = face
        .abilities
        .iter()
        .filter(|a| is_simple(a) && a.produces().amount == 1)
        .fold(0, |k, a| k | a.produces().colors);
    if words.changes_subtypes() {
        return crate::effects::text::changed_land_mana(face, words, written);
    }
    let printed = BasicLandTypes::of(face.subtypes.read(&Words::NONE));
    printed.mana().intrinsic_mana() & !written
}

/// Whether a face has an ability other than those the solver taps for, which tapping it for
/// mana would cost. Abilities that work only from a hand, such as cycling, don't count.
pub fn has_other_ability(face: &Face) -> bool {
    face.abilities.iter().any(|a| {
        matches!(a.kind(), AbilityKind::Activated | AbilityKind::Mana)
            && !is_simple(a)
            && !a.cost().does(CostAction::Discard)
    })
}

/// Whether an ability is one the solver taps for: a mana ability whose whole cost is {T}, and
/// whose mana may be spent on anything.
#[inline]
pub fn is_simple(a: &Ability) -> bool {
    is_tap_for_mana(a) && a.spend().is_none()
}

/// Whether an ability is a mana ability whose whole cost is {T}: [`is_simple`], or one whose
/// mana is restricted, which the solver taps only for a payment the restriction accepts.
#[inline]
pub fn is_tap_for_mana(a: &Ability) -> bool {
    a.kind() == AbilityKind::Mana
        && a.cost().mana.is_zero()
        && matches!(a.cost().parts(), [CostPart::This(CostAction::Tap)])
}

/// Whether an ability is a mana ability whose whole cost is {T} and sacrificing its source, as a
/// Treasure's ([CR 111.10a]): "{T}, Sacrifice this token: Add one mana of any color." Spending the
/// permanent is a resource a plan consumes (§10): the solver taps such a source only when the cost
/// needs it, or when sacrificing it keeps more later costs payable.
///
/// [CR 111.10a]: https://yawgatog.com/resources/magic-rules/#R11110a
#[inline]
pub fn is_sacrificed(a: &Ability) -> bool {
    a.kind() == AbilityKind::Mana
        && a.cost().mana.is_zero()
        && matches!(
            a.cost().parts(),
            [
                CostPart::This(CostAction::Tap),
                CostPart::This(CostAction::Sacrifice)
            ] | [
                CostPart::This(CostAction::Sacrifice),
                CostPart::This(CostAction::Tap)
            ]
        )
}

#[cfg(test)]
mod tests;
