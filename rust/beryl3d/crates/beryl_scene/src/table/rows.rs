//! One battlefield row at rest (docs/beryl-3d.md, "Objects and placement"). A row lays its
//! cards out at full size while they fit its width, then overlaps them down to the layout's
//! least uncovered strip, then stacks identical permanents into piles, and when even that is too
//! long, shows as many as fit from a scroll offset and leaves the rest undrawn.

use std::f32::consts::FRAC_PI_2;

use beryl_protocol::protocol::{Facing, ObjectView};
use glam::{Quat, Vec3};

use super::{CARD_HEIGHT, CARD_WIDTH, LAYER, Layout, RowArea};
use crate::pose::Pose;

/// One card of a laid-out row: its pose, and whether it is drawn.
pub(super) struct Laid<'a> {
    pub object: &'a ObjectView,
    pub pose: Pose,
    pub visible: bool,
}

/// How a row is laid out: the least strip in millimetres, the layout's spacing, and the row's
/// scroll offset in groups.
pub(super) struct RowRules<'a> {
    pub layout: &'a Layout,
    pub strip: f32,
    pub scroll: usize,
}

/// A row's cards gathered into groups laid out side by side.
enum Groups<'a, 'b> {
    /// Each card a group of its own.
    Single(&'b [&'a ObjectView]),
    /// Each group's cards a run of `cards`, its top card first; `ends` holds where each run
    /// ends.
    Piles {
        cards: Vec<&'a ObjectView>,
        ends: Vec<usize>,
    },
}

impl<'a> Groups<'a, '_> {
    fn len(&self) -> usize {
        match self {
            Groups::Single(cards) => cards.len(),
            Groups::Piles { ends, .. } => ends.len(),
        }
    }

    /// The groups, in order.
    fn iter(&self) -> impl Iterator<Item = &[&'a ObjectView]> {
        (0..self.len()).map(|index| match self {
            Groups::Single(cards) => &cards[index..=index],
            Groups::Piles { cards, ends } => {
                let start = index.checked_sub(1).map_or(0, |before| ends[before]);
                &cards[start..ends[index]]
            }
        })
    }
}

/// Lays out `cards`, in order from left to right, in `area` at height `y` on the table. `names`
/// links each card to the one before it with its name, if the caller has them already, and
/// `ids` numbers the cards' names, if the caller keeps numbers. Each card goes to `laid` as it is
/// laid out.
pub(super) fn lay_row<'a>(
    cards: &[&'a ObjectView],
    (names, ids): (Option<SameName>, Option<&NameIds<'_>>),
    area: RowArea,
    y: f32,
    rules: &RowRules,
    laid: &mut impl FnMut(Laid<'a>),
) {
    let room = area.right - area.left;
    let groups = if fits_overlapped(cards, room, rules) {
        Groups::Single(cards)
    } else {
        piles(cards, &names.unwrap_or_else(|| SameName::of(cards, ids)))
    };
    let widths: Vec<f32> = groups.iter().map(|g| group_width(g, rules)).collect();
    let full: f32 = widths.iter().sum::<f32>() + rules.layout.gap * gaps(groups.len());
    if full <= room {
        let mut left = area.left + (room - full) / 2.0;
        let mut height = 0;
        for (group, width) in groups.iter().zip(&widths) {
            place_group(group, left, y, &mut height, true, rules, laid);
            left += width + rules.layout.gap;
        }
        return;
    }
    // Every window of `shown` consecutive groups fits at the least strip, however wide its
    // last group is.
    let widest = widths.iter().copied().fold(0.0, f32::max);
    let shown = ((((room - widest) / rules.strip).floor() as usize) + 1).clamp(1, groups.len());
    let first = rules.scroll.min(groups.len() - shown);
    let last_width = widths[first + shown - 1];
    let step = if shown > 1 {
        (room - last_width) / gaps(shown)
    } else {
        0.0
    };
    let mut height = 0;
    for (index, group) in groups.iter().enumerate() {
        let visible = (first..first + shown).contains(&index);
        // A group scrolled out of view rests at the nearer end of the row, undrawn.
        let place = index.clamp(first, first + shown - 1) - first;
        let left = area.left + step * place as f32;
        place_group(group, left, y, &mut height, visible, rules, laid);
    }
}

/// Whether every card, alone, fits the row with at least the least strip of each uncovered.
fn fits_overlapped(cards: &[&ObjectView], room: f32, rules: &RowRules) -> bool {
    let Some(&last) = cards.last() else {
        return true;
    };
    gaps(cards.len()) * rules.strip + card_width(last) <= room
}

/// The number of spaces between `count` things in a line.
fn gaps(count: usize) -> f32 {
    count.saturating_sub(1) as f32
}

/// Puts a group's cards at `left`, the copies beneath peeking out to the right and away from
/// the viewer. Every card lies one `LAYER` above the card placed before it, whether or not the
/// row overlaps: a tapped card is wider than an untapped row's step, and two cards at one height
/// would flicker where they overlap.
fn place_group<'a>(
    group: &[&'a ObjectView],
    left: f32,
    y: f32,
    height: &mut u32,
    visible: bool,
    rules: &RowRules,
    laid: &mut impl FnMut(Laid<'a>),
) {
    // The top card is the group's first; the copies beneath are placed first, lowest.
    for (depth, &object) in group.iter().enumerate().rev() {
        let offset = rules.layout.pile_step * depth as f32;
        let centre = left + card_width(object) / 2.0 + offset.x;
        let rotation = if object.tapped {
            Quat::from_rotation_z(-FRAC_PI_2)
        } else {
            Quat::IDENTITY
        };
        let pose = Pose {
            position: Vec3::new(centre, y + offset.y, LAYER * *height as f32),
            rotation,
            turn: if face_up(object) { 0.0 } else { 1.0 },
            ..Pose::default()
        };
        *height += 1;
        laid(Laid {
            object,
            pose,
            visible,
        });
    }
}

/// How wide a card lies: a tapped card is turned a quarter turn.
fn card_width(object: &ObjectView) -> f32 {
    if object.tapped {
        CARD_HEIGHT
    } else {
        CARD_WIDTH
    }
}

/// How wide a group lies: its top card and the copies peeking out beside it.
fn group_width(group: &[&ObjectView], rules: &RowRules) -> f32 {
    card_width(group[0]) + rules.layout.pile_step.x * gaps(group.len())
}

/// Whether the viewer sees a card's face: a face-up card whose name they know.
pub(super) fn face_up(object: &ObjectView) -> bool {
    object.facing == Facing::Up && object.name.is_some()
}

/// `cards` with identical permanents gathered into piles, each pile where its first card was.
/// The pile's first card is the one with the lowest slot, which lies on top: the engine offers
/// the lowest slot of interchangeable objects, so what a choice takes leaves from the top.
///
/// Only cards with the same name can be twins, so each card is compared only with the piles of
/// its own name, found through `names`. A row of a thousand different names would otherwise
/// compare every card with every pile.
fn piles<'a, 'b>(cards: &'b [&'a ObjectView], names: &SameName) -> Groups<'a, 'b> {
    if !names.repeats() {
        return Groups::Single(cards);
    }
    // The index in `cards` of the first card of each card's pile.
    let mut tops: Vec<usize> = (0..cards.len()).collect();
    for (index, &card) in cards.iter().enumerate() {
        if !plain(card) {
            continue;
        }
        // Twins is an equivalence: a card that twins any card of a pile twins its first card,
        // and of the piles of one name at most one takes it.
        let mut earlier = names.before(index);
        while let Some(other) = earlier {
            if plain(cards[other]) && twins(cards[other], card) {
                tops[index] = tops[other];
                break;
            }
            earlier = names.before(other);
        }
    }
    // Each pile's cards as one run, the piles in the order of their first cards: count each
    // pile's cards, then put each card at its pile's next free place.
    let mut ends: Vec<usize> = Vec::with_capacity(cards.len());
    let mut pile_of = vec![0; cards.len()];
    for index in 0..cards.len() {
        if tops[index] == index {
            pile_of[index] = ends.len();
            ends.push(0);
        }
        ends[pile_of[tops[index]]] += 1;
    }
    let mut total = 0;
    for end in &mut ends {
        total += *end;
        *end = total;
    }
    let mut next: Vec<usize> = std::iter::once(0).chain(ends.iter().copied()).collect();
    let mut gathered = cards.to_vec();
    for (index, &card) in cards.iter().enumerate() {
        let pile = pile_of[tops[index]];
        gathered[next[pile]] = card;
        next[pile] += 1;
    }
    let mut start = 0;
    for &end in &ends {
        if end - start > 1 {
            gathered[start..end].sort_by_key(|object| object.slot.0);
        }
        start = end;
    }
    Groups::Piles {
        cards: gathered,
        ends,
    }
}

/// A number for each card name, kept by slot for every object of a table, so that a row finds the
/// cards that share a name by their numbers, without comparing or hashing the names. The table a
/// script plays over keeps one for the whole script and numbers again only the objects each step
/// names, the only objects a step changes.
pub(crate) struct NameIds<'a> {
    /// By slot, the number of the object's name; `u32::MAX` for a slot that holds no object.
    by_slot: Vec<u32>,
    /// The names of the objects it was made from, borrowed from them, sorted, each with its
    /// number.
    names: Vec<(Option<&'a str>, u32)>,
    /// Names first seen when an object was numbered again, each with its number.
    later: Vec<(Option<String>, u32)>,
}

impl<'a> NameIds<'a> {
    pub(crate) fn of(objects: &'a [ObjectView]) -> Self {
        let mut sorted: Vec<&ObjectView> = objects.iter().collect();
        sorted.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        let end = sorted
            .iter()
            .map(|object| usize::from(object.slot.0) + 1)
            .max()
            .unwrap_or(0);
        let mut ids = Self {
            by_slot: vec![u32::MAX; end],
            names: Vec::new(),
            later: Vec::new(),
        };
        for run in sorted.chunk_by(|a, b| a.name == b.name) {
            let id = ids.next_id();
            ids.names.push((run[0].name.as_deref(), id));
            for object in run {
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
    pub(crate) fn note(&mut self, object: &ObjectView) {
        let name = object.name.as_deref();
        let known = match self.names.binary_search_by(|(other, _)| other.cmp(&name)) {
            Ok(at) => Some(self.names[at].1),
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

/// For each card of a row, the nearest card before it with the same name. With `NameIds`, a card
/// looks up the last card seen with its name's number. Without, the cards are sorted by a hash
/// of their names, which compares as one number where sorting by the names compares strings,
/// and only cards whose hashes are equal have their names compared.
pub(super) struct SameName {
    /// By index in the row: the index of the last earlier card with its name, if any.
    previous: Vec<Option<usize>>,
}

impl SameName {
    pub(super) fn of(cards: &[&ObjectView], ids: Option<&NameIds<'_>>) -> Self {
        if let Some(ids) = ids
            && let Some(numbered) = cards
                .iter()
                .map(|card| ids.get(card))
                .collect::<Option<Vec<usize>>>()
        {
            // By name number, the index of the last card seen with that name, or `u32::MAX`.
            let mut last = vec![u32::MAX; ids.count()];
            let previous = numbered
                .iter()
                .enumerate()
                .map(|(index, &id)| {
                    let before = std::mem::replace(
                        &mut last[id],
                        u32::try_from(index).expect("a row holds fewer than 2^32 cards"),
                    );
                    (before != u32::MAX).then_some(before as usize)
                })
                .collect();
            return Self { previous };
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
    pub(super) fn from_previous(previous: Vec<Option<usize>>) -> Self {
        Self { previous }
    }

    /// Whether any two cards share a name.
    fn repeats(&self) -> bool {
        self.previous.iter().any(Option::is_some)
    }

    /// The last card before the one at `index` with the same name.
    pub(super) fn before(&self, index: usize) -> Option<usize> {
        self.previous[index]
    }

    /// The first card with the same name as the one at `index`, given the answers for every
    /// card before it.
    pub(super) fn first(&self, index: usize, firsts: &[usize]) -> usize {
        self.previous[index].map_or(index, |previous| firsts[previous])
    }
}

/// A hash of a card's name, or of its having none, in the manner of `FxHash`, which the Rust
/// compiler uses for its own tables: it mixes in eight bytes at a time with a rotation and a
/// multiplication. Equal names hash alike; different names rarely do.
fn name_hash(name: Option<&str>) -> u64 {
    let mut hash: u64 = 0;
    let mut mix = |word: u64| {
        hash = (hash.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    };
    let Some(name) = name else {
        mix(u64::MAX);
        return hash;
    };
    mix(name.len() as u64);
    let (words, rest) = name.as_bytes().as_chunks::<8>();
    for &word in words {
        mix(u64::from_le_bytes(word));
    }
    if !rest.is_empty() {
        let mut word = [0; 8];
        word[..rest.len()].copy_from_slice(rest);
        mix(u64::from_le_bytes(word));
    }
    hash
}

/// Whether a permanent may share a pile: nothing on it or about it that a pile would hide.
fn plain(object: &ObjectView) -> bool {
    face_up(object)
        && object.note.is_none()
        && !object.doesnt_untap
        && object.named_counters.is_empty()
        && object.counters.is_empty()
        && object.damage == 0
        && object.combat.is_none()
        && object.attached_to.is_none()
}

/// Whether two permanents are alike in everything the table shows of them.
fn twins(a: &ObjectView, b: &ObjectView) -> bool {
    a.slot != b.slot
        && a.name.is_some()
        && a.name == b.name
        && a.facing == b.facing
        && a.owner == b.owner
        && a.controller == b.controller
        && a.token == b.token
        && a.tapped == b.tapped
        && a.summoning_sick == b.summoning_sick
        && a.types == b.types
        && a.pt == b.pt
}
