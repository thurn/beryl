//! What the rollout policy knows about each card of a game (architecture §19.1, decision
//! D21): a value, estimated from mana value and stats, and a few flags of card knowledge,
//! each derived from what the card's definition says its abilities do (plan K9). Hand-tuned
//! values for the supported decks come from the per-deck overlay ([`crate::overlay`]),
//! which [`Knowledge::Tuned`] applies on top. Built once per game, indexed by game-table
//! index (§6.4), so a decision reads it with one load.

use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Effect, Face, Keywords, SpecIx, TriggerEvent, Types,
};
use beryl_engine::query::DefinitionKey;
use beryl_engine::{CardId, CardIx, GameTable, LargeCardIx};

use crate::overlay;

/// One card's entry.
#[derive(Clone, Copy, Debug, Default)]
pub struct CardInfo {
    pub value: i16,
    pub wheel: bool,
    pub never_cast: bool,
    /// Bit `i`: ability `i` of the front face targets to help its controller.
    pub friendly: u8,
    /// Bit `i`: never activate ability `i` of the front face.
    pub never: u8,
    /// Bit `i`: ability `i` of the front face draws its controller cards.
    pub draws: u8,
    /// Bit `i`: ability `i` of the front face wins by drawing the library empty.
    pub empties: u8,
    /// Whether a face of it wins instead of losing by drawing from an empty library, as
    /// Jace, Wielder of Mysteries does.
    pub wins_instead: bool,
}

impl CardInfo {
    pub fn friendly(self, ability: u8) -> bool {
        ability < 8 && self.friendly & (1 << ability) != 0
    }

    pub fn never(self, ability: u8) -> bool {
        ability < 8 && self.never & (1 << ability) != 0
    }

    pub fn draws(self, ability: u8) -> bool {
        ability < 8 && self.draws & (1 << ability) != 0
    }

    pub fn empties(self, ability: u8) -> bool {
        ability < 8 && self.empties & (1 << ability) != 0
    }
}

/// Every card's entry, by game index.
#[derive(Clone, Debug)]
pub struct CardValues {
    /// The first 255, flat: every index of a [`beryl_engine::Small`] game, which reads its
    /// row with no bounds check.
    cards: [CardInfo; CardIx::COUNT],
    /// The rows past them, of a table only a wider layout numbers.
    more: Vec<CardInfo>,
}

/// Where card knowledge comes from, from the most general to the most hand-made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Knowledge {
    /// Every card's value estimated from what its face shows (mana value, stats, loyalty),
    /// and its flags derived from its definition.
    Estimate,
    /// [`Knowledge::Estimate`] with the per-deck overlay applied on top ([`crate::overlay`]):
    /// the values, the cards never cast and the abilities never activated that were
    /// hand-tuned for the WU Control mirror. A card with no overlay entry is as estimated.
    #[default]
    Tuned,
}

impl CardValues {
    /// The hand-tuned values ([`Knowledge::Tuned`]).
    pub fn new(t: &GameTable) -> CardValues {
        Self::with(t, Knowledge::Tuned)
    }

    /// The values of the tier `knowledge`.
    pub fn with(t: &GameTable, knowledge: Knowledge) -> CardValues {
        let mut cards = [CardInfo::default(); CardIx::COUNT];
        let mut more = Vec::new();
        for card in t.indices::<LargeCardIx>() {
            let row = info(t.def(card), knowledge);
            match cards.get_mut(card.index()) {
                Some(flat) => *flat = row,
                None => more.push(row),
            }
        }
        CardValues { cards, more }
    }

    /// The row at a game index.
    #[inline]
    fn row(&self, index: usize) -> CardInfo {
        match self.cards.get(index) {
            Some(&info) => info,
            None => self.more[index - CardIx::COUNT],
        }
    }

    #[inline]
    pub fn get(&self, card: impl CardId) -> CardInfo {
        self.row(card.index())
    }

    /// What is known of the definition an object's values are read from
    /// ([`beryl_engine::State::copiable`]).
    #[inline]
    pub fn of<C: CardId>(&self, key: DefinitionKey<C>) -> CardInfo {
        self.row(key.index())
    }
}

/// What `knowledge` knows of the card `def`.
pub(crate) fn info(def: &CardDef, knowledge: Knowledge) -> CardInfo {
    let mut info = derive(def);
    if knowledge == Knowledge::Tuned {
        overlay::apply(def.name, &mut info);
    }
    info
}

/// What the definition says of the card: its estimated value and the flags its abilities'
/// effect data states. A flag of ability `i` is read from the front face, as the policy
/// reads it, and only for the first eight abilities, which the bits hold.
fn derive(def: &CardDef) -> CardInfo {
    let front = def.front();
    let bits = |fact: fn(&Ability, &Effect) -> bool| {
        front
            .abilities
            .iter()
            .take(8)
            .enumerate()
            .filter(|(_, a)| effect(a).is_some_and(|e| fact(a, e)))
            .fold(0, |bits, (i, _)| bits | 1 << i)
    };
    let abilities = || def.faces.iter().flat_map(|f| f.abilities);
    CardInfo {
        value: estimate(front),
        wheel: abilities()
            .any(|a| a.kind() == AbilityKind::Spell && effect(a).is_some_and(Effect::wheels)),
        never_cast: false,
        friendly: bits(helps_first_target),
        never: 0,
        draws: bits(draws),
        empties: bits(|_, e| e.wins_by_drawing_out()),
        wins_instead: abilities().any(beryl_engine::wins_instead_of_drawing),
    }
}

/// The ability's effect data, or none for a Rust body, which nothing can read.
fn effect(a: &Ability) -> Option<&'static Effect> {
    match a.body() {
        Behavior::Data(effect) => Some(effect),
        Behavior::Custom(_) => None,
    }
}

/// Whether the ability takes cards from its controller's library
/// ([`Effect::takes_from_own_library`]) at its controller's choice: a spell or an activated
/// ability, or a triggered ability that triggers at the beginning of a step or phase, which
/// keeps taking cards every turn once its permanent is cast, as "At the beginning of each
/// opponent's upkeep, manifest dread" does. Such an ability is worth nothing while the
/// library runs low and an empty library would lose (`Policy::may_draw`), and such a
/// trigger makes its permanent not worth casting then. A trigger on another event, such as
/// "When this enters, draw a card", takes cards once or only when its controller acts, and
/// is not counted.
fn draws(a: &Ability, e: &Effect) -> bool {
    let chosen = match a.kind() {
        AbilityKind::Spell | AbilityKind::Activated => true,
        AbilityKind::Triggered => a.trigger().is_some_and(|trigger| {
            matches!(
                trigger.event,
                TriggerEvent::Step(_) | TriggerEvent::MainPhase
            )
        }),
        _ => false,
    };
    chosen && e.takes_from_own_library()
}

/// Whether the ability's first target is one its effect helps ([`Effect::helps_target`]),
/// so it belongs on its controller's side although its filter allows either side.
fn helps_first_target(a: &Ability, e: &Effect) -> bool {
    !a.targets().is_empty() && e.helps_target(SpecIx::new(0))
}

/// A value from what the face shows, for cards and tokens without a hand-tuned one: a
/// creature by its stats and evasion, a planeswalker by its loyalty, anything else by its
/// mana value.
fn estimate(face: &Face) -> i16 {
    let mana_value = i16::from(face.cost.mana_value());
    if face.types.intersects(Types::CREATURE) {
        let stats = i16::from(face.power.unwrap_or(0)) + i16::from(face.toughness.unwrap_or(0));
        let flying = if face.keywords.intersects(Keywords::FLYING) {
            5
        } else {
            0
        };
        4 * stats + 3 * mana_value + flying
    } else if face.types.intersects(Types::PLANESWALKER) {
        15 * i16::from(face.loyalty.unwrap_or(0))
    } else if face.types.intersects(Types::LAND) {
        10
    } else {
        5 + 4 * mana_value
    }
}
