//! The static card table and the per-game table built from two decklists (architecture §6.4,
//! decision D20).

use beryl_card::{
    Ability, AbilityIx, AbilityKind, CardDef, CostAction, CostPart, FACE_DOWN, FACE_DOWN_COPY,
    Face, FaceIx, Needs, Rule, Rules,
};

use crate::event::kind;
use crate::ids::{CardId, CardIx, LargeCardIx, Seat};
use crate::layout::{Layout, PerLayout, PerSeat};
use crate::listener::{self, Listener, Unmatched};
use crate::payable::{self, Unpayable};
use crate::static_ability::{self, Unapplied};

/// Every card the program knows, built once and shared by every thread as a `&'static`.
#[derive(Debug)]
pub struct CardTable {
    cards: &'static [&'static CardDef],
    /// Each name's first card, built on first use by [`CardTable::get`]. Never iterated: the order
    /// the engine sees is [`CardTable::cards`]'s.
    by_name: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static CardDef>>,
}

impl CardTable {
    pub const fn new(cards: &'static [&'static CardDef]) -> Self {
        Self {
            cards,
            by_name: std::sync::OnceLock::new(),
        }
    }

    /// The card called `name`: the first of that name in the table, as a scan of
    /// [`CardTable::cards`] would find it.
    pub fn get(&self, name: &str) -> Option<&'static CardDef> {
        self.by_name
            .get_or_init(|| {
                let mut map = std::collections::HashMap::with_capacity(self.cards.len());
                for &card in self.cards {
                    map.entry(card.name).or_insert(card);
                }
                map
            })
            .get(name)
            .copied()
    }

    #[inline]
    pub fn cards(&self) -> &'static [&'static CardDef] {
        self.cards
    }
}

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
    /// A card of the game, or a token or emblem one makes, has a static ability whose
    /// continuous effect the layers have no code to apply ([`static_ability::applied`]): it
    /// would change nothing, so no game is played with it.
    Static {
        card: &'static str,
        unapplied: Unapplied,
    },
    /// A filter of a card of the game, or of a token or emblem one makes, names players the
    /// engine does not read in a filter ([`crate::filter::reads`]): "that player controls",
    /// "its owner controls".
    Players { card: &'static str },
    /// A card of the game, or a token or emblem one makes, has a cost the engine has no
    /// code to pay ([`payable`]): it would be played without paying it, so no game is played
    /// with it.
    Cost {
        card: &'static str,
        unpayable: Unpayable,
    },
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
    /// Per game index, the event kinds its triggered abilities listen to, and
    /// [`kind::RULE_CHANGE`] if a static ability of it changes a rule for players.
    listens: Vec<u16>,
    /// Every kind any card of the game listens to.
    listens_any: u16,
    /// Every triggered ability of the game's cards under the kind of event it listens to:
    /// each card's in turn, in the order its faces list them.
    listeners: Vec<Listener>,
    /// Per game index, where its listeners start in `listeners`, and after the last where
    /// they end.
    listeners_from: Vec<u32>,
    /// Per game index, the facts of its first two faces.
    faces: Vec<[FaceFacts; 2]>,
    /// Per game index, the [`Rule`]s any of its faces, or face down, has.
    rules: Vec<Rules>,
    /// The game index of [`FACE_DOWN_COPY`], in the width of each layout's
    /// ([`GameTable::face_down_copy`]); index 0, never read, in a width that does not number
    /// it.
    face_down_copy: (CardIx, LargeCardIx),
    /// Whether a card of the game has an activated ability that may do nothing
    /// ([`beryl_card::Needs`]), so that no other game pays to check.
    needy: bool,
    /// A bit per [`Capability`] a card of the game has.
    capabilities: u8,
}

/// Something a game's cards may make the stores keep (architecture §6.4, the crate's
/// contract, "Stores"), found once per game from every face of every card it can contain.
/// What no card of a game can need is not kept in it and costs it one AND.
///
/// The kinds of continuous effect have no bit here: the Effects store knows exactly which
/// are live ([`crate::effects`]), which a table could only bound, since a Rust body is
/// opaque to it. These are what no live entry can say: whether to keep a count nothing has
/// yet read. Whether a static ability changes a rule for players is no capability of the
/// game either: it is a bit of each card's listener mask ([`kind::RULE_CHANGE`]), which says
/// it of the permanents on the battlefield now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// A card reads whether a permanent entered the battlefield this turn, so that is
    /// tracked (by-w8gv.7.8).
    EnteredThisTurn,
    /// A card has a loyalty ability, so whether one of a permanent's was activated this
    /// turn is tracked (by-w8gv.7.8)
    /// ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)): "A player may
    /// activate a loyalty ability of a permanent they control any time they have priority
    /// and the stack is empty during a main phase of their turn, but only if no player has
    /// previously activated a loyalty ability of that permanent that turn."
    LoyaltyActivated,
}

impl Capability {
    /// The capabilities a face gives a game that can contain it.
    fn of(face: &Face) -> u8 {
        let mut found = 0;
        for a in face.abilities {
            if a.rule == Rule::InstantLoyaltyTheTurnItEntered {
                found |= Capability::EnteredThisTurn.bit();
            }
            if a.cost.is_loyalty() {
                found |= Capability::LoyaltyActivated.bit();
            }
        }
        found
    }

    #[inline]
    const fn bit(self) -> u8 {
        1 << (self as u8)
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
        defs.push(&FACE_DOWN_COPY);
        let last = defs.len() - 1;
        let face_down_copy = (
            CardIx::try_from_index(last).unwrap_or_default(),
            LargeCardIx::try_from_index(last).unwrap_or_default(),
        );
        for def in &defs {
            payable::check(def).map_err(|unpayable| TableError::Cost {
                card: def.name,
                unpayable,
            })?;
        }
        let mut listeners = Vec::new();
        let mut listeners_from = Vec::with_capacity(defs.len() + 1);
        let mut listens = Vec::with_capacity(defs.len());
        for def in &defs {
            let of = listener::of(def).map_err(|unmatched| TableError::Trigger {
                card: def.name,
                unmatched,
            })?;
            static_ability::check(def).map_err(|unapplied| TableError::Static {
                card: def.name,
                unapplied,
            })?;
            if !crate::filter::reads_players_of(def) {
                return Err(TableError::Players { card: def.name });
            }
            listens.push(of.iter().fold(rule_change(def), |mask, l| mask | l.kind));
            listeners_from.push(u32::try_from(listeners.len()).expect("fewer listeners"));
            listeners.extend(of);
        }
        listeners_from.push(u32::try_from(listeners.len()).expect("fewer listeners"));
        let listens_any = listens.iter().fold(0, |a, &b| a | b);
        let needy = defs.iter().any(|d| {
            d.faces
                .iter()
                .flat_map(|f| f.abilities)
                .any(|a| a.needs != Needs::Nothing)
        });
        let faces = defs
            .iter()
            .map(|d| {
                let of = |i: usize| d.faces.get(i).map_or(FaceFacts::NONE, FaceFacts::of);
                [of(0), of(1)]
            })
            .collect();
        let down = FaceFacts::of(&FACE_DOWN).rules;
        let rules = defs
            .iter()
            .map(|d| {
                d.faces
                    .iter()
                    .fold(down, |r, f| r.union(FaceFacts::of(f).rules))
            })
            .collect();
        let capabilities = defs
            .iter()
            .flat_map(|d| d.faces)
            .fold(Capability::of(&FACE_DOWN), |c, f| c | Capability::of(f));
        Ok(Self {
            cards,
            defs,
            decks: indices,
            listens,
            listens_any,
            listeners,
            listeners_from,
            faces,
            rules,
            face_down_copy,
            needy,
            capabilities,
        })
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

    /// The event kinds the triggered abilities of the card at `card` listen to. The crate's
    /// own: others ask [`crate::Copiable::may_listen`], which is nothing for a face-down
    /// permanent whatever its hidden card listens to.
    #[inline]
    pub(crate) fn listens(&self, card: impl CardId) -> u16 {
        self.listens[card.index()]
    }

    /// The triggered abilities of the face `face` of the card at `card` that listen to events
    /// of `kind`, in the order the face lists them: what an event of that kind is matched
    /// against, and nothing else of the face is read. A face-down permanent's face has none
    /// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)).
    #[inline]
    pub fn listeners(
        &self,
        card: impl CardId,
        face: FaceIx,
        kind: u16,
    ) -> impl Iterator<Item = &Listener> {
        let from = self.listeners_from[card.index()] as usize;
        let to = self.listeners_from[card.index() + 1] as usize;
        self.listeners[from..to]
            .iter()
            .filter(move |l| l.kind & kind != 0 && l.face == face)
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

    /// The rules the static abilities of any face of the card at `card` change: more than
    /// the face that is up may, so a set to skip a card by and nothing else. The crate's
    /// own: others ask [`crate::Copiable::has_rule`] and [`crate::Copiable::player_rules`].
    #[inline]
    pub(crate) fn may_have_rules(&self, card: impl CardId) -> Rules {
        self.rules[card.index()]
    }

    /// The event kinds any card of this game listens to.
    #[inline]
    pub fn listens_any(&self) -> u16 {
        self.listens_any
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
    /// A bit per [`AbilityIx`] of its activated abilities.
    activated: u64,
    /// The [`Rule`]s its static abilities change.
    rules: Rules,
}

impl FaceFacts {
    /// No face: never read, as no object has it up.
    const NONE: FaceFacts = FaceFacts {
        mana: ManaFace::Read,
        activated: 0,
        rules: Rules::NONE,
    };

    /// Whether it has an activated ability
    /// ([CR 602.1](https://yawgatog.com/resources/magic-rules/#R6021)): "Activated abilities
    /// have a cost and an effect."
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
                let i = left.trailing_zeros() as u8;
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
            activated: 0,
            rules: Rules::NONE,
        };
        for (i, a) in face.abilities.iter().enumerate() {
            match a.kind {
                AbilityKind::Activated => facts.activated |= 1 << i,
                AbilityKind::Static if a.rule != Rule::None => {
                    facts.rules = facts.rules.with(a.rule);
                }
                _ => {}
            }
        }
        facts
    }

    /// The rules its static abilities change.
    #[inline]
    pub fn rules(self) -> Rules {
        self.rules
    }
}

/// [`kind::RULE_CHANGE`] if a static ability of any face of a card changes a rule for
/// players, the bit of its listener mask no event has.
fn rule_change(def: &CardDef) -> u16 {
    let changes = def
        .faces
        .iter()
        .flat_map(|face| face.abilities)
        .any(|ability| ability.kind == AbilityKind::Static && ability.rule.whose().is_some());
    if changes { kind::RULE_CHANGE } else { 0 }
}

/// What a face makes as a source, found once per game ([`GameTable::facts`]) for the
/// many faces the mana solver reads: the cube's permanents are mostly not sources, or sources whose
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
    pub fn of(face: &Face) -> ManaFace {
        let simple = || face.abilities.iter().filter(|a| is_simple(a));
        if !face
            .abilities
            .iter()
            .any(|a| is_simple(a) || is_sacrificed(a))
        {
            return ManaFace::Not;
        }
        if simple().next().is_none() || simple().any(|a| a.only_if_you_control.is_some()) {
            return ManaFace::Read;
        }
        let kinds = simple().fold(0, |k, a| k | a.produces.colors);
        let amount = simple().next_back().map_or(0, |a| a.produces.amount);
        if kinds == 0 {
            return ManaFace::Read;
        }
        ManaFace::Plain {
            kinds,
            amount,
            other: has_other_ability(face),
        }
    }
}

/// Whether a face has an ability other than those the solver taps for, which tapping it for
/// mana would cost. Abilities that work only from a hand, such as cycling, don't count.
pub fn has_other_ability(face: &Face) -> bool {
    face.abilities.iter().any(|a| {
        matches!(a.kind, AbilityKind::Activated | AbilityKind::Mana)
            && !is_simple(a)
            && !a.cost.does(CostAction::Discard)
    })
}

/// Whether an ability is one the solver taps for: a mana ability whose whole cost is {T}.
#[inline]
pub fn is_simple(a: &Ability) -> bool {
    a.kind == AbilityKind::Mana
        && a.cost.mana.is_zero()
        && matches!(a.cost.parts(), [CostPart::This(CostAction::Tap)])
}

/// Whether an ability is a mana ability whose whole cost is {T} and sacrificing its source,
/// as a Treasure's ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)):
/// "{T}, Sacrifice this token: Add one mana of any color." Spending the permanent is a
/// resource a plan consumes (§10): the solver taps such a source only when the cost needs it,
/// or when sacrificing it keeps more later costs payable.
#[inline]
pub fn is_sacrificed(a: &Ability) -> bool {
    a.kind == AbilityKind::Mana
        && a.cost.mana.is_zero()
        && matches!(
            a.cost.parts(),
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
