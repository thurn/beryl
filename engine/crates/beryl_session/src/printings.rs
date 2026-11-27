//! Which Secret Lair printing each visible card of a printed deck shows
//! ([`ObjectView::printing`](crate::protocol::ObjectView)).
//!
//! The engine cannot say which physical copy a card is: a shuffle deals identities anew among
//! a library's slots, and search needs two copies of a card to stay interchangeable. So the
//! session decides the art, and the engine never sees it. At the start of a game whose deck
//! has printings (`data/deck-printings.csv`), each card name gets a queue of its printings,
//! each repeated by its copies, in an order drawn from a stream seeded by the game's seed and
//! apart from the game's own. A slot takes the next printing of its card's name once some
//! player knows its identity, and gives it back to the end of the queue once no player does,
//! which is when a shuffle or a random order has dealt the slot's identity anew. A card that
//! stays known keeps its printing from zone to zone, so a Dandân shows the same art from hand
//! to stack to battlefield to graveyard, and no more copies of an art are visible at once
//! than the deck has.
//!
//! The session notes the assignment after each step the engine plays ([`Printings::note`]),
//! which a replay of the game takes too, so a replay assigns the same printings. Undo puts
//! the assignment back with the state.

use std::collections::{BTreeMap, VecDeque};

use beryl_card_names::decks;
use beryl_card_records::{PRINTINGS, PrintingIx};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{GameTable, Layout, Seat, SlotId, State};

use crate::protocol::{ObjectId, PlayerView};

/// Mixed into the game's seed for the stream that orders the queues, so that it draws apart
/// from the agent's stream (`SplitMix64::new(seed)`) and from the game's own.
const STREAM: u64 = 0x5052_494E_5449_4E47;

/// The printings of one card name in one pile of cards.
#[derive(Clone, Debug, Default)]
struct Name {
    /// The printings no slot shows, the next one first. A name whose copies all show one
    /// printing has none here: every copy shows [`Name::only`].
    queue: VecDeque<PrintingIx>,
    /// The printing of every copy, where there is just one.
    only: Option<PrintingIx>,
}

/// The printings of a game's visible cards: empty in a game whose decks have none, which
/// then pays nothing for them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Printings {
    /// By pile (0 in a shared-deck game, else the owner's seat) and card name.
    names: BTreeMap<(u8, &'static str), Name>,
    /// The printing each slot shows, by its protocol number, with the key of its name.
    held: BTreeMap<u16, ((u8, &'static str), PrintingIx)>,
}

impl Printings {
    /// The queues of a game dealt from `seed` with `decks`, each seat's decklist; in a
    /// shared-deck game `decks[0]` is the shared list and `decks[1]` is empty.
    pub(crate) fn dealt(seed: u64, decks: &[Vec<(u8, String)>; 2]) -> Printings {
        let mut printings = Printings::default();
        let mut rng = SplitMix64::new(seed ^ STREAM);
        for (pile, list) in (0u8..).zip(decks) {
            let Some(deck) = printed_deck(list) else {
                continue;
            };
            for (i, p) in PRINTINGS.iter().enumerate() {
                if p.deck != deck {
                    continue;
                }
                let ix = PrintingIx(u32::try_from(i).expect("the printings fit in u32"));
                let name = printings.names.entry((pile, p.name)).or_default();
                name.queue
                    .extend(std::iter::repeat_n(ix, p.copies as usize));
            }
        }
        for name in printings.names.values_mut() {
            let first = name.queue.front().copied();
            if name.queue.iter().all(|&p| Some(p) == first) {
                name.only = first;
                name.queue.clear();
            } else {
                rng.shuffle(name.queue.make_contiguous());
            }
        }
        printings
    }

    /// The queues of a prepared shared-deck game, cycling one of each printing of each card
    /// before repeating so that small scenario boards show every art of a multi-art card.
    pub(crate) fn shared_deck() -> Printings {
        let mut by_name: BTreeMap<&'static str, Vec<(PrintingIx, u32)>> = BTreeMap::new();
        for (i, p) in PRINTINGS.iter().enumerate() {
            if p.deck != "forgetful-fish" {
                continue;
            }
            let ix = PrintingIx(u32::try_from(i).expect("the printings fit in u32"));
            by_name.entry(p.name).or_default().push((ix, p.copies));
        }
        let mut printings = Printings::default();
        for (card, entries) in by_name {
            let name = printings.names.entry((0, card)).or_default();
            if entries.len() == 1 {
                name.only = Some(entries[0].0);
            } else {
                let max = entries.iter().map(|&(_, c)| c).max().unwrap_or(0);
                for round in 0..max {
                    for &(ix, copies) in &entries {
                        if round < copies {
                            name.queue.push_back(ix);
                        }
                    }
                }
            }
        }
        printings
    }

    /// Whether the game shows any printing at all.
    fn any(&self) -> bool {
        !self.names.is_empty()
    }

    /// Takes the printings of the cards no player knows any more back to their queues, then
    /// gives each card some player knows and that shows none the next printing of its name:
    /// in `s`, the position the players see.
    pub(crate) fn note<L: Layout>(&mut self, t: &GameTable, s: &State<L>) {
        if !self.any() {
            return;
        }
        let seats = [Seat::new(0), Seat::new(1)];
        let key = |slot: L::Slot| {
            let o = s.object(slot);
            let pile = if s.shares_zones() {
                0
            } else {
                o.owner().index() as u8
            };
            (!o.is_token()).then(|| (pile, s.definition_name(t, slot)))
        };
        let known = |slot: L::Slot| seats.iter().any(|&p| s.knows(p, slot));
        let gone: Vec<u16> = self
            .held
            .iter()
            .filter(|&(&num, &(held, _))| {
                L::Slot::from_num(num).is_none_or(|slot| !known(slot) || key(slot) != Some(held))
            })
            .map(|(&num, _)| num)
            .collect();
        for num in gone {
            let (name, printing) = self.held.remove(&num).expect("a held slot");
            if let Some(name) = self.names.get_mut(&name)
                && name.only.is_none()
            {
                name.queue.push_back(printing);
            }
        }
        for slot in L::Slot::all() {
            if self.held.contains_key(&slot.num()) || !known(slot) {
                continue;
            }
            let Some(k) = key(slot) else {
                continue;
            };
            let Some(name) = self.names.get_mut(&k) else {
                continue;
            };
            if let Some(printing) = name.only.or_else(|| name.queue.pop_front()) {
                self.held.insert(slot.num(), (k, printing));
            }
        }
    }

    /// Exchanges the printings held by `first` and `second`, when a player chooses a twin of
    /// the card the engine offered: the played slot then shows the art the player dragged or
    /// clicked, and the twin left behind keeps the other's.
    pub(crate) fn trade(&mut self, first: ObjectId, second: ObjectId) {
        if first == second || !self.any() {
            return;
        }
        let a = self.held.remove(&first.0);
        let b = self.held.remove(&second.0);
        if let Some(entry) = b {
            self.held.insert(first.0, entry);
        }
        if let Some(entry) = a {
            self.held.insert(second.0, entry);
        }
    }

    /// The printing the object `id` shows, if it shows one of its name's printings.
    pub(crate) fn of(&self, id: ObjectId) -> Option<PrintingIx> {
        self.held.get(&id.0).map(|&(_, printing)| printing)
    }

    /// Sets the printing of each object in `view` whose card its viewer knows.
    pub(crate) fn show(&self, view: &mut PlayerView) {
        if self.any() {
            crate::protocol::show_printings(view, |id| self.of(id));
        }
    }
}

/// Every printing of the deck in `data/deck-printings.csv` whose list is one of `lists`, in the
/// file's order: the printings a game dealt from these decklists can show. Empty when no list
/// is a printed deck.
pub fn printings_of(lists: &[Vec<(u8, String)>]) -> Vec<PrintingIx> {
    let keys: Vec<_> = lists.iter().filter_map(|list| printed_deck(list)).collect();
    (0u32..)
        .zip(PRINTINGS.iter())
        .filter(|(_, p)| keys.contains(&p.deck))
        .map(|(i, _)| PrintingIx(i))
        .collect()
}

/// The key of the deck in `data/deck-printings.csv` whose list is `list`, if one is.
fn printed_deck(list: &[(u8, String)]) -> Option<&'static str> {
    if list.is_empty() {
        return None;
    }
    let mut ours: Vec<(u8, &str)> = list.iter().map(|(n, name)| (*n, name.as_str())).collect();
    ours.sort_unstable();
    PRINTINGS.iter().map(|p| p.deck).find(|&key| {
        decks::named(key).is_some_and(|deck| {
            let mut theirs = deck.list.to_vec();
            theirs.sort_unstable();
            theirs == ours
        })
    })
}
