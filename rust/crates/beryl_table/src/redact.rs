//! Projections of the host's table and log for a single seat.

use std::collections::BTreeSet;

use crate::change::{ObjectEdit, TableEdit};
use crate::state::{AbilityMarker, Card, LineVisibility, Object, ObjectId};
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
        redact_marker(marker, seat, &names);
    }
    for (state, owner) in view.game_match.seats.iter_mut().zip(table.seats()) {
        redact_deck(&mut state.deck, owner, seat);
        redact_deck(&mut state.sideboard, owner, seat);
    }
    for proposal in &mut view.proposals {
        for line in &mut proposal.lines {
            match line.visibility {
                LineVisibility::Public => {}
                LineVisibility::KnownObject(id) if knows(table, id, seat) => {}
                LineVisibility::KnownObject(_) => line.text.clear(),
                LineVisibility::Unstructured => names.redact_text(&mut line.text),
            }
            for action in &mut line.actions {
                redact_action(action, table, table, seat, &names, &BTreeSet::new());
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

    let private_ids = forgotten_ids(entry, table_before, &after, seat);
    let private_marker_sources = entry
        .changes
        .iter()
        .filter_map(|change| match change {
            Change::CreateMarker { marker, .. } | Change::RemoveMarker { marker, .. }
                if !marker_visible(marker, seat) =>
            {
                Some(marker.source)
            }
            _ => None,
        })
        .collect();
    let mut projected = entry.clone();
    redact_action(
        &mut projected.action,
        table_before,
        &after,
        seat,
        &names,
        &private_marker_sources,
    );
    if let Some(action) = &mut projected.undone_action {
        redact_action(
            action,
            table_before,
            &after,
            seat,
            &names,
            &private_marker_sources,
        );
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
        Change::CreateMarker { marker, .. } | Change::RemoveMarker { marker, .. } => {
            redact_marker(marker, seat, &names);
            true
        }
        Change::SetObject { id, edit } => {
            redact_object_edit(
                edit,
                table_before.objects.get(id),
                after.objects.get(id),
                seat,
            );
            true
        }
        Change::SetTable(edit) => redact_table_edit(edit, seat),
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

fn forgotten_ids(entry: &Entry, before: &Table, after: &Table, seat: Seat) -> BTreeSet<ObjectId> {
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
            ] if id == from && (edit.after.is_empty() || !knows(after, *to, seat)) => {
                private_ids.extend([*from, *to]);
            }
            [
                Change::ReplaceId { from, to },
                Change::ResetKnowledge {
                    id,
                    edit,
                    forget: false,
                },
            ] if id == to && (edit.before.is_empty() || !knows(before, *from, seat)) => {
                private_ids.extend([*from, *to]);
            }
            _ => {}
        }
    }
    private_ids
}

fn knows(table: &Table, id: ObjectId, seat: Seat) -> bool {
    table
        .objects
        .get(&id)
        .is_some_and(|object| object.known_to.contains(&seat))
}

fn redact_object(object: &mut Object, seat: Seat, names: &HiddenNames) {
    if !object.known_to.contains(&seat) {
        object
            .counters
            .retain(|(label, _)| !mentions_card(label, &object.card));
        object.card = Card::Hidden;
        object.note = None;
    }
    if let Card::Custom { types, colors, .. } = &mut object.card {
        names.redact_text(types);
        colors.retain(|color| !names.mentions_hidden(color));
    }
}

// A label attached to an unidentified card can disclose that card's identity.
// Counters elsewhere can independently use the same word.
fn mentions_card(label: &str, card: &Card) -> bool {
    match card {
        Card::Named(name) | Card::Custom { name, .. } => {
            !name.is_empty() && label.to_lowercase().contains(&name.to_lowercase())
        }
        Card::Hidden => false,
    }
}

fn unknown_card(object: Option<&Object>, seat: Seat) -> Option<&Card> {
    object
        .filter(|object| !object.known_to.contains(&seat))
        .map(|object| &object.card)
}

fn marker_visible(marker: &AbilityMarker, seat: Seat) -> bool {
    marker
        .display_source
        .as_ref()
        .is_some_and(|source| source.known_to.contains(&seat) && source.card != Card::Hidden)
}

fn redact_marker(marker: &mut AbilityMarker, seat: Seat, names: &HiddenNames) {
    if !marker_visible(marker, seat) {
        marker.display_source = None;
        marker.text.clear();
        return;
    }
    let source = marker.display_source.as_mut().expect("visible source");
    if let Card::Custom { types, colors, .. } = &mut source.card {
        names.redact_text(types);
        colors.retain(|color| !names.mentions_hidden(color));
    }
    source.known_to.retain(|&viewer| viewer == seat);
}

fn redact_object_edit(
    edit: &mut ObjectEdit,
    before: Option<&Object>,
    after: Option<&Object>,
    seat: Seat,
) {
    match edit {
        ObjectEdit::Note(edit) => {
            if !before.is_some_and(|object| object.known_to.contains(&seat)) {
                edit.before = None;
            }
            if !after.is_some_and(|object| object.known_to.contains(&seat)) {
                edit.after = None;
            }
        }
        ObjectEdit::Counters(edit) => {
            if let Some(card) = unknown_card(before.or(after), seat) {
                edit.before.retain(|(label, _)| !mentions_card(label, card));
            }
            if let Some(card) = unknown_card(after.or(before), seat) {
                edit.after.retain(|(label, _)| !mentions_card(label, card));
            }
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

fn redact_table_edit(edit: &TableEdit, seat: Seat) -> bool {
    match edit {
        TableEdit::SetDeck { seat: owner, .. } | TableEdit::SetSideboard { seat: owner, .. } => {
            *owner == seat
        }
        TableEdit::SetPlayerCounters { .. }
        | TableEdit::SetLife { .. }
        | TableEdit::SetActive { .. }
        | TableEdit::SetTurn { .. }
        | TableEdit::SetPhase { .. }
        | TableEdit::ResetTurnPlays { .. }
        | TableEdit::TurnPlay { .. }
        | TableEdit::SetAssists { .. }
        | TableEdit::SetReady { .. }
        | TableEdit::SetWins { .. }
        | TableEdit::SetInGame { .. }
        | TableEdit::SetLastWinner { .. } => true,
    }
}

fn redact_action(
    action: &mut Action,
    before: &Table,
    table: &Table,
    seat: Seat,
    names: &HiddenNames,
    private_marker_sources: &BTreeSet<ObjectId>,
) {
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
        }
        Action::Counter {
            selection, name, ..
        } => {
            if selection.objects.iter().any(|id| {
                [before, table].into_iter().any(|state| {
                    unknown_card(state.objects.get(id), seat)
                        .is_some_and(|card| mentions_card(name, card))
                })
            }) {
                name.clear();
            }
        }
        Action::Marker { selection, text } => {
            // A batch can reveal its source after placing a private marker.
            // The projected marker change carries its exact display text.
            if selection
                .objects
                .iter()
                .any(|id| private_marker_sources.contains(id) || !knows(table, *id, seat))
            {
                text.clear();
            }
        }
        Action::Token { card, note, .. } => {
            names.redact_token(card);
            names.redact_note(note);
        }
        Action::Reveal {
            objects: Some(ids), ..
        } => ids.retain(|&id| knows(table, id, seat)),
        Action::Peek { selection } => selection.objects.retain(|&id| knows(table, id, seat)),
        Action::Batch { actions } => {
            for action in actions {
                redact_action(action, before, table, seat, names, private_marker_sources);
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
        | Action::PlayerCounter { .. }
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

/// Older proposal labels and unstructured custom card descriptions can mention
/// a card whose identity is private. Structured labels have their own visibility.
struct HiddenNames(BTreeSet<String>);

impl HiddenNames {
    fn redact_token(&self, card: &mut Card) {
        match card {
            Card::Named(name) | Card::Custom { name, .. } if self.mentions_hidden(name) => {
                *card = Card::Hidden;
            }
            Card::Custom { types, colors, .. } => {
                self.redact_text(types);
                colors.retain(|color| !self.mentions_hidden(color));
            }
            Card::Named(_) | Card::Hidden => {}
        }
    }

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
}
