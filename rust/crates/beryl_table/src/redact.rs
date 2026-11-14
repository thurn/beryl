//! Projections of the host's table and log for a single seat.

use std::collections::BTreeSet;

use crate::change::{ObjectEdit, TableEdit};
use crate::state::{AbilityMarker, Card, Object, ObjectId};
use crate::{Action, Change, Entry, Seat, Table};

/// Keep zone order and public object state while hiding identities the seat
/// hasn't learned. The host retains the full table.
#[must_use]
pub fn seen_by(table: &Table, seat: Seat) -> Table {
    let mut view = table.clone();
    let names = HiddenNames::new(table, seat);
    for object in view.objects.values_mut() {
        redact_object(object, seat, &names);
    }
    for marker in view.markers.values_mut() {
        redact_marker(marker, table, seat, &names);
    }
    for player in &mut view.players {
        names.redact_counters(&mut player.counters);
    }
    for (state, owner) in view.game_match.seats.iter_mut().zip(table.seats()) {
        redact_deck(&mut state.deck, owner, seat);
        redact_deck(&mut state.sideboard, owner, seat);
    }
    for proposal in &mut view.proposals {
        for line in &mut proposal.lines {
            names.redact_text(&mut line.text);
            for action in &mut line.actions {
                redact_action(action, table, seat, &names);
            }
        }
    }
    view
}

/// Project the entry for display using the host table immediately before it.
/// The host retains the original entry for replay, including private changes.
#[must_use]
pub fn entry_seen_by(entry: &Entry, table_before: &Table, seat: Seat) -> Entry {
    let mut after = table_before.clone();
    for change in &entry.changes {
        change.perform(&mut after);
    }
    let mut names = HiddenNames::new(table_before, seat);
    names.include_table(&after, seat);

    // Suppressing the random numbers alone would leave the shuffle order
    // recoverable from the moves and the links between old and new ids.
    let mut private_ids = BTreeSet::new();
    for pair in entry.changes.windows(2) {
        match pair {
            [
                Change::ResetKnowledge {
                    id,
                    edit,
                    forget: true,
                },
                Change::ReplaceId { from, to },
            ] if id == from && (edit.after.is_empty() || !knows(&after, *to, seat)) => {
                private_ids.extend([*from, *to]);
            }
            [
                Change::ReplaceId { from, to },
                Change::ResetKnowledge {
                    id,
                    edit,
                    forget: false,
                },
            ] if id == to && (edit.before.is_empty() || !knows(table_before, *from, seat)) => {
                private_ids.extend([*from, *to]);
            }
            _ => {}
        }
    }

    let mut projected = entry.clone();
    redact_action(&mut projected.action, &after, seat, &names);
    if let Some(action) = &mut projected.undone_action {
        redact_action(action, &after, seat, &names);
    }
    projected.touches.project(&private_ids, &after, seat);
    projected.changes.retain_mut(|change| match change {
        Change::Move { id, .. } | Change::ReplaceId { from: id, .. } => !private_ids.contains(id),
        Change::Random { .. } | Change::RestoreTable(_) | Change::ResetKnowledge { .. } => false,
        Change::Learn { id, .. } => knows(&after, *id, seat),
        Change::Create { object, .. } => {
            if private_ids.contains(&object.id) {
                return false;
            }
            if let Some(current) = after.objects.get(&object.id) {
                object.known_to.clone_from(&current.known_to);
            } else {
                object.known_to.clear();
            }
            redact_object(object, seat, &names);
            true
        }
        Change::Remove { object, .. } => {
            if !knows(table_before, object.id, seat)
                || (after.objects.contains_key(&object.id) && !knows(&after, object.id, seat))
            {
                object.known_to.clear();
            }
            redact_object(object, seat, &names);
            true
        }
        Change::CreateMarker { marker, .. } => {
            redact_marker(marker, &after, seat, &names);
            true
        }
        Change::RemoveMarker { marker, .. } => {
            redact_marker(marker, table_before, seat, &names);
            true
        }
        Change::SetObject { id, edit } => {
            redact_object_edit(
                edit,
                knows(table_before, *id, seat),
                knows(&after, *id, seat),
                &names,
            );
            true
        }
        Change::SetTable(edit) => redact_table_edit(edit, seat, &names),
        Change::Roll { .. }
        | Change::InsertArrow { .. }
        | Change::RemoveArrow { .. }
        | Change::Lend { .. }
        | Change::AlwaysRevealTop { .. }
        | Change::AlwaysLookAtTop { .. }
        | Change::Nothing => true,
    });
    projected
}

fn knows(table: &Table, id: ObjectId, seat: Seat) -> bool {
    table
        .objects
        .get(&id)
        .is_some_and(|object| object.known_to.contains(&seat))
}

fn redact_object(object: &mut Object, seat: Seat, names: &HiddenNames) {
    if !object.known_to.contains(&seat) {
        object.card = Card::Hidden;
        object.note = None;
    }
    if let Card::Custom { types, colors, .. } = &mut object.card {
        names.redact_text(types);
        colors.retain(|color| !names.mentions_hidden(color));
    }
    names.redact_note(&mut object.note);
    names.redact_counters(&mut object.counters);
}

fn redact_marker(marker: &mut AbilityMarker, table: &Table, seat: Seat, names: &HiddenNames) {
    if knows(table, marker.source, seat) {
        names.redact_text(&mut marker.text);
    } else {
        marker.text.clear();
    }
}

fn redact_object_edit(edit: &mut ObjectEdit, knew: bool, knows: bool, names: &HiddenNames) {
    match edit {
        ObjectEdit::Note(edit) => {
            if !knew {
                edit.before = None;
            }
            if !knows {
                edit.after = None;
            }
            names.redact_note(&mut edit.before);
            names.redact_note(&mut edit.after);
        }
        ObjectEdit::Counters(edit) => {
            names.redact_counters(&mut edit.before);
            names.redact_counters(&mut edit.after);
        }
        ObjectEdit::Controller(_)
        | ObjectEdit::Face(_)
        | ObjectEdit::Tapped(_)
        | ObjectEdit::Stats(_)
        | ObjectEdit::Damage(_)
        | ObjectEdit::AttachedTo(_)
        | ObjectEdit::DoesntUntap(_)
        | ObjectEdit::Row(_) => {}
    }
}

fn redact_table_edit(edit: &mut TableEdit, seat: Seat, names: &HiddenNames) -> bool {
    match edit {
        TableEdit::SetDeck { seat: owner, .. } | TableEdit::SetSideboard { seat: owner, .. } => {
            *owner == seat
        }
        TableEdit::SetPlayerCounters { edit, .. } => {
            names.redact_counters(&mut edit.before);
            names.redact_counters(&mut edit.after);
            true
        }
        TableEdit::SetLife { .. }
        | TableEdit::SetActive { .. }
        | TableEdit::SetTurn { .. }
        | TableEdit::SetPhase { .. }
        | TableEdit::SetReady { .. }
        | TableEdit::SetWins { .. }
        | TableEdit::SetInGame { .. }
        | TableEdit::SetLastWinner { .. } => true,
    }
}

fn redact_action(action: &mut Action, table: &Table, seat: Seat, names: &HiddenNames) {
    match action {
        Action::LoadDeck {
            seat: owner,
            main,
            side,
        } => {
            redact_deck(main, *owner, seat);
            redact_deck(side, *owner, seat);
        }
        Action::Sideboard {
            seat: owner,
            into_main,
            into_side,
        } => {
            redact_deck(into_main, *owner, seat);
            redact_deck(into_side, *owner, seat);
        }
        Action::Note { selection, note } => {
            if selection.objects.iter().any(|id| !knows(table, *id, seat)) {
                *note = None;
            }
            names.redact_note(note);
        }
        Action::Marker { selection, text } => {
            if selection.objects.iter().any(|id| !knows(table, *id, seat)) {
                text.clear();
            }
            names.redact_text(text);
        }
        Action::Token { card, note, .. } => {
            match card {
                Card::Named(name) if names.mentions_hidden(name) => *card = Card::Hidden,
                Card::Custom { name, .. } if names.mentions_hidden(name) => *card = Card::Hidden,
                Card::Custom { types, colors, .. } => {
                    names.redact_text(types);
                    colors.retain(|color| !names.mentions_hidden(color));
                }
                Card::Named(_) | Card::Hidden => {}
            }
            names.redact_note(note);
        }
        Action::Counter { name, .. } | Action::PlayerCounter { name, .. } => {
            names.redact_text(name);
        }
        Action::Reveal {
            objects: Some(ids), ..
        } => ids.retain(|&id| knows(table, id, seat)),
        Action::Peek { selection } => selection.objects.retain(|&id| knows(table, id, seat)),
        Action::Batch { actions } => {
            for action in actions {
                redact_action(action, table, seat, names);
            }
        }
        Action::Move { .. }
        | Action::Reorder { .. }
        | Action::Pin { .. }
        | Action::Give { .. }
        | Action::Tap { .. }
        | Action::Face { .. }
        | Action::Flag { .. }
        | Action::Stats { .. }
        | Action::Damage { .. }
        | Action::Attach { .. }
        | Action::Unattach { .. }
        | Action::Clone { .. }
        | Action::Remove { .. }
        | Action::Draw { .. }
        | Action::Mulligan { .. }
        | Action::Shuffle { .. }
        | Action::Look { .. }
        | Action::Reveal { .. }
        | Action::Lend { .. }
        | Action::AlwaysRevealTop { .. }
        | Action::AlwaysLookAtTop { .. }
        | Action::SortHand { .. }
        | Action::Life { .. }
        | Action::Phase { .. }
        | Action::NextTurn { .. }
        | Action::Roll { .. }
        | Action::Arrow { .. }
        | Action::ClearArrows { .. }
        | Action::Ready { .. }
        | Action::Start { .. }
        | Action::NextGame { .. }
        | Action::Concede { .. }
        | Action::SetAssist { .. }
        | Action::Flow { .. }
        | Action::Undo { .. }
        | Action::Rewind { .. } => {}
    }
}

fn redact_deck(cards: &mut Vec<(String, u16)>, owner: Seat, viewer: Seat) {
    if owner != viewer {
        cards.clear();
    }
}

/// Free text has no object references. If it mentions the name of a card the
/// seat can't identify, suppress the whole label. Structured cards use their
/// own knowledge even when two different objects share a name.
struct HiddenNames(BTreeSet<String>);

impl HiddenNames {
    fn new(table: &Table, seat: Seat) -> Self {
        let mut names = Self(BTreeSet::new());
        names.include_table(table, seat);
        names
    }

    fn include_table(&mut self, table: &Table, seat: Seat) {
        for object in table.objects.values() {
            if !object.known_to.contains(&seat) {
                match &object.card {
                    Card::Named(name) | Card::Custom { name, .. } => self.insert(name),
                    Card::Hidden => {}
                }
            }
        }
        for (state, owner) in table.game_match.seats.iter().zip(table.seats()) {
            if seat != owner {
                for (name, _) in state.deck.iter().chain(&state.sideboard) {
                    self.insert(name);
                }
            }
        }
    }

    fn insert(&mut self, name: &str) {
        if !name.is_empty() {
            self.0.insert(name.to_lowercase());
        }
    }

    fn mentions_hidden(&self, text: &str) -> bool {
        if text.is_empty() || self.0.is_empty() {
            return false;
        }
        let lower = text.to_lowercase();
        self.0.iter().any(|name| lower.contains(name))
    }

    fn redact_text(&self, text: &mut String) {
        if self.mentions_hidden(text) {
            text.clear();
        }
    }

    fn redact_note(&self, note: &mut Option<String>) {
        if note.as_ref().is_some_and(|text| self.mentions_hidden(text)) {
            *note = None;
        }
    }

    fn redact_counters(&self, counters: &mut Vec<(String, i32)>) {
        counters.retain(|(name, _)| !self.mentions_hidden(name));
    }
}
