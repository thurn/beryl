use serde::{Deserialize, Serialize};

use crate::change::{Edit, TableEdit};
use crate::log::Touches;
use crate::permissions::RevealRules;
use crate::state::{ObjectId, Phase, Zone};
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
    // Save replay reconstructs these facts from the action and its outcomes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) members: Vec<Member>,
    #[serde(skip)]
    pub(crate) legacy_reveals: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Member {
    len: usize,
    presentation: Option<Presentation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Presentation {
    Look {
        library: Zone,
        ids: Vec<ObjectId>,
    },
    Reveal {
        zone: Zone,
        ids: Vec<ObjectId>,
        to: Vec<Seat>,
    },
    Shuffle {
        library: Zone,
    },
    Phase(Phase),
    Reset,
    GameReset,
    LobbyReset,
    Draw,
    MulliganReturn,
    MulliganShuffle {
        library: Zone,
    },
    Peek {
        ids: Vec<ObjectId>,
    },
}

impl Entry {
    pub(crate) fn member_changes(
        &self,
    ) -> impl Iterator<Item = (Option<&Presentation>, &[Change])> {
        let single = self.members.is_empty()
            && (!self.changes.is_empty() || !matches!(self.action, Action::Batch { .. }));
        let mut remaining = self.changes.as_slice();
        self.members
            .iter()
            .map(move |member| {
                let (changes, rest) = remaining.split_at(member.len);
                remaining = rest;
                (member.presentation.as_ref(), changes)
            })
            .chain(single.then_some((None, self.changes.as_slice())))
    }
}

#[derive(Default)]
pub(crate) struct AppliedChanges {
    pub(crate) changes: Vec<Change>,
    members: Vec<Member>,
}

impl AppliedChanges {
    pub(crate) fn record(&mut self, changes: Vec<Change>, presentation: Option<Presentation>) {
        self.members.push(Member {
            len: changes.len(),
            presentation,
        });
        self.changes.extend(changes);
    }

    fn append(&mut self, mut other: Self) {
        self.changes.append(&mut other.changes);
        self.members.append(&mut other.members);
    }

    fn presented(changes: Vec<Change>, presentation: Presentation) -> Self {
        let mut applied = Self::default();
        applied.record(changes, Some(presentation));
        applied
    }

    fn close_handling_before(&mut self, index: usize, change: Change) {
        let mut start = 0;
        for member in &mut self.members {
            let end = start + member.len;
            if index < end {
                member.len += 1;
                // Keep dependent edits in the member adjacent, including a
                // shuffle's knowledge reset and the id replacement it needs.
                self.changes.insert(start, change);
                return;
            }
            start = end;
        }
        unreachable!("every planned change belongs to a member");
    }

    fn extend_last_member(&mut self, len: usize) {
        if let Some(member) = self.members.last_mut() {
            member.len += len;
        } else if len > 0 {
            self.members.push(Member {
                len,
                presentation: None,
            });
        }
    }
}

type Changes = Result<Vec<Change>, Refusal>;

fn edit<T: PartialEq, C>(before: T, after: T, change: impl FnOnce(Edit<T>) -> C) -> Option<C> {
    (before != after).then(|| change(Edit { before, after }))
}

/// Applies an ordinary action or batch without keeping a log. History actions
/// use [`crate::Log::apply`].
pub fn apply(
    table: &mut Table,
    by: Seat,
    action: &Action,
    chance: &mut Chance,
) -> Result<Entry, Refusal> {
    apply_with_reveals(table, by, action, chance, RevealRules::Explicit)
}

pub(crate) fn apply_with_reveals(
    table: &mut Table,
    by: Seat,
    action: &Action,
    chance: &mut Chance,
    reveals: RevealRules,
) -> Result<Entry, Refusal> {
    apply_mapped(table, by, action.clone(), chance, reveals, |_, action| {
        Ok(action)
    })
}

/// Resolve each leaf against the staged state before applying it.
pub(crate) fn apply_mapped<Id>(
    table: &mut Table,
    by: Seat,
    action: Action<Id>,
    chance: &mut Chance,
    reveals: RevealRules,
    mut resolve: impl FnMut(&Table, Action<Id>) -> Result<Action, Refusal>,
) -> Result<Entry, Refusal> {
    let n = next_entry(table)?;
    let mut planned_chance = chance.clone();
    let mut touches = Touches::default();
    let (action, mut applied) = apply_action(
        table,
        by,
        action,
        &mut planned_chance,
        &mut touches,
        reveals,
        &mut resolve,
    )?;
    if matches!(
        &applied.members[..],
        [Member {
            presentation: None,
            ..
        }]
    ) && !matches!(action, Action::Batch { .. })
    {
        applied.members.clear();
    }
    *chance = planned_chance;
    Ok(commit(table, n, by, action, applied, touches))
}

pub(crate) fn next_entry(table: &Table) -> Result<u32, Refusal> {
    table.entry_number.checked_add(1).ok_or(Refusal::INVALID)
}

pub(crate) fn commit(
    table: &mut Table,
    n: u32,
    by: Seat,
    action: Action,
    applied: AppliedChanges,
    touches: Touches,
) -> Entry {
    table.entry_number = n;
    let summary = crate::log::Summary::new(table, by, &action, &applied.changes);
    Entry {
        n,
        by,
        action,
        changes: applied.changes,
        undoes: None,
        touches,
        seen: false,
        undone_action: None,
        summary,
        members: applied.members,
        legacy_reveals: false,
    }
}

fn apply_action<Id>(
    table: &mut Table,
    by: Seat,
    action: Action<Id>,
    chance: &mut Chance,
    touches: &mut Touches,
    reveals: RevealRules,
    resolve: &mut impl FnMut(&Table, Action<Id>) -> Result<Action, Refusal>,
) -> Result<(Action, AppliedChanges), Refusal> {
    if let Action::Batch { actions } = action {
        permissions::check_actor(table, by)?;
        let mut staged = table.clone();
        let mut applied = AppliedChanges::default();
        let mut resolved = Vec::with_capacity(actions.len());
        for member in actions {
            let (action, changes) =
                apply_action(&mut staged, by, member, chance, touches, reveals, resolve)?;
            resolved.push(action);
            applied.append(changes);
        }
        *table = staged;
        return Ok((Action::Batch { actions: resolved }, applied));
    }
    let action = resolve(table, action)?;
    permissions::check_with_reveals(table, by, &action, reveals)?;
    let mut applied = plan_action(table, by, &action, chance, reveals)?;
    library::settle_handling(table, by, &action, &mut applied);
    let len = applied.changes.len();
    if table.turn_plays.is_some() && counts_as_turn_play(&action, table.turn) {
        applied
            .changes
            .push(Change::SetTable(TableEdit::TurnPlay { add: true }));
    }
    perform_with_top_knowledge(table, &action, &mut applied.changes, touches);
    applied.extend_last_member(applied.changes.len() - len);
    Ok((action, applied))
}

/// Opening-hand steps and assist switches can be used before the first play.
fn counts_as_turn_play(action: &Action, turn: u32) -> bool {
    use crate::action::{Destination, Position};
    match action {
        Action::Start { .. }
        | Action::Keep {}
        | Action::NextTurn {}
        | Action::NextGame { .. }
        | Action::Concede {}
        | Action::SetAssist { .. }
        | Action::CloseLook {}
        | Action::Phase {
            phase: Phase::Untap,
        } => false,
        Action::Mulligan { .. } if turn == 1 => false,
        Action::Move {
            selection,
            to: Destination::Zone(Zone::Library(seat)),
            position: Position::Bottom,
            ..
        } if turn == 1 && selection.zone == Zone::Hand(*seat) => false,
        _ => true,
    }
}

pub(crate) fn perform_history(
    table: &mut Table,
    action: &Action,
    mut applied: AppliedChanges,
) -> (AppliedChanges, Touches) {
    let mut touches = Touches::default();
    let len = applied.changes.len();
    for index in 0..len {
        // The inverse of a creation carries the original visibility. Record
        // what the players actually knew when the undo removed the card.
        if let Change::Remove { object, .. } = &mut applied.changes[index] {
            object
                .known_to
                .clone_from(&table.objects[&object.id].known_to);
        }
        let change = &applied.changes[index];
        touches.record(table, change);
        perform_one(table, action, &applied.changes, index, change);
    }
    perform_top_knowledge(table, action, &mut applied.changes, &mut touches);
    applied.extend_last_member(applied.changes.len() - len);
    (applied, touches)
}

fn perform_with_top_knowledge(
    table: &mut Table,
    action: &Action,
    changes: &mut Vec<Change>,
    touches: &mut Touches,
) {
    perform_tracking(table, changes, action, touches);
    perform_top_knowledge(table, action, changes, touches);
}

fn perform_top_knowledge(
    table: &mut Table,
    action: &Action,
    changes: &mut Vec<Change>,
    touches: &mut Touches,
) {
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

#[expect(
    clippy::too_many_lines,
    reason = "one dispatch lists each table action"
)]
fn plan_action(
    table: &Table,
    by: Seat,
    action: &Action,
    chance: &mut Chance,
    reveals: RevealRules,
) -> Result<AppliedChanges, Refusal> {
    let changes = match action {
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
        Action::Peek { selection } => {
            return Ok(AppliedChanges::presented(
                cards::peek(table, by, selection)?,
                Presentation::Peek {
                    ids: selection.objects.clone(),
                },
            ));
        }
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
            face_down,
        } => cards::token(
            table,
            by,
            card,
            *count,
            *attached_to,
            note.as_deref(),
            *face_down,
        ),
        Action::Marker { selection, text } => cards::marker(table, selection, text),
        Action::Remove { selection } => cards::remove(table, selection),
        Action::StackUntil { seat, query, hits } => {
            library::stack_until(table, *seat, query, *hits)
        }
        Action::Draw { seat, count } => {
            return Ok(AppliedChanges::presented(
                library::draw(table, *seat, *count)?,
                Presentation::Draw,
            ));
        }
        Action::Mulligan { seat, count } => {
            return library::mulligan(table, *seat, *count, chance, action);
        }
        Action::Shuffle { seat, portion } => {
            return library::plan_shuffle(table, *seat, *portion, chance);
        }
        Action::Look { seat, portion } => return library::look(table, by, *seat, *portion),
        Action::Reveal {
            zone,
            objects,
            random,
            to,
        } => {
            return library::reveal(
                table,
                *zone,
                objects.as_deref(),
                *random,
                to,
                chance,
                reveals,
            );
        }
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
        Action::Phase { phase } => return Ok(game::plan_phase(table, *phase)),
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
        Action::GameSettings { settings } => game::settings(table, *settings),
        Action::Start { first, opening } => {
            return Ok(AppliedChanges::presented(
                game::start(table, *first, *opening, chance, action)?,
                Presentation::GameReset,
            ));
        }
        Action::NextGame { winner } => {
            return Ok(AppliedChanges::presented(
                game::next_game(table, *winner)?,
                Presentation::GameReset,
            ));
        }
        Action::Concede {} => {
            return Ok(AppliedChanges::presented(
                game::concede(table, by)?,
                Presentation::LobbyReset,
            ));
        }
        Action::CloseLook {} | Action::Keep {} => Ok(Vec::new()),
        Action::SetAssist { assist, on } => Ok(game::set_assist(table, by, *assist, *on)),
        Action::Flow { seat, count, .. } => library::flow(table, by, *seat, *count),
        Action::Undo { .. } | Action::Rewind { .. } | Action::Batch { .. } => Err(Refusal::INVALID),
    }?;
    let mut applied = AppliedChanges::default();
    applied.record(changes, None);
    Ok(applied)
}
