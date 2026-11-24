//! Finding cards that share names in a battlefield row.

use std::cell::RefCell;

use beryl_protocol::protocol::ObjectView;

/// A number for each card name, kept by slot for every object of a table, so that a row finds the
/// cards that share a name by their numbers, without comparing or hashing the names. The table a
/// script plays over keeps one for the whole script and numbers again only the objects each step
/// names, the only objects a step changes.
pub struct NameIds<'a> {
    /// By slot, the number of the object's name; `u32::MAX` for a slot that holds no object.
    by_slot: Vec<u32>,
    /// The names of the objects it was made from, borrowed from them, each with its hash
    /// (`name_hash`) and number, sorted by hash and then by name: sorting by a hash compares
    /// one number where sorting by the names compares strings.
    names: Vec<(u64, Option<&'a str>, u32)>,
    /// Names first seen when an object was numbered again, each with its number.
    later: Vec<(Option<String>, u32)>,
    /// Per-name scratch used to find the preceding card in a row.
    seen: RefCell<Seen>,
}

impl<'a> NameIds<'a> {
    /// Assigns one number to each distinct name in `objects`.
    pub fn of(objects: &'a [ObjectView]) -> Self {
        let mut sorted: Vec<(u64, &ObjectView)> = objects
            .iter()
            .map(|object| (name_hash(object.name.as_deref()), object))
            .collect();
        // Names are compared only where two hashes are equal, which equal names' are.
        sorted.sort_unstable_by(|(a_hash, a), (b_hash, b)| {
            a_hash.cmp(b_hash).then_with(|| a.name.cmp(&b.name))
        });
        let end = objects
            .iter()
            .map(|object| usize::from(object.slot.0) + 1)
            .max()
            .unwrap_or(0);
        let mut ids = Self {
            by_slot: vec![u32::MAX; end],
            names: Vec::new(),
            later: Vec::new(),
            seen: RefCell::default(),
        };
        for run in sorted.chunk_by(|(a_hash, a), (b_hash, b)| a_hash == b_hash && a.name == b.name)
        {
            let id = ids.next_id();
            ids.names.push((run[0].0, run[0].1.name.as_deref(), id));
            for (_, object) in run {
                ids.by_slot[usize::from(object.slot.0)] = id;
            }
        }
        ids
    }

    /// The number a name not yet numbered takes.
    fn next_id(&self) -> u32 {
        u32::try_from(self.names.len() + self.later.len()).expect("fewer than 2^32 names")
    }

    /// Numbers `object`'s name again, as it now stands.
    pub fn note(&mut self, object: &ObjectView) {
        let name = object.name.as_deref();
        if let Some(id) = self.get(object) {
            let known = if let Some((_, name, _)) = self.names.get(id) {
                *name
            } else {
                self.later[id - self.names.len()].0.as_deref()
            };
            if known == name {
                return;
            }
        }
        let hash = name_hash(name);
        let found = self
            .names
            .binary_search_by(|(other_hash, other, _)| (other_hash, other).cmp(&(&hash, &name)));
        let known = match found {
            Ok(at) => Some(self.names[at].2),
            Err(_) => self
                .later
                .iter()
                .find(|(other, _)| other.as_deref() == name)
                .map(|(_, id)| *id),
        };
        let id = known.unwrap_or_else(|| {
            let id = self.next_id();
            self.later.push((object.name.clone(), id));
            id
        });
        let slot = usize::from(object.slot.0);
        if self.by_slot.len() <= slot {
            self.by_slot.resize(slot + 1, u32::MAX);
        }
        self.by_slot[slot] = id;
    }

    /// The number of `object`'s name, if it has one.
    fn get(&self, object: &ObjectView) -> Option<usize> {
        let id = *self.by_slot.get(usize::from(object.slot.0))?;
        (id != u32::MAX).then_some(id as usize)
    }

    /// How many names it has numbered.
    fn count(&self) -> usize {
        self.names.len() + self.later.len()
    }
}

#[derive(Default)]
struct Seen {
    row: usize,
    /// The pass that last visited this name and its position in that row.
    last: Vec<(usize, usize)>,
}

/// For each card of a row, the nearest card before it with the same name. With `NameIds`, a card
/// looks up the last card seen with its name's number. Without, the cards are sorted by a hash
/// of their names, which compares as one number where sorting by the names compares strings,
/// and only cards whose hashes are equal have their names compared.
pub struct SameName {
    /// By index in the row: the index of the last earlier card with its name, if any.
    previous: Vec<Option<usize>>,
}

impl SameName {
    pub(in crate::table) fn of(cards: &[&ObjectView], ids: Option<&NameIds<'_>>) -> Self {
        if let Some(ids) = ids {
            let mut seen = ids.seen.borrow_mut();
            let Seen { row, last } = &mut *seen;
            *row = row.wrapping_add(1);
            if *row == 0 {
                last.clear();
                *row = 1;
            }
            last.resize(ids.count(), (0, 0));
            let mut previous = Vec::new();
            let mut missing = false;
            for (index, card) in cards.iter().enumerate() {
                let Some(id) = ids.get(card) else {
                    missing = true;
                    break;
                };
                let (pass, before) = std::mem::replace(&mut last[id], (*row, index));
                if pass == *row {
                    if previous.is_empty() {
                        previous.resize(cards.len(), None);
                    }
                    previous[index] = Some(before);
                }
            }
            if !missing {
                return Self { previous };
            }
        }
        let mut order: Vec<(u64, usize)> = cards
            .iter()
            .enumerate()
            .map(|(index, card)| (name_hash(card.name.as_deref()), index))
            .collect();
        order.sort_unstable();
        let mut previous = vec![None; cards.len()];
        for run in order.chunk_by(|a, b| a.0 == b.0) {
            for (at, &(_, index)) in run.iter().enumerate() {
                // Two names may share a hash: the nearest earlier card in the run whose name is
                // equal. The run is in the order of the row.
                previous[index] = run[..at]
                    .iter()
                    .rev()
                    .map(|&(_, other)| other)
                    .find(|&other| cards[other].name == cards[index].name);
            }
        }
        Self { previous }
    }

    /// The links of a row given them already, by index.
    pub(in crate::table) fn from_previous(previous: Vec<Option<usize>>) -> Self {
        Self { previous }
    }

    /// Whether any two cards share a name.
    pub(in crate::table) fn repeats(&self) -> bool {
        self.previous.iter().any(Option::is_some)
    }

    /// The last card before the one at `index` with the same name.
    pub(in crate::table) fn before(&self, index: usize) -> Option<usize> {
        self.previous.get(index).copied().flatten()
    }

    /// The first card with the same name as the one at `index`, given the answers for every
    /// card before it.
    pub(in crate::table) fn first(&self, index: usize, firsts: &[usize]) -> usize {
        self.before(index)
            .map_or(index, |previous| firsts[previous])
    }
}

/// A hash of a card's name (`faces::name_hash`), or of its having none.
fn name_hash(name: Option<&str>) -> u64 {
    // What `faces::name_hash` mixes in first, the length, is never `u64::MAX`.
    name.map_or(
        u64::MAX.wrapping_mul(0x51_7c_c1_b7_27_22_0a_95),
        crate::faces::name_hash,
    )
}
