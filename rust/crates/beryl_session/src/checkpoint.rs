//! Turn-start checkpoints in saves (architecture §14): the board as each turn begins, in the
//! rules' terms (each zone's cards by name, life, tapped, counters, attachments), so a replay
//! can check itself against the record and, where it can no longer go on, rebuild the game from
//! the latest turn start before it.
//!
//! A checkpoint is taken as the upkeep begins, once the untap step is over: the first place
//! the session sees a turn. A rebuild puts the board back as it stood just before the turn
//! began and begins the turn, so the untap step and what the turn's beginning triggers happen
//! again, as they did. Its active player's permanents untapped then are untapped already, so
//! the untap step changes nothing.
//!
//! What lasts past a turn's beginning off the board (an effect lasting until a player's next
//! turn, a delayed trigger, a permission to cast) is not recorded: a checkpoint names the first
//! such thing ([`State::off_board`]), and no game is rebuilt from it.

use beryl_engine::card::{CastingKind, CastingKinds, FaceIx, Types};
use beryl_engine::layout::Queue;
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{CounterKind, Memo, Object, ObjectFlags};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{CardIx, GameTable, Layout, Seat, Small, State, Zone, ZoneKey};
use serde::{Deserialize, Serialize};

/// The board as a turn begins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub turn: u16,
    pub active: u8,
    pub players: [Side; 2],
    /// Every permanent, in the order they entered the battlefield.
    pub battlefield: Vec<Permanent>,
    /// What rebuilding from this board would lose, in words, if anything: no game is rebuilt
    /// from it then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lingering: Option<String>,
}

/// One player's cards off the battlefield, and their life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    pub life: i16,
    /// The hand, in name order.
    pub hand: Vec<String>,
    /// The cards of the hand the other player knows, in name order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shown: Vec<String>,
    /// Top first.
    pub graveyard: Vec<String>,
    /// In name order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exile: Vec<Exiled>,
    /// How many cards the library holds.
    pub library: u16,
    /// The library's cards some player knows, top first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known: Vec<Known>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emblems: Vec<String>,
}

/// A card of a library that some player knows, as a look at the top puts it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Known {
    /// How many cards are above it.
    pub depth: u16,
    pub card: String,
    /// Who knows it.
    pub by: Vec<u8>,
}

/// A card in exile, and the permanent it is linked to, by its place in
/// [`Checkpoint::battlefield`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exiled {
    pub card: String,
    /// Exiled "with" that permanent, by its ability whose linked ability refers to it
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<u16>,
    /// Exiled until that permanent leaves the battlefield
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u16>,
}

/// A permanent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a permanent's statuses and how it was made, each yes or no in the rules"
)]
pub struct Permanent {
    /// The card's name, or the token's.
    pub card: String,
    pub owner: u8,
    /// Its controller before continuous effects.
    pub controller: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub face: Face,
    #[serde(default, skip_serializing_if = "is_default")]
    pub token: bool,
    /// A copy that is 1/1 by exception, such as an offspring token
    /// ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)).
    #[serde(default, skip_serializing_if = "is_default")]
    pub one_one: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tapped: bool,
    /// Not under its controller's control continuously since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    #[serde(default, skip_serializing_if = "is_default")]
    pub sick: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counters: Vec<Counter>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub damage: u8,
    /// The permanent it is attached to, by its place in [`Checkpoint::battlefield`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_to: Option<u16>,
    /// Cast for its impending cost
    /// ([CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)).
    #[serde(default, skip_serializing_if = "is_default")]
    pub impending: bool,
    /// Who knows a face-down permanent's card.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known: Vec<u8>,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Which face of a permanent is up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Face {
    #[default]
    Front,
    /// The back face of a double-faced card
    /// ([CR 712.2](https://yawgatog.com/resources/magic-rules/#R7122)).
    Back,
    /// Face down, a 2/2 with no name or text
    /// ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)).
    Down,
}

impl Face {
    fn of(face: FaceIx) -> Face {
        match face {
            FaceIx::FRONT => Face::Front,
            FaceIx::BACK => Face::Back,
            FaceIx::DOWN => Face::Down,
            face => unreachable!("no face {}", face.get()),
        }
    }

    fn ix(self) -> FaceIx {
        match self {
            Face::Front => FaceIx::FRONT,
            Face::Back => FaceIx::BACK,
            Face::Down => FaceIx::DOWN,
        }
    }
}

/// Counters of one kind on a permanent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counter {
    pub kind: CounterName,
    pub count: u8,
}

impl Counter {
    /// The counters on `o`, a kind at a time.
    pub fn on<L: Layout>(o: &Object<L>) -> Vec<Counter> {
        CounterName::ALL
            .into_iter()
            .filter_map(|kind| {
                let count = o.counters().get(kind.kind());
                (count > 0).then_some(Counter { kind, count })
            })
            .collect()
    }
}

/// A kind of counter, by its name in the rules
/// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CounterName {
    #[serde(rename = "+1/+1")]
    PlusOne,
    #[serde(rename = "-1/-1")]
    MinusOne,
    #[serde(rename = "loyalty")]
    Loyalty,
    #[serde(rename = "time")]
    Time,
    #[serde(rename = "story")]
    Story,
    #[serde(rename = "chorus")]
    Chorus,
}

impl CounterName {
    const ALL: [CounterName; 6] = [
        CounterName::PlusOne,
        CounterName::MinusOne,
        CounterName::Loyalty,
        CounterName::Time,
        CounterName::Story,
        CounterName::Chorus,
    ];

    fn kind(self) -> CounterKind {
        match self {
            CounterName::PlusOne => CounterKind::PlusOne,
            CounterName::MinusOne => CounterKind::MinusOne,
            CounterName::Loyalty => CounterKind::Loyalty,
            CounterName::Time => CounterKind::Time,
            CounterName::Story => CounterKind::Story,
            CounterName::Chorus => CounterKind::Chorus,
        }
    }
}

/// The players who know the object in `slot`.
fn knowers<L: Layout>(s: &State<L>, slot: L::Slot) -> Vec<u8> {
    Seat::BOTH
        .into_iter()
        .filter(|&p| s.knows(p, slot))
        .map(Seat::get)
        .collect()
}

impl Checkpoint {
    /// The board of `s` as its turn begins: called as the upkeep begins.
    pub fn of<L: Layout>(t: &GameTable, s: &State<L>) -> Checkpoint {
        let battlefield = s.battlefield();
        let place = |slot: L::Slot| {
            battlefield
                .iter()
                .position(|&b| b == slot)
                .map(|i| u16::try_from(i).expect("a battlefield fits u16"))
        };
        Checkpoint {
            turn: s.turn().number(),
            active: s.turn().active().get(),
            players: Seat::BOTH.map(|p| side(t, s, p, &place)),
            battlefield: battlefield
                .iter()
                .map(|&slot| permanent(t, s, slot, &place))
                .collect(),
            lingering: s.off_board(t),
        }
    }

    /// Arranges `s`, a new game's state, as this board stood just before its turn began, so
    /// the engine's next call begins the turn ([`State::begin_before_turn`]). The library's
    /// cards no player knows go in an order `seed` shuffles. Fails, saying why, where the
    /// game's decks cannot hold the board.
    pub fn arrange(&self, t: &GameTable, s: &mut State<Small>, seed: u64) -> Result<(), String> {
        let seat = |p: u8| Seat::try_new(p).ok_or_else(|| format!("no seat {p}"));
        let full = |_| "a zone is full".to_owned();
        s.board().begin_before_turn(self.turn, seat(self.active)?);
        for (p, side) in Seat::BOTH.into_iter().zip(&self.players) {
            s.board().set_life(p, side.life);
            for name in &side.emblems {
                s.board()
                    .add_emblem(p, game_index(t, name)?)
                    .map_err(full)?;
            }
            let mut shown = side.shown.clone();
            for name in &side.hand {
                let slot = from_library(t, s, p, name)?;
                s.board().place(slot, Zone::Hand).map_err(full)?;
                if let Some(i) = shown.iter().position(|n| n == name) {
                    shown.swap_remove(i);
                    s.board().set_known(p.opponent(), slot, true);
                }
            }
            if let Some(name) = shown.first() {
                return Err(format!(
                    "player {} shows a {name} not in their hand",
                    p.get()
                ));
            }
        }
        let mut slots = Vec::with_capacity(self.battlefield.len());
        for b in &self.battlefield {
            let owner = seat(b.owner)?;
            let slot = if b.token {
                s.board()
                    .place_token(owner, game_index(t, &b.card)?, b.face.ix(), b.one_one)
                    .map_err(full)?
            } else {
                let slot = from_library(t, s, owner, &b.card)?;
                s.board().place(slot, Zone::Battlefield).map_err(full)?;
                slot
            };
            s.board().set_controller(slot, seat(b.controller)?);
            if !b.token && b.face != Face::Front {
                s.board().set_face(slot, b.face.ix());
            }
            s.board().set_tapped(slot, b.tapped);
            s.board().set_sick(slot, b.sick);
            for c in &b.counters {
                s.board().set_counters(slot, c.kind.kind(), c.count);
            }
            s.board().set_damage(slot, b.damage);
            if b.impending {
                let impending = CastingKinds::NONE.with(CastingKind::Impending);
                s.board()
                    .remember(slot, Memo::Paid(impending))
                    .map_err(full)?;
            }
            if b.face == Face::Down {
                for p in Seat::BOTH {
                    s.board().set_known(p, slot, b.known.contains(&p.get()));
                }
            }
            slots.push(slot);
        }
        let permanent = |i: u16| {
            slots
                .get(usize::from(i))
                .copied()
                .ok_or_else(|| format!("no permanent {i}"))
        };
        for (b, &slot) in self.battlefield.iter().zip(&slots) {
            if let Some(i) = b.attached_to {
                s.board().attach(slot, permanent(i)?);
            }
        }
        for (p, side) in Seat::BOTH.into_iter().zip(&self.players) {
            // Top first: the last placed is on top.
            for name in side.graveyard.iter().rev() {
                let slot = from_library(t, s, p, name)?;
                s.board().place(slot, Zone::Graveyard).map_err(full)?;
            }
            for x in &side.exile {
                let slot = from_library(t, s, p, &x.card)?;
                s.board().place(slot, Zone::Exile).map_err(full)?;
                if let Some(i) = x.with {
                    s.board().set_exiled_with(slot, permanent(i)?);
                }
                if let Some(i) = x.until {
                    s.board().set_exiled_until(slot, permanent(i)?);
                }
            }
            arrange_library(t, s, p, side, seed)?;
        }
        Ok(())
    }

    /// Where `replayed`, the board a replay reached as this turn began in the game of `t`,
    /// first differs from this one, the record's, in words; none where they agree. Some
    /// differences are none:
    /// - permanents and exiled cards that differ only in where they stand among their like,
    ///   as two untapped Islands of one player do;
    /// - the order of a graveyard, where cards put there at once go in the order of their
    ///   slots;
    /// - whether a permanent that is no creature card has been under its controller's control
    ///   since their turn began, which restricts only creatures
    ///   ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)): "A creature's
    ///   activated ability with the tap symbol ... can't be activated unless the creature has
    ///   been under its controller's control continuously since their most recent turn
    ///   began", so which of two Islands, one played last turn, a payment taps follows their
    ///   slots;
    ///
    /// A rebuild restores each of these as recorded.
    pub fn difference(&self, t: &GameTable, replayed: &Checkpoint) -> Option<String> {
        first_difference("", &self.canonical(t), &replayed.canonical(t))
    }

    /// This board as [`Checkpoint::difference`] compares it: without noncreature cards'
    /// summoning sickness, with its graveyards in name order and its
    /// permanents and exiled cards each in one line and in name order, naming the permanents
    /// they are linked to rather than counting to them.
    fn canonical(&self, t: &GameTable) -> serde_json::Value {
        let named: Vec<String> = self
            .battlefield
            .iter()
            .map(|b| format!("{} of player {}", b.card, b.controller))
            .collect();
        let name = |i: &mut serde_json::Value| {
            if let Some(n) = i.as_u64() {
                let n = usize::try_from(n).expect("an index fits usize");
                *i = named
                    .get(n)
                    .map_or(serde_json::Value::Null, |n| n.as_str().into());
            }
        };
        let lines = |values: Vec<serde_json::Value>, links: &[&str]| {
            let mut lines: Vec<String> = values
                .into_iter()
                .map(|mut v| {
                    for &link in links {
                        if let Some(i) = v.get_mut(link) {
                            name(i);
                        }
                    }
                    v.to_string()
                })
                .collect();
            lines.sort();
            lines
        };
        let mut value = to_value(self);
        let battlefield = self
            .battlefield
            .iter()
            .map(|b| {
                let creature = game_index(t, &b.card).map_or(true, |card| {
                    t.def(card)
                        .face(b.face.ix())
                        .types
                        .contains(Types::CREATURE)
                });
                let mut v = to_value(b);
                if !creature {
                    let fields = v.as_object_mut().expect("a permanent is an object");
                    fields.remove("sick");
                }
                v
            })
            .collect();
        value["battlefield"] = lines(battlefield, &["attachedTo"]).into();
        for (i, side) in self.players.iter().enumerate() {
            let exile = side.exile.iter().map(to_value).collect();
            let mut graveyard = side.graveyard.clone();
            graveyard.sort();
            let v = &mut value["players"][i];
            v["exile"] = lines(exile, &["with", "until"]).into();
            v["graveyard"] = graveyard.into();
        }
        value
    }
}

fn to_value(value: &impl Serialize) -> serde_json::Value {
    serde_json::to_value(value).expect("a checkpoint serializes")
}

/// The name of the card in `slot`.
fn name<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot) -> String {
    s.definition_name(t, slot).to_owned()
}

/// Player `p`'s cards off the battlefield and their life; `place` finds a permanent's place
/// on the battlefield.
fn side<L: Layout>(
    t: &GameTable,
    s: &State<L>,
    p: Seat,
    place: &impl Fn(L::Slot) -> Option<u16>,
) -> Side {
    let hand = s.zone(ZoneKey::Of(p, Zone::Hand));
    // The candidate order three times, each sorted next: a checkpoint names cards, not
    // slots.
    let names = |cards| -> Vec<String> {
        let cards = s.in_candidate_order(cards);
        cards.map(|slot| name(t, s, slot)).collect()
    };
    let mut shown = names(hand.filter(|slot| s.knows(p.opponent(), slot)));
    shown.sort();
    let mut hand = names(hand);
    hand.sort();
    let mut exile: Vec<Exiled> = s
        .in_candidate_order(s.zone(ZoneKey::Of(p, Zone::Exile)))
        .map(|slot| {
            let o = s.object(slot);
            let link = |flag| {
                o.flags()
                    .has(flag)
                    .then_some(o.linked())
                    .flatten()
                    .and_then(place)
            };
            Exiled {
                card: name(t, s, slot),
                with: link(ObjectFlags::EXILED_WITH),
                until: link(ObjectFlags::EXILED_UNTIL),
            }
        })
        .collect();
    exile.sort();
    // A library's top is its last slot.
    let library = s.library(p);
    let known = library
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(depth, &slot)| {
            let by = knowers(s, slot);
            (!by.is_empty()).then(|| Known {
                depth: u16::try_from(depth).expect("a library fits u16"),
                card: name(t, s, slot),
                by,
            })
        })
        .collect();
    Side {
        life: s.player(p).life(),
        hand,
        shown,
        graveyard: s
            .graveyard(p)
            .iter()
            .rev()
            .map(|&slot| name(t, s, slot))
            .collect(),
        exile,
        library: u16::try_from(library.len()).expect("a library fits u16"),
        known,
        emblems: s
            .player(p)
            .emblems()
            .iter()
            .map(|&card| t.def(card).name.to_owned())
            .collect(),
    }
}

/// The permanent in `slot`; `place` finds a permanent's place on the battlefield.
fn permanent<L: Layout>(
    t: &GameTable,
    s: &State<L>,
    slot: L::Slot,
    place: &impl Fn(L::Slot) -> Option<u16>,
) -> Permanent {
    let o = s.object(slot);
    let face = Face::of(o.face());
    Permanent {
        card: name(t, s, slot),
        owner: o.owner().get(),
        controller: s.base_controller(slot).get(),
        face,
        token: o.flags().has(ObjectFlags::TOKEN),
        one_one: o.flags().has(ObjectFlags::ONE_ONE),
        tapped: o.flags().has(ObjectFlags::TAPPED),
        sick: o.flags().has(ObjectFlags::SUMMONING_SICK),
        counters: Counter::on(o),
        damage: o.damage(),
        attached_to: o.attached_to().and_then(place),
        impending: s.paid(slot, CastingKind::Impending),
        known: if face == Face::Down {
            knowers(s, slot)
        } else {
            Vec::new()
        },
    }
}

/// Puts the cards left in `p`'s library in order, top first: those some player knows where
/// `side` says, the rest as `seed` shuffles them.
fn arrange_library(
    t: &GameTable,
    s: &mut State<Small>,
    p: Seat,
    side: &Side,
    seed: u64,
) -> Result<(), String> {
    let mut rest = s.library(p).to_vec();
    if rest.len() != usize::from(side.library) {
        return Err(format!(
            "player {}'s library holds {} cards, not {}",
            p.get(),
            rest.len(),
            side.library
        ));
    }
    let mut order = vec![None; rest.len()];
    for k in &side.known {
        let i = rest
            .iter()
            .position(|&slot| s.definition_name(t, slot) == k.card)
            .ok_or_else(|| format!("player {}'s library has no {}", p.get(), k.card))?;
        let slot = rest.swap_remove(i);
        let at = order
            .get_mut(usize::from(k.depth))
            .ok_or_else(|| format!("no depth {} in player {}'s library", k.depth, p.get()))?;
        if at.replace(slot).is_some() {
            return Err(format!("two cards at depth {}", k.depth));
        }
        for q in Seat::BOTH {
            s.board().set_known(q, slot, k.by.contains(&q.get()));
        }
    }
    rest.sort_unstable();
    let mut shuffle = SplitMix64::new(seed ^ u64::from(p.get()));
    shuffle.shuffle(&mut rest);
    for slot in &rest {
        for q in Seat::BOTH {
            s.board().set_known(q, *slot, false);
        }
    }
    let mut rest = rest.into_iter();
    let order: Vec<_> = order
        .into_iter()
        .map(|slot| {
            slot.or_else(|| rest.next())
                .expect("as many slots as places")
        })
        .collect();
    s.board().order_library(p, &order);
    Ok(())
}

/// A slot of `player`'s library holding the card `name`.
fn from_library(
    t: &GameTable,
    s: &State<Small>,
    player: Seat,
    name: &str,
) -> Result<beryl_engine::Slot, String> {
    s.library(player)
        .iter()
        .copied()
        .find(|&slot| s.definition_name(t, slot) == name)
        .ok_or_else(|| format!("player {}'s deck has no {name} left", player.get()))
}

/// The game index of the card or token `name`.
fn game_index(t: &GameTable, name: &str) -> Result<CardIx, String> {
    t.index_named(name)
        .ok_or_else(|| format!("the game has no {name}"))
}

/// Where `b` first differs from `a`, at `path`: the first leaf that differs, or for lists of
/// different lengths, what only one of them has.
fn first_difference(path: &str, a: &serde_json::Value, b: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter().find_map(|k| {
                let null = Value::Null;
                first_difference(
                    &format!("{path}.{k}"),
                    x.get(k).unwrap_or(&null),
                    y.get(k).unwrap_or(&null),
                )
            })
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (a, b))| first_difference(&format!("{path}[{i}]"), a, b)),
        (Value::Array(x), Value::Array(y)) => {
            let only = |x: &[Value], y: &[Value]| {
                let mut y = y.to_vec();
                let mut only = Vec::new();
                for v in x {
                    match y.iter().position(|w| w == v) {
                        Some(i) => {
                            y.swap_remove(i);
                        }
                        None => only.push(v.to_string()),
                    }
                }
                only.join(", ")
            };
            Some(format!(
                "{path}: only the record has [{}], only the replay [{}]",
                only(x, y),
                only(y, x)
            ))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: the record has {a}, the replay {b}")),
    }
}
