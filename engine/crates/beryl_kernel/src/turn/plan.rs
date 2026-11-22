//! The remaining steps of a turn whose structure can change.
//!
//! Implements: CR 500.8, CR 500.9, CR 500.11, CR 505.1a

use std::hash::{Hash, Hasher};

use beryl_card::{TurnPhase, TurnStep};

use crate::event::Event;
use crate::fixed::Full;
use crate::format::Format;
use crate::layout::{Layout, Queue, Small};
use crate::state::State;

/// A step's place in its turn. A planned turn counts steps as played. An ordinary
/// turn uses the format's fixed positions, with each repeated cleanup numbered after them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StepPosition {
    pub turn: u16,
    pub ordinal: u32,
}

/// The beginning of the combat a simulation is following.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombatPosition(StepPosition);

/// A scheduled step, stable when steps ahead of it are inserted or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepTarget {
    Standard(StepPosition),
    Scheduled { turn: u16, id: u32 },
}

/// Where an insertion goes relative to its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    Before,
    After,
}

/// An edit to a turn's remaining steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TurnChange {
    InsertStep(TurnStep),
    InsertPhase(TurnPhase),
    RemoveStep(TurnStep),
    RemovePhase(TurnPhase),
    CutToCleanup,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlannedStep {
    // Storing the ID as bytes keeps each of the 24 entries in Small at six
    // bytes, without reducing the range of IDs available in a turn.
    id: [u8; 4],
    step: Option<TurnStep>,
    starts_phase: bool,
}

impl PlannedStep {
    fn id(self) -> u32 {
        u32::from_ne_bytes(self.id)
    }

    fn name(self) -> TurnStep {
        self.step.expect("an entry in the turn plan has a step")
    }
}

impl Hash for PlannedStep {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Preserve the hash of an existing plan across the layout change.
        self.id().hash(state);
        self.step.hash(state);
        self.starts_phase.hash(state);
    }
}

/// The queue fits a normal turn plus two extra combats in Small.
const CAPACITY: usize = 24;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct TurnPlan<L: Layout> {
    remaining: L::Queue<PlannedStep, CAPACITY>,
    ordinal: u32,
    phase: u32,
    next_id: u32,
    played: u16,
}

impl Copy for TurnPlan<Small> {}

impl<L: Layout> TurnPlan<L> {
    pub(crate) fn is_unused(&self) -> bool {
        self.next_id == 0 && self.ordinal == 0
    }

    /// Whether layout `M` has room for the steps left in the turn.
    pub(crate) fn fits<M: Layout>(&self) -> Result<(), crate::Misfit> {
        crate::fit::room(
            "the steps left in the turn",
            self.remaining.len(),
            M::Queue::<PlannedStep, CAPACITY>::CAPACITY,
        )
    }

    pub(crate) fn relayout<M: Layout>(&self) -> TurnPlan<M> {
        TurnPlan {
            remaining: crate::relayout::queue(self.remaining.as_slice(), |&step| step),
            ordinal: self.ordinal,
            phase: self.phase,
            next_id: self.next_id,
            played: self.played,
        }
    }

    fn insert(&mut self, index: usize, steps: &[TurnStep], starts_phase: bool) -> Result<(), Full> {
        if steps.len() > L::Queue::<PlannedStep, CAPACITY>::CAPACITY - self.remaining.len() {
            return Err(Full);
        }
        let added = u32::try_from(steps.len()).map_err(|_| Full)?;
        self.next_id.checked_add(added).ok_or(Full)?;
        for (offset, &step) in steps.iter().enumerate() {
            self.next_id += 1;
            self.remaining.insert(
                index + offset,
                PlannedStep {
                    id: self.next_id.to_ne_bytes(),
                    step: Some(step),
                    starts_phase: starts_phase && offset == 0,
                },
            )?;
        }
        Ok(())
    }

    // The low bits of `played` record the step names. This bit records whether the
    // step just taken was already played this turn.
    const REPEATED: u16 = 1 << 15;

    fn has_played(&self, step: TurnStep) -> bool {
        self.played & (1 << step.index()) != 0
    }

    fn phase_end(&self, index: usize) -> usize {
        let rest = self.remaining.as_slice();
        (index..rest.len())
            .find(|&i| rest[i].starts_phase)
            .unwrap_or(rest.len())
    }

    fn remove(&mut self, index: usize) -> TurnStep {
        let removed = self.remaining.remove(index);
        if removed.starts_phase
            && let Some(next) = self.remaining.as_mut_slice().get_mut(index)
        {
            next.starts_phase = true;
        }
        removed.name()
    }
}

impl<L: Layout> State<L> {
    /// Identifies the current step independently of its name.
    pub fn step_position(&self) -> StepPosition {
        let step = self.turn.step;
        let ordinal = if self.turn_plan.next_id > 0 {
            self.turn_plan.ordinal
        } else {
            u32::try_from(step.index()).expect("a standard step fits in u32")
                + 1
                + if step == TurnStep::Cleanup {
                    self.turn_plan.ordinal
                } else {
                    0
                }
        };
        StepPosition {
            turn: self.turn.number,
            ordinal,
        }
    }

    /// Whether the current step is the first occurrence of its name in this turn.
    pub fn current_step_is_first(&self) -> bool {
        if self.has_turn_plan() {
            self.turn_plan.played & TurnPlan::<L>::REPEATED == 0
        } else {
            self.turn.step != TurnStep::Cleanup || self.turn_plan.ordinal == 0
        }
    }

    /// Whether this turn has a plan.
    pub fn has_turn_plan(&self) -> bool {
        self.turn_plan.next_id != 0
    }

    /// The names of the remaining steps, each with a target for its occurrence.
    pub fn later_steps(&self) -> impl Iterator<Item = (TurnStep, StepTarget)> + '_ {
        let turn = self.turn.number;
        let planned = self.has_turn_plan();
        let rest = &TurnStep::ALL[self.turn.step.index() + 1..];
        self.turn_plan
            .remaining
            .iter()
            .scan(
                self.turn_plan.has_played(TurnStep::PrecombatMain),
                move |had_main, entry| {
                    let step = main_phase_name(entry.name(), *had_main);
                    *had_main |= step == TurnStep::PrecombatMain;
                    Some((
                        step,
                        StepTarget::Scheduled {
                            turn,
                            id: entry.id(),
                        },
                    ))
                },
            )
            .chain(rest.iter().filter(move |_| !planned).map(move |&step| {
                let ordinal = u32::try_from(step.index()).expect("a standard step fits in u32") + 1;
                (step, StepTarget::Standard(StepPosition { turn, ordinal }))
            }))
    }

    /// Compares a named milestone with the current position. In a planned turn, a
    /// milestone is reached at the first occurrence of its name.
    pub fn compare_named_step(&self, turn: u16, step: TurnStep) -> std::cmp::Ordering {
        self.turn.number.cmp(&turn).then_with(|| {
            if !self.has_turn_plan() {
                return self.turn.step.index().cmp(&step.index());
            }
            if self.turn.step == step {
                std::cmp::Ordering::Equal
            } else if self.turn_plan.has_played(step) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        })
    }

    /// Whether a target still lies ahead in the schedule.
    pub fn before_step(&self, target: StepTarget) -> bool {
        match target {
            StepTarget::Standard(position) => self.step_position() < position,
            StepTarget::Scheduled { turn, id } => {
                self.turn.number < turn
                    || (self.turn.number == turn
                        && self
                            .turn_plan
                            .remaining
                            .iter()
                            .any(|entry| entry.id() == id))
            }
        }
    }

    /// Identifies the current combat for a caller playing through its end.
    pub fn combat_position(&self) -> CombatPosition {
        CombatPosition(StepPosition {
            turn: self.turn.number,
            ordinal: self.turn_plan.phase,
        })
    }

    /// Whether the specified combat has reached its end step or already ended.
    pub fn reached_end_of_combat(&self, combat: CombatPosition) -> bool {
        self.combat_position() != combat
            || self.turn.step == TurnStep::EndOfCombat
            || TurnPhase::of(self.turn.step) != TurnPhase::Combat
    }

    /// Starts the turn's queue with the format's steps. The rules may skip an entry
    /// before playing the first step.
    pub fn plan_turn(&mut self, format: Format) -> Result<(), Full> {
        self.turn_plan = TurnPlan::default();
        for &step in format.turn() {
            let starts_phase = matches!(
                step,
                TurnStep::Untap
                    | TurnStep::PrecombatMain
                    | TurnStep::BeginningOfCombat
                    | TurnStep::PostcombatMain
                    | TurnStep::End
            );
            let index = self.turn_plan.remaining.len();
            self.turn_plan.insert(index, &[step], starts_phase)?;
        }
        Ok(())
    }

    /// Reconstructs the ordinary remainder of a turn for a scenario placed directly in
    /// a step. No earlier turn-based actions are performed.
    pub fn plan_from_current_step(&mut self, format: Format) -> Result<(), Full> {
        self.plan_turn(format)?;
        let current = self.turn.step;
        let first = self.turn.number == 1;
        if first && current != TurnStep::Draw {
            let _skipped = self.remove_step(TurnStep::Draw);
        }
        while self.take_planned_step().is_some_and(|step| step != current) {}
        Ok(())
    }

    /// The first remaining entry, and whether it starts a new phase.
    pub fn planned_next_step(&self) -> Option<(TurnStep, bool)> {
        self.turn_plan
            .remaining
            .as_slice()
            .first()
            .map(|entry| (entry.name(), entry.starts_phase))
    }

    /// Takes the next entry, recording its place in the turn. Under [CR 505.1a],
    /// "Only the first main phase of the turn is a precombat main phase."
    ///
    /// [CR 505.1a]: https://yawgatog.com/resources/magic-rules/#R5051a
    pub fn take_planned_step(&mut self) -> Option<TurnStep> {
        let plan = &mut self.turn_plan;
        let entry = *plan.remaining.as_slice().first()?;
        plan.remaining.remove(0);
        plan.ordinal += 1;
        if entry.starts_phase {
            plan.phase = plan.ordinal;
        }
        let step = main_phase_name(entry.name(), plan.has_played(TurnStep::PrecombatMain));
        let repeated = plan.has_played(step);
        plan.played &= !TurnPlan::<L>::REPEATED;
        plan.played |= 1 << step.index();
        if repeated {
            plan.played |= TurnPlan::<L>::REPEATED;
        }
        if step == TurnStep::Draw {
            self.trackers
                .set_count(crate::trackers::Count::DrewInDrawStep, self.turn.active, 0);
        }
        Some(step)
    }

    /// Records a cleanup step the rules repeat after giving players priority.
    pub fn repeat_cleanup(&mut self) {
        self.turn_plan.ordinal += 1;
        if self.has_turn_plan() {
            self.turn_plan.played |= TurnPlan::<L>::REPEATED;
        }
    }

    /// Starts the first cleanup of an ordinary turn.
    pub fn reset_cleanup(&mut self) {
        self.turn_plan.ordinal = 0;
    }

    /// Inserts one step in the anchor's phase. An after-anchor can be the current step.
    /// Otherwise the first future step of that name is the anchor.
    ///
    /// Under [CR 500.9], "If multiple extra steps are created after the same step, the
    /// most recently created step will occur first."
    ///
    /// [CR 500.9]: https://yawgatog.com/resources/magic-rules/#R5009
    pub fn insert_step(
        &mut self,
        anchor: TurnStep,
        placement: Placement,
        step: TurnStep,
    ) -> Result<Option<Event<L>>, Full> {
        assert!(self.has_turn_plan());
        assert_eq!(TurnPhase::of(anchor), TurnPhase::of(step));
        let plan = &mut self.turn_plan;
        let index =
            if placement == Placement::After && plan.ordinal != 0 && self.turn.step == anchor {
                0
            } else if let Some(index) = plan
                .remaining
                .iter()
                .position(|entry| entry.name() == anchor)
            {
                index + usize::from(placement == Placement::After)
            } else {
                return Ok(None);
            };
        let starts =
            placement == Placement::Before && plan.remaining.as_slice()[index].starts_phase;
        plan.insert(index, &[step], starts)?;
        if starts {
            plan.remaining.as_mut_slice()[index + 1].starts_phase = false;
        }
        Ok(Some(Event::TurnPlanChanged(TurnChange::InsertStep(step))))
    }

    /// Inserts a phase at the boundary of the named phase. An after-anchor can be
    /// the current phase. Otherwise the first future phase of that kind is the anchor.
    ///
    /// Under [CR 500.8], "If multiple extra phases are created after the same phase, the
    /// most recently created phase will occur first."
    ///
    /// [CR 500.8]: https://yawgatog.com/resources/magic-rules/#R5008
    pub fn insert_phase(
        &mut self,
        anchor: TurnPhase,
        placement: Placement,
        phase: TurnPhase,
    ) -> Result<Option<Event<L>>, Full> {
        assert!(self.has_turn_plan());
        let plan = &mut self.turn_plan;
        let index = if placement == Placement::After
            && plan.ordinal != 0
            && TurnPhase::of(self.turn.step) == anchor
        {
            plan.phase_end(0)
        } else if let Some(index) = plan
            .remaining
            .iter()
            .position(|entry| entry.starts_phase && TurnPhase::of(entry.name()) == anchor)
        {
            match placement {
                Placement::Before => index,
                Placement::After => plan.phase_end(index + 1),
            }
        } else {
            return Ok(None);
        };
        plan.insert(index, phase.steps(), true)?;
        Ok(Some(Event::TurnPlanChanged(TurnChange::InsertPhase(phase))))
    }

    /// Adds `phases` in order after the current phase, if it is `after`.
    ///
    /// Under [CR 500.8], "They do this by adding the phases directly after the specified phase."
    /// A later insertion at the same boundary precedes the entire earlier insertion.
    /// Checks that the whole sequence fits before changing the plan.
    ///
    /// [CR 500.8]: https://yawgatog.com/resources/magic-rules/#R5008
    pub fn insert_phases_after_current(
        &mut self,
        after: TurnPhase,
        phases: &[TurnPhase],
    ) -> Result<bool, Full> {
        if phases.is_empty() || TurnPhase::of(self.turn.step) != after {
            return Ok(false);
        }
        assert!(self.has_turn_plan());
        let plan = &mut self.turn_plan;
        let added = phases.iter().try_fold(0usize, |count, phase| {
            count.checked_add(phase.steps().len()).ok_or(Full)
        })?;
        if added > L::Queue::<PlannedStep, CAPACITY>::CAPACITY - plan.remaining.len() {
            return Err(Full);
        }
        plan.next_id
            .checked_add(u32::try_from(added).map_err(|_| Full)?)
            .ok_or(Full)?;
        let mut index = plan.phase_end(0);
        for phase in phases {
            let steps = phase.steps();
            plan.insert(index, steps, true)?;
            index += steps.len();
        }
        Ok(true)
    }

    /// Removes the first remaining step of this name.
    ///
    /// Under [CR 500.11], "To skip a step, phase, or turn is to proceed past it as though
    /// it didn’t exist."
    ///
    /// [CR 500.11]: https://yawgatog.com/resources/magic-rules/#R50011
    pub fn remove_step(&mut self, step: TurnStep) -> Option<Event<L>> {
        let plan = &mut self.turn_plan;
        let index = plan
            .remaining
            .iter()
            .position(|entry| entry.name() == step)?;
        plan.remove(index);
        Some(Event::TurnPlanChanged(TurnChange::RemoveStep(step)))
    }

    /// Removes the first remaining phase of this kind.
    pub fn remove_phase(&mut self, phase: TurnPhase) -> Option<Event<L>> {
        let plan = &mut self.turn_plan;
        let index = plan
            .remaining
            .iter()
            .position(|entry| entry.starts_phase && TurnPhase::of(entry.name()) == phase)?;
        let end = plan.phase_end(index + 1);
        for _ in index..end {
            plan.remaining.remove(index);
        }
        Some(Event::TurnPlanChanged(TurnChange::RemovePhase(phase)))
    }

    /// Leaves a single cleanup step in the queue (see [CR 725.1d]):
    /// "The game skips straight to the cleanup step".
    ///
    /// [CR 725.1d]: https://yawgatog.com/resources/magic-rules/#R7251d
    pub fn cut_to_cleanup(&mut self) -> Result<Event<L>, Full> {
        assert!(self.has_turn_plan());
        self.turn_plan.next_id.checked_add(1).ok_or(Full)?;
        self.turn_plan.remaining.clear();
        self.turn_plan.insert(0, &[TurnStep::Cleanup], true)?;
        Ok(Event::TurnPlanChanged(TurnChange::CutToCleanup))
    }
}

/// The first main phase actually played is precombat, even if the plan was edited.
fn main_phase_name(step: TurnStep, had_main: bool) -> TurnStep {
    if TurnPhase::of(step) == TurnPhase::Main {
        if had_main {
            TurnStep::PostcombatMain
        } else {
            TurnStep::PrecombatMain
        }
    } else {
        step
    }
}
