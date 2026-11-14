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
    _chance: &mut Chance,
) -> Result<Entry, Refusal> {
    permissions::check(table, by, action)?;
    let n = table.entry_number.checked_add(1).ok_or(Refusal::Invalid)?;
    let changes = changes_for(table, action)?;
    perform(table, &changes);
    table.entry_number = n;
    Ok(Entry {
        n,
        by,
        action: action.clone(),
        changes,
        undoes: None,
    })
}

fn perform(table: &mut Table, changes: &[Change]) {
    for change in changes {
        change.perform(table);
        if let Some(id) = change.introduced_id() {
            table.next_id = table.next_id.max(u64::from(id.0) + 1);
        }
    }
}

fn changes_for(table: &Table, action: &Action) -> Changes {
    match action {
        Action::Move {
            selection,
            to,
            position,
            face,
            tapped,
        } => cards::move_cards(table, selection, *to, *position, *face, *tapped),
        Action::Tap { selection, tapped } => cards::tap(table, selection, *tapped),
        Action::Reorder { .. } => cards::reorder(action),
        Action::Pin { .. } => cards::pin(action),
        Action::Give { .. } => cards::give(action),
        Action::Face { .. } => cards::face(action),
        Action::Flag { .. } => cards::flag(action),
        Action::Peek { .. } => cards::peek(action),
        Action::Counter { .. } => cards::counter(action),
        Action::Stats { .. } => cards::stats(action),
        Action::Damage { .. } => cards::damage(action),
        Action::Note { .. } => cards::note(action),
        Action::Attach { .. } => cards::attach(action),
        Action::Unattach { .. } => cards::unattach(action),
        Action::Clone { .. } => cards::clone(action),
        Action::Token { .. } => cards::token(action),
        Action::Marker { .. } => cards::marker(action),
        Action::Remove { .. } => cards::remove(action),
        Action::Draw { .. } => library::draw(action),
        Action::Mulligan { .. } => library::mulligan(action),
        Action::Shuffle { .. } => library::shuffle(action),
        Action::Look { .. } => library::look(action),
        Action::Reveal { .. } => library::reveal(action),
        Action::Lend { .. } => library::lend(action),
        Action::AlwaysRevealTop { .. } => library::always_reveal_top(action),
        Action::AlwaysLookAtTop { .. } => library::always_look_at_top(action),
        Action::SortHand { .. } => library::sort_hand(action),
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
