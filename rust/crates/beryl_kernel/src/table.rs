//! The static card table and the per-game table built from two decklists (architecture §6.4,
//! decision D20).

use beryl_card::{
    Ability, AbilityIx, AbilityKind, CardDef, Cost, FACE_DOWN, FACE_DOWN_COPY, Face, FaceIx, Needs,
    Rule, Trigger,
};

use crate::event::kind;
use crate::ids::{CardIx, Seat};
use crate::state::DECK_SLOTS;

/// Every card the program knows, built once and shared by every thread as a `&'static`.
#[derive(Debug)]
pub struct CardTable {
    cards: &'static [&'static CardDef],
}

impl CardTable {
    pub const fn new(cards: &'static [&'static CardDef]) -> Self {
        Self { cards }
    }

    pub fn get(&self, name: &str) -> Option<&'static CardDef> {
        self.cards.iter().copied().find(|c| c.name == name)
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
    /// A deck has more cards than the engine reserves slots for.
    DeckTooLarge { player: Seat, cards: usize },
    /// The decks and their tokens need more game indices than [`CardIx`] has.
    TooManyCards,
}

/// The cards one game can contain, under dense game-local indices: the [`CardIx`] that
/// `State` stores for an object's card. Built once per game and shared, read-only, by every thread
/// searching it, or cloned into an agent that outlives the borrow.
#[derive(Clone, Debug)]
pub struct GameTable {
    cards: &'static CardTable,
    defs: Vec<&'static CardDef>,
    decks: [Vec<CardIx>; 2],
    /// Per game index, the event kinds its triggered abilities listen to.
    listens: Vec<u16>,
    /// Every kind any card of the game listens to.
    listens_any: u16,
    /// Per game index, the facts of its first two faces.
    faces: Vec<[FaceFacts; 2]>,
    /// Per game index, the [`Rule`]s any of its faces, or face down, has.
    rules: Vec<u8>,
    /// The game index of [`FACE_DOWN_COPY`].
    face_down_copy: CardIx,
    /// Whether a card of the game has an activated ability that may do nothing
    /// ([`beryl_card::Needs`]), so that no other game pays to check.
    needy: bool,
}

impl GameTable {
    /// Assigns game indices in order of first appearance, the first deck before the second,
    /// then the tokens and emblems those cards can create, then [`FACE_DOWN_COPY`], so the
    /// same decklists always give the same table.
    pub fn new(cards: &'static CardTable, decks: [&Decklist; 2]) -> Result<Self, TableError> {
        let mut defs: Vec<&'static CardDef> = Vec::new();
        let mut indices: [Vec<CardIx>; 2] = [Vec::new(), Vec::new()];
        for (player, list) in Seat::BOTH.into_iter().zip(decks) {
            for &(count, name) in list {
                let def = cards.get(name).ok_or(TableError::UnknownCard(name))?;
                let index = if let Some(i) = defs.iter().position(|d| std::ptr::eq(*d, def)) {
                    i
                } else {
                    defs.push(def);
                    defs.len() - 1
                };
                let index = u8::try_from(index)
                    .ok()
                    .and_then(CardIx::try_new)
                    .ok_or(TableError::TooManyCards)?;
                for _ in 0..count {
                    indices[player.index()].push(index);
                }
            }
            if indices[player.index()].len() > DECK_SLOTS {
                return Err(TableError::DeckTooLarge {
                    player,
                    cards: indices[player.index()].len(),
                });
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
        if defs.len() > CardIx::COUNT {
            return Err(TableError::TooManyCards);
        }
        let face_down_copy = CardIx::new((defs.len() - 1) as u8);
        let listens: Vec<u16> = defs.iter().map(|d| listens(d)).collect();
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
            .map(|d| d.faces.iter().fold(down, |r, f| r | FaceFacts::of(f).rules))
            .collect();
        Ok(Self {
            cards,
            defs,
            decks: indices,
            listens,
            listens_any,
            faces,
            rules,
            face_down_copy,
            needy,
        })
    }

    /// The game index of [`FACE_DOWN_COPY`], the card a copy of a face-down permanent copies.
    #[inline]
    pub fn face_down_copy(&self) -> CardIx {
        self.face_down_copy
    }

    /// The game index of a card or token, if the game can contain it.
    pub fn index_of(&self, def: &CardDef) -> Option<CardIx> {
        self.defs
            .iter()
            .position(|d| std::ptr::eq(*d, def))
            .map(|i| CardIx::new(i as u8))
    }

    /// The event kinds the triggered abilities of the card at `card` listen to.
    #[inline]
    pub fn listens(&self, card: CardIx) -> u16 {
        self.listens[card.index()]
    }

    /// The facts of the face `face` of the card at `card`: found once per game for its first
    /// two faces, and read from the face itself for any other, or face down.
    #[inline]
    pub fn facts(&self, card: CardIx, face: FaceIx) -> FaceFacts {
        match self.faces[card.index()].get(usize::from(face.get())) {
            Some(&facts) => facts,
            None => FaceFacts::of(self.def(card).face(face)),
        }
    }

    /// Whether some face of the card at `card` has a static ability with `rule`: so whether it
    /// can, whichever face is up.
    #[inline]
    pub fn may_have_rule(&self, card: CardIx, rule: Rule) -> bool {
        self.rules[card.index()] & rule_bit(rule) != 0
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

    #[inline]
    pub fn cards(&self) -> &'static CardTable {
        self.cards
    }

    /// The definition behind a game index.
    #[inline]
    pub fn def(&self, card: CardIx) -> &'static CardDef {
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

    /// A player's deck as game indices, in decklist order.
    #[inline]
    pub fn deck(&self, player: Seat) -> &[CardIx] {
        &self.decks[player.index()]
    }
}

/// What the engine's scans over every permanent read of a face, found once per game rather
/// than from its abilities each time.
#[derive(Clone, Copy, Debug)]
pub struct FaceFacts {
    /// What it makes as a mana source.
    pub mana: ManaFace,
    /// A bit per [`AbilityIx`] of its activated abilities.
    activated: u64,
    /// A bit per [`Rule`] of its static abilities.
    rules: u8,
}

impl FaceFacts {
    /// No face: never read, as no object has it up.
    const NONE: FaceFacts = FaceFacts {
        mana: ManaFace::Read,
        activated: 0,
        rules: 0,
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
            rules: 0,
        };
        for (i, a) in face.abilities.iter().enumerate() {
            match a.kind {
                AbilityKind::Activated => facts.activated |= 1 << i,
                AbilityKind::Static => facts.rules |= rule_bit(a.rule),
                _ => {}
            }
        }
        facts
    }

    /// Whether one of its static abilities has `rule`.
    #[inline]
    pub fn has_rule(self, rule: Rule) -> bool {
        self.rules & rule_bit(rule) != 0
    }
}

#[inline]
fn rule_bit(rule: Rule) -> u8 {
    1 << (rule as u8)
}

/// The event kinds a card's triggered abilities listen to, on any face.
fn listens(def: &CardDef) -> u16 {
    let mut mask = 0;
    for face in def.faces {
        for ability in face.abilities {
            mask |= match ability.trigger {
                Trigger::None | Trigger::Delayed(_) => 0,
                Trigger::Enters => kind::ENTERS,
                Trigger::Dies => kind::DIES,
                Trigger::Attacks | Trigger::YouAttack => kind::ATTACK,
                Trigger::CombatDamageToPlayer => kind::DAMAGE,
                Trigger::Step(..) => kind::STEP,
                Trigger::YouCreateTokens(_) => kind::TOKENS,
                Trigger::YouCast => kind::CAST,
                Trigger::YouDiscard => kind::DISCARD,
                Trigger::YouDraw => kind::DRAW,
            };
        }
    }
    mask
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
            && !a.cost.discard_self
    })
}

/// Whether an ability is one the solver taps for: a mana ability whose whole cost is {T}.
#[inline]
pub fn is_simple(a: &Ability) -> bool {
    a.kind == AbilityKind::Mana && a.cost == Cost::TAP
}

/// Whether an ability is a mana ability whose whole cost is {T} and sacrificing its source,
/// as a Treasure's ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)):
/// "{T}, Sacrifice this token: Add one mana of any color." Spending the permanent is a
/// resource a plan consumes (§10): the solver taps such a source only when the cost needs it,
/// or when sacrificing it keeps more later costs payable.
#[inline]
pub fn is_sacrificed(a: &Ability) -> bool {
    a.kind == AbilityKind::Mana
        && a.cost
            == Cost {
                sacrifice_self: true,
                ..Cost::TAP
            }
}
