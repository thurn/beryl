use serde::{Deserialize, Serialize};

use crate::{Action, Chance, Change, Seat, Table, permissions};

mod cards;
mod game;
mod library;
mod players;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Permission,
    Stale,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub n: u32,
    pub by: Seat,
    pub action: Action,
    pub changes: Vec<Change>,
    pub undoes: Option<u32>,
}

type Changes = Result<Vec<Change>, Refusal>;

/// The reserved actions still have a dispatch target so they can be
/// implemented independently in the module that owns their state.
macro_rules! unimplemented_actions {
    ($($name:ident),+ $(,)?) => {
        $(pub(super) fn $name(_action: &Action) -> Changes {
            Err(crate::Refusal::Invalid)
        })+
    };
}

use unimplemented_actions;

pub fn apply(
    table: &mut Table,
    by: Seat,
    action: &Action,
    chance: &mut Chance,
) -> Result<Entry, Refusal> {
    permissions::check(table, by, action)?;
    let n = table.entry_number.checked_add(1).ok_or(Refusal::Invalid)?;
    let mut planned_chance = chance.clone();
    let mut changes = changes_for(table, by, action, &mut planned_chance)?;
    perform(table, &changes, action);
    let top_knowledge = library::keep_top_known(table);
    perform(table, &top_knowledge, action);
    changes.extend(top_knowledge);
    *chance = planned_chance;
    table.entry_number = n;
    Ok(Entry {
        n,
        by,
        action: action.clone(),
        changes,
        undoes: None,
    })
}

fn perform(table: &mut Table, changes: &[Change], action: &Action) {
    for (index, change) in changes.iter().enumerate() {
        debug_assert!(
            valid_knowledge_change(table, action, changes, index),
            "knowledge may only shrink when a card enters a hand or is shuffled"
        );
        change.perform(table);
        if let Some(id) = change.introduced_id() {
            table.next_id = table.next_id.max(u64::from(id.0) + 1);
        }
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
        Action::Life { .. } => players::life(action),
        Action::PlayerCounter { .. } => players::player_counter(action),
        Action::Phase { .. } => game::phase(action),
        Action::NextTurn { .. } => game::next_turn(action),
        Action::Roll { .. } => players::roll(action),
        Action::Arrow { .. } => players::arrow(action),
        Action::ClearArrows { .. } => players::clear_arrows(action),
        Action::LoadDeck { .. } => game::load_deck(action),
        Action::Sideboard { .. } => game::sideboard(action),
        Action::Ready { .. } => game::ready(action),
        Action::Start { .. } => game::start(action),
        Action::NextGame { .. } => game::next_game(action),
        Action::Concede { .. } => game::concede(action),
        Action::SetAssist { .. } => game::set_assist(action),
        Action::Flow { .. } => library::flow(action),
        Action::Undo { .. } | Action::Rewind { .. } | Action::Batch { .. } => Err(Refusal::Invalid),
    }
}
