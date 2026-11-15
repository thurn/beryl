//! Commits manual table actions only when every seat can publish the result.

use beryl_session::protocol::{Answer, ObjectId as Slot, PlayerView, TableAction, TableActionRefs};
use beryl_table::state::{ObjectId, Proposal};
use beryl_table::{Action, Chance, Entry, Log, Refusal, Seat, Table};

use crate::assists::{Assists, Suggestions};
use crate::prompt::{Completion, OpenPrompt, Progress, PromptState};

#[path = "flow.rs"]
mod flow;
pub use crate::script::ProposalMarks;
use crate::script::{proposal_marks, script_with_proposal_marks, snapshot};
use crate::view::{Slots, attach_history, project};

/// The snapshot and object references issued to one viewer.
pub struct SeatPublication {
    seat: Seat,
    view: PlayerView,
    slots: Slots,
    refs: TableActionRefs,
    assists: Assists,
}

impl SeatPublication {
    /// Projects a fresh snapshot, such as the first view on joining a table.
    pub fn new(table: &Table, log: &Log, seat: Seat) -> Result<Self, Refusal> {
        Self::snapshot(table, log, seat, Slots::default(), &[], &Assists::default())
    }

    /// Updates this viewer's snapshot while preserving their existing slots.
    /// Use the same table and log throughout this publication's lifetime.
    pub fn refresh(&mut self, table: &Table, log: &Log) -> Result<(), Refusal> {
        *self = Self::snapshot(
            table,
            log,
            self.seat,
            self.slots.clone(),
            &[],
            &self.assists,
        )?;
        Ok(())
    }

    fn snapshot(
        table: &Table,
        log: &Log,
        seat: Seat,
        slots: Slots,
        names: &[Option<String>],
        assists: &Assists,
    ) -> Result<Self, Refusal> {
        let mut published = Self::project(table, log.entries(), seat, slots)?;
        published.set_assists(assists.refresh(table, seat, &Suggestions::new(table)));
        published.attach_history(log, table, names);
        Ok(published)
    }

    fn project(
        table: &Table,
        entries: &[Entry],
        seat: Seat,
        mut slots: Slots,
    ) -> Result<Self, Refusal> {
        let (view, refs) = project(table, entries, seat, &mut slots)?;
        Ok(Self {
            seat,
            view,
            slots,
            refs,
            assists: Assists::default(),
        })
    }

    pub fn view(&self) -> &PlayerView {
        &self.view
    }

    pub fn slot(&self, id: ObjectId) -> Option<Slot> {
        self.slots.get(id)
    }

    /// Returns the marks for proposals already redacted for this viewer.
    pub fn proposal_marks(&self, proposals: &[Proposal]) -> Vec<Slot> {
        proposal_marks(proposals, &self.view, &self.slots)
    }

    fn set_assists(&mut self, assists: Assists) {
        assists.project(&mut self.view, self.seat, &self.slots);
        self.assists = assists;
    }

    fn marks(&self) -> &[Slot] {
        marks_in(&self.view)
    }

    fn toggle_line(&mut self, id: u32, line: u32, on: bool) -> Result<(), Refusal> {
        let mut assists = self.assists.clone();
        assists.toggle(id, line, on)?;
        let mut view = self.view.clone();
        assists.project(&mut view, self.seat, &self.slots);
        view.script = Some(script_with_proposal_marks(
            &self.view,
            None,
            &view,
            self.seat,
            &self.slots,
            &self.slots,
            ProposalMarks {
                before: self.marks(),
                after: marks_in(&view),
            },
        )?);
        self.view = view;
        self.assists = assists;
        Ok(())
    }

    fn attach_history(&mut self, log: &Log, table: &Table, names: &[Option<String>]) {
        attach_history(&mut self.view, log, table, self.seat, names);
    }

    fn project_prompts(
        view: &mut PlayerView,
        seat: Seat,
        slots: &Slots,
        table: &Table,
        prompts: &[Option<OpenPrompt>],
    ) {
        view.handling = crate::view::handling(table, crate::view::Viewer(seat));
        let mut own = None;
        let mut choosing_first = false;
        for (actor, prompt) in table.seats().zip(prompts) {
            let Some(prompt) = prompt
                .as_ref()
                .filter(|prompt| prompt.complete.valid(table) && prompt.state.valid(table, actor))
            else {
                continue;
            };
            choosing_first |= matches!(prompt.complete, Completion::FirstPlayer);
            if actor == seat {
                own = prompt.state.view(|id| slots.get(id));
            } else if let Some(lift) = prompt.state.observer_lift() {
                view.handling = Some(lift);
            }
        }
        let manual = view.table.as_mut().expect("manual publication");
        manual.prompt = own;
        manual.choosing_first = choosing_first;
    }

    fn advance(
        &self,
        table_before: &Table,
        staged: &Table,
        entry: &Entry,
        marks: Option<ProposalMarks<'_>>,
        suggestions: &Suggestions,
        prompts: &[Option<OpenPrompt>],
    ) -> Result<Self, Refusal> {
        let mut next = Self::project(
            staged,
            std::slice::from_ref(entry),
            self.seat,
            self.slots.clone(),
        )?;
        next.set_assists(self.assists.refresh(staged, self.seat, suggestions));
        Self::project_prompts(&mut next.view, next.seat, &next.slots, staged, prompts);
        let before = self.marks();
        let after = next.marks();
        next.view.script = Some(script_with_proposal_marks(
            &self.view,
            Some((entry, table_before)),
            &next.view,
            self.seat,
            &self.slots,
            &next.slots,
            marks.unwrap_or(ProposalMarks { before, after }),
        )?);
        Ok(next)
    }
}

fn marks_in(view: &PlayerView) -> &[Slot] {
    view.table
        .as_ref()
        .and_then(|table| table.proposal_marks.as_deref())
        .unwrap_or_default()
}

/// The authoritative table and the latest publications for all of its seats.
/// A host can send each resulting view to its seat after an action succeeds.
pub struct PublishedTable {
    table: Table,
    log: Log,
    chance: Chance,
    seats: Vec<SeatPublication>,
    names: Vec<Option<String>>,
    prompts: Vec<Option<OpenPrompt>>,
}

enum Request<'a> {
    Client(TableAction),
    Host(&'a Action),
}

impl PublishedTable {
    /// Takes ownership of a new or restored table and its matching log and chance.
    pub fn new(table: Table, log: Log, chance: Chance) -> Result<Self, Refusal> {
        if !(1..=2).contains(&table.players.len()) {
            return Err(Refusal::INVALID);
        }
        let seats = table
            .seats()
            .map(|seat| SeatPublication::new(&table, &log, seat))
            .collect::<Result<_, _>>()?;
        let prompts = table.seats().map(|_| None).collect();
        Ok(Self {
            table,
            log,
            chance,
            seats,
            names: Vec::new(),
            prompts,
        })
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn publication(&self, seat: Seat) -> Option<&SeatPublication> {
        self.seats.get(usize::from(seat.0))
    }

    pub(crate) fn set_names(&mut self, names: Vec<Option<String>>) {
        self.names = names;
        for publication in &mut self.seats {
            publication.attach_history(&self.log, &self.table, &self.names);
        }
    }

    /// Retire the viewer's previous references and start a new stage snapshot.
    pub(crate) fn reset_seat(&mut self, seat: Seat) -> Result<(), Refusal> {
        let publication = self
            .seats
            .get_mut(usize::from(seat.0))
            .ok_or(Refusal::Stale)?;
        let mut reset = SeatPublication::snapshot(
            &self.table,
            &self.log,
            seat,
            Slots::default(),
            &self.names,
            &publication.assists,
        )?;
        SeatPublication::project_prompts(
            &mut reset.view,
            seat,
            &reset.slots,
            &self.table,
            &self.prompts,
        );
        reset.view.script = Some(snapshot(&reset.view, reset.marks()));
        *publication = reset;
        Ok(())
    }

    /// Open an authoritative prompt and publish its redacted view. A sort learns
    /// the cards through the log so knowledge survives a cancellation and a save.
    pub(crate) fn open_prompt(
        &mut self,
        seat: Seat,
        state: PromptState,
        on_done: impl Fn(&PromptState) -> Result<Action, Refusal> + Send + Sync + 'static,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let index = usize::from(seat.0);
        let current = self.prompts.get(index).ok_or(Refusal::Stale)?;
        if current.is_some() || !state.valid(&self.table, seat) {
            return Err(Refusal::INVALID);
        }
        let peek = state.peek();
        self.prompts[index] = Some(OpenPrompt {
            state,
            complete: Completion::Action(Box::new(on_done)),
        });
        let result = if let Some(action) = peek {
            self.apply(seat, Request::Host(&action), |_| None, save)
                .map(|_| ())
        } else {
            self.republish_prompts(Some(seat))
        };
        if result.is_err() {
            self.prompts[index] = None;
        }
        result
    }

    /// The return value indicates whether other seats also need an update.
    pub(crate) fn answer(
        &mut self,
        seat: Seat,
        answer: Answer,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<bool, Refusal> {
        let index = usize::from(seat.0);
        let publication = self.seats.get_mut(index).ok_or(Refusal::Stale)?;
        let action = match answer {
            Answer::ProposalLine { id, line, on } => {
                publication.toggle_line(id, line, on)?;
                return Ok(false);
            }
            Answer::ApplyProposal { id, revision } => {
                Some(publication.assists.apply(id, revision)?)
            }
            Answer::ApplyLabel { id } => Some(publication.assists.label_action(
                id,
                publication.view.table.as_ref().expect("manual publication"),
            )?),
            _ => None,
        };
        if let Some(action) = action {
            self.apply(seat, Request::Host(&action), |_| None, save)?;
            return Ok(true);
        }
        let current = self
            .prompts
            .get(index)
            .and_then(Option::as_ref)
            .ok_or(Refusal::Stale)?;
        let publication = &self.seats[index];
        let mut state = current.state.clone();
        let progress = state.answer(answer, publication)?;
        if progress != Progress::Editing && !matches!(&current.complete, Completion::Action(_)) {
            return self.finish_setup_prompt(seat, state, progress, save);
        }
        match progress {
            Progress::Editing => {
                let previous = std::mem::replace(
                    &mut self.prompts[index].as_mut().expect("open prompt").state,
                    state,
                );
                if let Err(error) = self.republish_prompts(Some(seat)) {
                    self.prompts[index].as_mut().expect("open prompt").state = previous;
                    return Err(error);
                }
                Ok(false)
            }
            Progress::Complete => {
                let Completion::Action(complete) = &current.complete else {
                    unreachable!("setup prompts have a separate completion path");
                };
                let action = complete(&state)?;
                self.apply_closing_prompt(seat, &action, save)?;
                Ok(true)
            }
            Progress::Cancel => {
                let all = current.state.observer_lift().is_some();
                let previous = self.prompts[index].take();
                if let Err(error) = self.republish_prompts((!all).then_some(seat)) {
                    self.prompts[index] = previous;
                    return Err(error);
                }
                Ok(all)
            }
        }
    }

    fn apply_closing_prompt(
        &mut self,
        seat: Seat,
        action: &Action,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let index = usize::from(seat.0);
        let previous = self.prompts[index].take();
        if let Err(error) = self.apply(seat, Request::Host(action), |_| None, save) {
            self.prompts[index] = previous;
            return Err(error);
        }
        Ok(())
    }

    fn republish_prompts(&mut self, only: Option<Seat>) -> Result<(), Refusal> {
        let mut prepared = Vec::new();
        for (index, publication) in self.seats.iter().enumerate() {
            if only.is_some_and(|seat| publication.seat != seat) {
                continue;
            }
            let slots = &publication.slots;
            let seat = publication.seat;
            let mut view = publication.view.clone();
            SeatPublication::project_prompts(&mut view, seat, slots, &self.table, &self.prompts);
            let marks = publication.marks();
            view.script = Some(script_with_proposal_marks(
                &publication.view,
                None,
                &view,
                seat,
                slots,
                slots,
                ProposalMarks {
                    before: marks,
                    after: marks,
                },
            )?);
            prepared.push((index, view));
        }
        for (index, view) in prepared {
            self.seats[index].view = view;
        }
        Ok(())
    }

    /// Uses only the mapping of the seat authenticated by the session.
    pub fn apply_client(&mut self, seat: Seat, action: TableAction) -> Result<&Entry, Refusal> {
        self.apply_client_checked(seat, action, |_| Ok(()))
    }

    pub(crate) fn apply_client_checked(
        &mut self,
        seat: Seat,
        action: TableAction,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<&Entry, Refusal> {
        self.apply(seat, Request::Client(action), |_| None, save)
    }

    /// Applies an action generated by the host with authoritative object IDs.
    pub fn apply_host(&mut self, seat: Seat, action: &Action) -> Result<&Entry, Refusal> {
        self.apply(seat, Request::Host(action), |_| None, |_| Ok(()))
    }

    /// As above, with the before and after proposal marks for each viewer.
    pub fn apply_host_with_marks<'a>(
        &mut self,
        seat: Seat,
        action: &Action,
        marks: impl Fn(Seat) -> ProposalMarks<'a>,
    ) -> Result<&Entry, Refusal> {
        self.apply(
            seat,
            Request::Host(action),
            |seat| Some(marks(seat)),
            |_| Ok(()),
        )
    }

    fn apply<'a>(
        &mut self,
        seat: Seat,
        action: Request<'_>,
        marks: impl Fn(Seat) -> Option<ProposalMarks<'a>>,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<&Entry, Refusal> {
        let actor = self.seats.get(usize::from(seat.0)).ok_or(Refusal::Stale)?;
        let mut prepared = Vec::with_capacity(self.seats.len());
        let check = |before: &Table, staged: &Table, entry: &Entry| {
            let suggestions = Suggestions::new(staged);
            for publication in &self.seats {
                prepared.push(publication.advance(
                    before,
                    staged,
                    entry,
                    marks(publication.seat),
                    &suggestions,
                    &self.prompts,
                )?);
            }
            save(entry)
        };
        match action {
            Request::Client(action) => actor.refs.apply_checked(
                &mut self.table,
                &mut self.log,
                seat,
                action,
                &mut self.chance,
                check,
            )?,
            Request::Host(action) => {
                self.log
                    .apply_checked(&mut self.table, seat, action, &mut self.chance, check)?
            }
        };
        for publication in &mut prepared {
            publication.attach_history(&self.log, &self.table, &self.names);
        }
        for (seat, prompt) in self.table.seats().zip(&mut self.prompts) {
            if prompt.as_ref().is_some_and(|prompt| {
                !prompt.complete.valid(&self.table) || !prompt.state.valid(&self.table, seat)
            }) {
                *prompt = None;
            }
        }
        self.seats = prepared;
        Ok(self.log.entries().last().expect("action recorded an entry"))
    }
}

#[cfg(test)]
mod tests {
    use beryl_table::Change;
    use beryl_table::state::{Card, Object, Zone};

    use super::{Action, Chance, Log, ObjectId, PublishedTable, Refusal, Seat, Slots, Table};

    #[test]
    fn capacity_failure_for_either_seat_leaves_the_entire_publication_unchanged() {
        // Fill the prior slot reservations for one viewer without growing the
        // live table. Even slots that are about to be retired cannot be reused
        // by a new object in this transition.
        let seat = Seat(0);
        let card = Card::Named("Island".into());
        let mut reservations = Table::new(2);
        for index in 0..=u16::MAX {
            let id = ObjectId(if index == 0 {
                1
            } else {
                u32::from(index) + 100
            });
            let object = Object::new(id, card.clone(), seat, Zone::Battlefield);
            Change::Create {
                object,
                index: usize::from(index),
            }
            .perform(&mut reservations);
        }
        let mut full = Slots::default();
        full.update(&reservations, seat, &[]).unwrap();

        for failing in [0, 1] {
            let mut table = Table::new(2);
            Change::Create {
                object: Object::new(ObjectId(1), card.clone(), seat, Zone::Battlefield),
                index: 0,
            }
            .perform(&mut table);
            let log = Log::new(&table);
            let mut published = PublishedTable::new(table, log, Chance::fresh(13)).unwrap();
            published
                .apply_client(seat, Action::Roll { count: 1, sides: 6 })
                .unwrap();
            let before = published.table().clone();
            let entries = published.log().entries().to_vec();
            let chance = published.chance.clone();
            let views =
                [Seat(0), Seat(1)].map(|seat| published.publication(seat).unwrap().view().clone());
            let free = std::mem::replace(&mut published.seats[failing].slots, full);
            let action = Action::Token {
                card: card.clone(),
                count: 1,
                attached_to: None,
                note: None,
                face_down: true,
            };
            assert_eq!(published.apply_client(seat, action), Err(Refusal::INVALID));
            assert_eq!(published.table(), &before);
            assert_eq!(published.log().entries(), entries);
            assert_eq!(published.chance, chance);
            for (seat, view) in published.table().seats().zip(views) {
                assert_eq!(
                    serde_json::to_value(published.publication(seat).unwrap().view()).unwrap(),
                    serde_json::to_value(view).unwrap(),
                );
            }
            full = std::mem::replace(&mut published.seats[failing].slots, free);
        }
    }
}
