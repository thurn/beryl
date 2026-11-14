use serde::{Deserialize, Serialize};

use crate::change::Edit;
use crate::log::Touches;
use crate::{Action, Chance, Change, Seat, Table, permissions};

mod cards;
mod game;
mod library;
mod players;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Permission,
    Stale,
    Invalid {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        blocking: Vec<u32>,
    },
}

impl Refusal {
    pub const INVALID: Self = Self::Invalid {
        blocking: Vec::new(),
    };
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub n: u32,
    pub by: Seat,
    pub action: Action,
    pub changes: Vec<Change>,
    pub undoes: Option<u32>,
    pub touches: Touches,
    pub seen: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) undone_action: Option<Action>,
    /// Display data captured before private changes are projected away.
    #[serde(default)]
    pub(crate) summary: crate::log::Summary,
}

type Changes = Result<Vec<Change>, Refusal>;

fn edit<T: PartialEq, C>(before: T, after: T, change: impl FnOnce(Edit<T>) -> C) -> Option<C> {
    (before != after).then(|| change(Edit { before, after }))
}

/// The reserved actions still have a dispatch target so they can be
/// implemented independently in the module that owns their state.
macro_rules! unimplemented_actions {
    ($($name:ident),+ $(,)?) => {
        $(pub(super) fn $name(_action: &Action) -> Changes {
            Err(crate::Refusal::INVALID)
        })+
    };
}

use unimplemented_actions;

/// Applies an ordinary action or batch without keeping a log. History actions
/// use [`crate::Log::apply`].
pub fn apply(
    table: &mut Table,
    by: Seat,
    action: &Action,
    chance: &mut Chance,
) -> Result<Entry, Refusal> {
    let n = next_entry(table)?;
    let mut planned_chance = chance.clone();
    let mut touches = Touches::default();
    let changes = apply_action(table, by, action, &mut planned_chance, &mut touches)?;
    *chance = planned_chance;
    Ok(commit(table, n, by, action, changes, touches))
}

pub(crate) fn next_entry(table: &Table) -> Result<u32, Refusal> {
    table.entry_number.checked_add(1).ok_or(Refusal::INVALID)
}

pub(crate) fn commit(
    table: &mut Table,
    n: u32,
    by: Seat,
    action: &Action,
    changes: Vec<Change>,
    touches: Touches,
) -> Entry {
    table.entry_number = n;
    let summary = crate::log::Summary::new(table, by, action, &changes);
    Entry {
        n,
        by,
        action: action.clone(),
        changes,
        undoes: None,
        touches,
        seen: false,
        undone_action: None,
        summary,
    }
}

fn apply_action(
    table: &mut Table,
    by: Seat,
    action: &Action,
    chance: &mut Chance,
    touches: &mut Touches,
) -> Changes {
    if let Action::Batch { actions } = action {
        permissions::check_actor(table, by)?;
        let mut staged = table.clone();
        let mut changes = Vec::new();
        for member in actions {
            changes.extend(apply_action(&mut staged, by, member, chance, touches)?);
        }
        *table = staged;
        return Ok(changes);
    }
    permissions::check(table, by, action)?;
    let mut changes = changes_for(table, by, action, chance)?;
    perform_with_top_knowledge(table, action, &mut changes, touches);
    Ok(changes)
}

pub(crate) fn perform_history(
    table: &mut Table,
    action: &Action,
    mut changes: Vec<Change>,
) -> (Vec<Change>, Touches) {
    let mut touches = Touches::default();
    perform_with_top_knowledge(table, action, &mut changes, &mut touches);
    (changes, touches)
}

fn perform_with_top_knowledge(
    table: &mut Table,
    action: &Action,
    changes: &mut Vec<Change>,
    touches: &mut Touches,
) {
    perform_tracking(table, changes, action, touches);
    let top_knowledge = library::keep_top_known(table);
    perform_tracking(table, &top_knowledge, action, touches);
    changes.extend(top_knowledge);
}

fn perform_tracking(table: &mut Table, changes: &[Change], action: &Action, touches: &mut Touches) {
    for (index, change) in changes.iter().enumerate() {
        touches.record(table, change);
        perform_one(table, action, changes, index, change);
    }
}

pub(crate) fn perform(table: &mut Table, changes: &[Change], action: &Action) {
    for (index, change) in changes.iter().enumerate() {
        perform_one(table, action, changes, index, change);
    }
}

fn perform_one(
    table: &mut Table,
    action: &Action,
    changes: &[Change],
    index: usize,
    change: &Change,
) {
    debug_assert!(
        valid_knowledge_change(table, action, changes, index),
        "knowledge may only shrink when a card enters a hand or is shuffled"
    );
    change.perform(table);
    if let Some(id) = change.introduced_id() {
        table.next_id = table.next_id.max(u64::from(id.0) + 1);
    }
}

fn valid_knowledge_change(
    table: &Table,
    action: &Action,
    changes: &[Change],
    index: usize,
) -> bool {
    use crate::state::Zone;

    let Change::ResetKnowledge {
        id,
        edit,
        forget: true,
    } = &changes[index]
    else {
        return true;
    };
    let object = &table.objects[id];
    if object.known_to.iter().all(|seat| edit.after.contains(seat)) {
        return true;
    }
    if !matches!(changes.get(index + 1), Some(Change::ReplaceId { from, .. }) if from == id) {
        return false;
    }
    match object.zone {
        Zone::Hand(owner) => {
            edit.after == [owner]
                && changes[..index]
                    .iter()
                    .rev()
                    .find_map(|change| match change {
                        Change::Move {
                            id: moved,
                            from,
                            to,
                        } if id == moved => Some(from.zone != to.zone && to.zone == object.zone),
                        _ => None,
                    })
                    == Some(true)
        }
        Zone::Library(owner) => {
            edit.after.is_empty()
                && matches!(
                    action,
                    Action::Shuffle { seat, .. } | Action::Mulligan { seat, .. } if *seat == owner
                )
        }
        _ => false,
    }
}

fn changes_for(table: &Table, by: Seat, action: &Action, chance: &mut Chance) -> Changes {
    match action {
        Action::Move {
            selection,
            to,
            position,
            face,
            tapped,
        } => cards::move_cards(table, selection, *to, *position, *face, *tapped),
        Action::Tap { selection, tapped } => cards::tap(table, selection, *tapped),
        Action::Reorder {
            selection,
            position,
        } => cards::reorder(table, selection, *position),
        Action::Pin { selection, row } => cards::pin(table, selection, *row),
        Action::Give { selection, to } => cards::give(table, selection, *to),
        Action::Face { selection, face } => cards::face(table, selection, *face),
        Action::Flag {
            selection,
            doesnt_untap,
        } => cards::flag(table, selection, *doesnt_untap),
        Action::Peek { selection } => cards::peek(table, by, selection),
        Action::Counter {
            selection,
            name,
            adjustment,
        } => cards::counter(table, selection, name, *adjustment),
        Action::Stats { selection, stats } => cards::stats(table, selection, *stats),
        Action::Damage { selection, damage } => cards::damage(table, selection, *damage),
        Action::Note { selection, note } => cards::note(table, selection, note.as_deref()),
        Action::Attach { selection, host } => cards::attach(table, selection, *host),
        Action::Unattach { selection } => cards::unattach(table, selection),
        Action::Clone { selection } => cards::clone(table, by, selection),
        Action::Token {
            card,
            count,
            attached_to,
            note,
        } => cards::token(table, by, card, *count, *attached_to, note.as_deref()),
        Action::Marker { selection, text } => cards::marker(table, selection, text),
        Action::Remove { selection } => cards::remove(table, selection),
        Action::Draw { seat, count } => library::draw(table, *seat, *count),
        Action::Mulligan { seat, count } => library::mulligan(table, *seat, *count, chance, action),
        Action::Shuffle { seat, portion } => library::shuffle(table, *seat, *portion, chance),
        Action::Look { seat, portion } => library::look(table, by, *seat, *portion),
        Action::Reveal {
            zone,
            objects,
            random,
            to,
        } => library::reveal(table, *zone, objects.as_deref(), *random, to, chance),
        Action::Lend { zone, to } => library::lend(table, *zone, *to),
        Action::AlwaysRevealTop { seat, on } => library::always_reveal_top(table, *seat, *on),
        Action::AlwaysLookAtTop { seat, on } => library::always_look_at_top(table, *seat, *on),
        Action::SortHand { seat, by } => library::sort_hand(table, *seat, *by),
        Action::Life { seat, adjustment } => players::life(table, *seat, *adjustment),
        Action::PlayerCounter {
            seat,
            name,
            adjustment,
        } => players::player_counter(table, *seat, name, *adjustment),
        Action::Phase { phase } => Ok(game::phase(table, *phase)),
        Action::NextTurn {} => game::next_turn(table),
        Action::Roll { count, sides } => players::roll(*count, *sides, chance),
        Action::Arrow { selection, to } => players::arrow(table, by, selection, *to),
        Action::ClearArrows { from, to } => players::clear_arrows(table, by, *from, *to),
        Action::LoadDeck { seat, main, side } => game::load_deck(table, *seat, main, side),
        Action::Sideboard {
            seat,
            into_main,
            into_side,
        } => game::sideboard(table, *seat, into_main, into_side),
        Action::Ready { seat, ready } => game::ready(table, *seat, *ready),
        Action::Start { first } => game::start(table, *first, chance, action),
        Action::NextGame { winner } => game::next_game(table, *winner),
        Action::Concede {} => game::concede(table, by),
        Action::SetAssist { .. } => game::set_assist(action),
        Action::Flow { .. } => library::flow(action),
        Action::Undo { .. } | Action::Rewind { .. } | Action::Batch { .. } => Err(Refusal::INVALID),
    }
}
