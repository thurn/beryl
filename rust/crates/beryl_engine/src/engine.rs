//! The engine loop: `next` advances to the next decision, `apply` consumes one answer
//! (architecture §3, §7).

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, Types};
use crate::decision::{
    Answers, Ask, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Refusal, StallReason,
    Step,
};
use crate::event::{self, Entry, Event, Reason, Sink};
use crate::fixed::{Fixed, Full};
use crate::ids::{Seat, Slot};
use crate::observation::Observation;
use crate::profile::{Profile, Stops};
use crate::state::{Pending, State, TurnStep, Zone};
use crate::table::GameTable;
use crate::{cast, combat, face_down, layers, mana, resolve, sba, start, trigger, turn, zone};

/// Per-thread working memory, never copied with the state: the event sink, the counts, and
/// the spare state transactional replay runs bodies on (§7.3), allocated once so the hot
/// path never allocates.
#[derive(Debug)]
pub struct Scratch {
    pub counts: Counts,
    pub events: Sink,
    spare: Box<State>,
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            counts: Counts::default(),
            events: Sink::default(),
            spare: Box::new(State::empty(Profile::EXACT)),
        }
    }
}

impl Scratch {
    /// Working memory whose sink drops events, as search wants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Working memory whose sink records every executed event, for the UI and replays.
    pub fn recording() -> Self {
        Self {
            events: Sink::recording(),
            ..Self::default()
        }
    }

    /// The spare state, a copy of `s`, and the sink, for a run of a body on a copy.
    pub(crate) fn copy_to_spare(&mut self, s: &State) -> (&mut State, &mut Sink) {
        *self.spare = *s;
        self.counts.copied();
        (&mut self.spare, &mut self.events)
    }
}

/// The position a player sees the open decision in. That is the state itself, except while
/// a resolution is paused (§7.3): the committed state is then the one before the resolution
/// began, so the paused body is replayed into scratch up to its question.
pub fn view<'a>(s: &'a State, t: &GameTable, x: &'a mut Scratch) -> &'a State {
    if matches!(s.pending, Pending::Resolving(_))
        && let Ok(paused) = resolve::replay_to_question(s, t, x, None)
    {
        return paused;
    }
    s
}

/// `player` concedes, ending the game, unless it is over already
/// ([CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a)): "A player can concede the
/// game at any time. A player who concedes leaves the game immediately. That player loses the
/// game." Their opponent, the one player left, wins
/// ([CR 104.2a](https://yawgatog.com/resources/magic-rules/#R1042a)): "A player still in the
/// game wins the game if that player's opponents have all left the game."
pub fn concede(s: &mut State, x: &mut Scratch, player: Seat) {
    if s.outcome.is_none() {
        sba::end_game(s, &mut x.events, player);
    }
}

/// The events a paused resolution's body executes on its way to its question (§7.3), which
/// [`view`]'s position shows done: the card a loot draws before it asks for the discard, the
/// card an explore reveals before it asks where it goes. None while no resolution is paused.
/// They are executed again, and so emitted, once the answers let the body finish.
pub fn paused_events(s: &State, t: &GameTable, x: &mut Scratch) -> Vec<Event> {
    let mut seen = Vec::new();
    if matches!(s.pending, Pending::Resolving(_)) {
        let _ = resolve::replay_to_question(s, t, x, Some(&mut seen));
    }
    seen
}

/// What `viewer` sees of the open decision: [`view`]'s position, rooted at the committed
/// state `s` so a search can play on from it.
pub fn observe<'a>(
    s: &'a State,
    t: &GameTable,
    x: &'a mut Scratch,
    viewer: Seat,
) -> Observation<'a> {
    Observation::paused(view(s, t, x), s, viewer)
}

/// Deterministic counts of the engine's logical operations, the stable performance signal CI
/// gates on with tolerance bands (architecture §17.4), summed over every game the scratch
/// serves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Decisions `next` returned.
    pub decisions: u64,
    /// Decisions `next` settled itself: one candidate, or a priority pass away from a stop.
    pub silent: u64,
    /// State-based action checks.
    pub state_based_checks: u64,
    /// Whole-state copies: the engine's, for transactional replay (§7.3), and a searcher's,
    /// which it counts itself with [`Counts::copied`].
    pub state_copies: u64,
    /// Recomputes of the derived characteristics (§9).
    pub layer_recomputes: u64,
    /// Runs of the mana payment solver (§10).
    pub solver_runs: u64,
    /// Events executed (§8), whether or not the sink keeps them.
    pub events: u64,
}

impl Counts {
    /// Counts a copy of a whole state that a caller of the engine made.
    pub fn copied(&mut self) {
        self.state_copies += 1;
    }
}

impl Scratch {
    /// Moves the work the sink counted into the counts.
    fn settle_counts(&mut self) {
        let work = std::mem::take(&mut self.events.work);
        self.counts.events += work.events;
        self.counts.layer_recomputes += work.layer_recomputes;
        self.counts.solver_runs += work.solver_runs;
    }
}

/// Advances the game until a player must decide or the game is over.
///
/// What the exact profile settles itself (§7.2): a decision with one candidate is applied
/// silently, so a player whose only legal action is to pass passes silently. Under smart stops
/// a player away from a stop passes silently too (§15).
///
/// [CR 704.3](https://yawgatog.com/resources/magic-rules/#R7043): "If any state-based actions
/// are performed as a result of a check, the check is repeated; otherwise all triggered
/// abilities that are waiting to be put on the stack are put on the stack, then the check is
/// repeated." Waiting triggers come before priority as decisions of their own, one pick at a
/// time ([`DecisionKind::TriggerOrder`]).
pub fn next(s: &mut State, t: &GameTable, x: &mut Scratch, a: &mut dyn Answers) -> Step {
    let step = advance::<false>(s, t, x, a, (s.turn.number, s.turn.step));
    x.settle_counts();
    step.expect("`next` stops only at a decision or the end of the game")
}

/// Advances the game as [`next`] does, but returns `None` rather than go on once the turn is
/// at a step other than `seen`, a turn number and step: so a caller that shows the game (the
/// app's sessions) can show each step as it begins, and stop the game there. Calling it again
/// with the new step goes on exactly as `next` would have; the profile, and so what is a
/// decision, is the state's own.
///
/// A step is seen once its turn-based actions are done
/// ([CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a)): "The active player
/// receives priority at the beginning of most steps and phases, after any turn-based actions
/// (such as drawing a card during the draw step; see rule 703) have been dealt with". The
/// untap step is never seen on its own, as it ends at once
/// ([CR 502.4](https://yawgatog.com/resources/magic-rules/#R5024)): "No player receives
/// priority during the untap step".
pub fn next_in_step(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
    seen: (u16, TurnStep),
) -> Option<Step> {
    let step = advance::<true>(s, t, x, a, seen);
    x.settle_counts();
    step
}

/// The engine loop; with `STEPS`, it returns `None` once the turn is past `seen`.
fn advance<const STEPS: bool>(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
    seen: (u16, TurnStep),
) -> Option<Step> {
    // Whether a state-based check found nothing and priority has only been passed since,
    // from one player to the other: a pass changes nothing the check reads, so the next
    // check would find nothing again and is skipped, though still counted.
    let mut settled = false;
    loop {
        let unchanged = std::mem::take(&mut settled);
        if let Some(outcome) = s.outcome {
            return Some(Step::Over(outcome));
        }
        layers::refresh(s, t, &mut x.events);
        if combat::in_combat(s.turn.step) {
            combat::prune(s);
        }
        if STEPS && (s.turn.number, s.turn.step) != seen {
            return None;
        }
        if s.pending == Pending::None {
            // CR 117.5: "Each time a player would get priority, the game first performs all
            // applicable state-based actions as a single event".
            x.counts.state_based_checks += 1;
            match if unchanged {
                Ok(false)
            } else {
                sba::check(s, t, &mut x.events)
            } {
                Ok(true) => {
                    s.turn.cleanup_check = false;
                    continue;
                }
                Ok(false) => {}
                Err(Full) => {
                    stall(s, StallReason::Capacity);
                    continue;
                }
            }
            if s.turn.cleanup_check {
                s.turn.cleanup_check = false;
                // CR 514.3a: with no state-based actions and no waiting triggers, no player
                // receives priority and the turn ends.
                if s.triggers.is_empty() {
                    if turn::begin_turn(s, t, &mut x.events).is_err() {
                        stall(s, StallReason::Capacity);
                    }
                    continue;
                }
            }
        }
        if passes_silently(s) {
            x.counts.silent += 1;
            let passes = s.turn.passes;
            if perform_priority(s, t, x, a, s.turn.priority, Opt::Pass).is_err() {
                stall(s, StallReason::Capacity);
            }
            settled = handed_over(s, passes);
            continue;
        }
        let Ok(d) = decision(s, t, &mut x.events) else {
            stall(s, StallReason::Capacity);
            continue;
        };
        debug_assert!(!d.is_empty());
        let priority = d.kind == DecisionKind::Priority;
        let forced = d.len() == 1
            && (priority || s.profile.stops != Stops::Forced)
            && !s.profile.asks_lone(s, &d);
        let silent = (forced || (priority && !s.profile.stops_for(s, d.player)))
            && !(priority && s.profile.asks_pass(s.turn, d.player));
        if silent {
            x.counts.silent += 1;
            let passes = s.turn.passes;
            perform_or_stall(s, t, x, a, &d, Choice(0));
            // A priority decision's first candidate is passing.
            settled = priority && handed_over(s, passes);
            continue;
        }
        if s.budgets.decisions >= s.profile.limits.decisions_per_turn {
            stall(s, StallReason::DecisionsPerTurn);
            continue;
        }
        s.budgets.decisions += 1;
        x.counts.decisions += 1;
        return Some(Step::Decide(d));
    }
}

/// Whether the pass just made, with `passes` passes in succession before it, only handed
/// priority to the other player
/// ([CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d)): "Then the next player
/// in turn order receives priority." The first of two passes in succession resolves nothing
/// and ends no step.
fn handed_over(s: &State, passes: u8) -> bool {
    passes == 0 && s.turn.passes == 1 && s.outcome.is_none() && s.pending == Pending::None
}

/// Whether the open decision is priority for a player away from a stop, who passes silently
/// whatever else they could do, so `next` passes without listing their options.
fn passes_silently(s: &State) -> bool {
    s.pending == Pending::None
        && s.triggers.is_empty()
        && !s.profile.stops_for(s, s.turn.priority)
        && !s.profile.breaks_at(s.turn)
}

/// Applies an answer to the decision `next` returned for this state.
pub fn apply(s: &mut State, t: &GameTable, x: &mut Scratch, a: &mut dyn Answers, c: Choice) {
    debug_assert!(s.outcome.is_none(), "apply after the game ended");
    layers::refresh(s, t, &mut x.events);
    match decision(s, t, &mut x.events) {
        Ok(d) => {
            debug_assert!(usize::from(c.0) < d.len(), "choice out of range");
            perform_or_stall(s, t, x, a, &d, c);
        }
        Err(Full) => stall(s, StallReason::Capacity),
    }
    x.settle_counts();
}

/// Applies an answer to `d` as [`apply`] does, without building the decision again: `d` must
/// be the decision `next` returned for this state, unchanged since, which debug builds check.
/// The hot loops of search, which hold that decision, save rebuilding every option of it.
pub fn apply_decided(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
    d: &Decision,
    c: Choice,
) {
    debug_assert!(s.outcome.is_none(), "apply after the game ended");
    layers::refresh(s, t, &mut x.events);
    #[cfg(debug_assertions)]
    {
        let work = x.events.work;
        let built = decision(s, t, &mut x.events);
        x.events.work = work;
        debug_assert_eq!(built, Ok(*d), "a decision other than the open one");
    }
    debug_assert!(usize::from(c.0) < d.len(), "choice out of range");
    perform_or_stall(s, t, x, a, d, c);
    x.settle_counts();
}

#[cold]
fn stall(s: &mut State, reason: StallReason) {
    s.outcome = Some(Outcome::Stalled(reason));
}

fn perform_or_stall(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
    d: &Decision,
    c: Choice,
) {
    if perform(s, t, x, a, d, c).is_err() {
        stall(s, StallReason::Capacity);
    }
}

/// The open decision, a pure function of the state, so `apply` recomputes rather than
/// remembers it. The sink only counts the payment solver's runs.
pub(crate) fn decision(s: &State, t: &GameTable, sink: &mut Sink) -> Result<Decision, Full> {
    // Each arm returns its result as it is, so a decision is built once, in place.
    match s.pending {
        Pending::None => match trigger::next_to_stack(s) {
            Some((player, triggers)) => {
                let mut list = Fixed::new();
                for &i in &triggers {
                    list.push(Opt::Trigger(i))?;
                }
                Ok(Decision {
                    player,
                    kind: DecisionKind::TriggerOrder,
                    candidates: Candidates::Options(list),
                    ask: Ask::Plain,
                })
            }
            None => priority_decision(s, t),
        },
        Pending::PlayOrDraw { chooser } => {
            options(chooser, DecisionKind::PlayOrDraw, &[Opt::Play, Opt::Draw])
        }
        Pending::Mulligans => match start::mulligan_question(s) {
            (p, true) => Ok(objects(
                p,
                DecisionKind::PutOnBottom,
                collapse(s, turn::hand(s, p)),
            )),
            (p, false) => options(p, DecisionKind::KeepOrMulligan, &[Opt::Keep, Opt::Mulligan]),
        },
        Pending::Discard => {
            let p = s.turn.active;
            let count = zone::hand_size(s, p).saturating_sub(turn::MAX_HAND_SIZE);
            Ok(Decision {
                ask: Ask::Cleanup {
                    count: u8::try_from(count).unwrap_or(u8::MAX),
                },
                ..objects(p, DecisionKind::Discard, collapse(s, turn::hand(s, p)))
            })
        }
        Pending::LegendRule { kept } => {
            let (p, group) =
                sba::unresolved_legends(s, t, kept).expect("a legend-rule group without a keeper");
            Ok(objects(p, DecisionKind::LegendRule, collapse(s, group)))
        }
        Pending::EnterPayingLife { slot, life } => Ok(Decision {
            ask: Ask::PayLife { land: slot, life },
            ..options(
                s.object(slot).owner,
                DecisionKind::PayLife,
                &[Opt::Yes, Opt::No],
            )?
        }),
        Pending::DeclareAttackers { attacker } => combat::attack_decision(s, attacker),
        Pending::DeclareBlockers { blocker } => combat::block_decision(s, blocker),
        Pending::CombatDamage { done, lethal } => combat::division_decision(s, done, lethal),
        Pending::Casting(c) => cast::decision(s, t, sink, &c),
        Pending::TurningFaceUp { slot, remaining } => {
            face_down::decision(s, t, sink, slot, remaining)
        }
        Pending::Resolving(r) => Ok(r.question),
        Pending::CastOffer {
            object,
            player,
            free,
        } => Ok(Decision {
            player,
            kind: DecisionKind::CastOffer,
            candidates: Candidates::Options(cast::offer_options(s, t, object, player, free)?),
            ask: Ask::Plain,
        }),
    }
}

fn objects(player: Seat, kind: DecisionKind, candidates: Bitboard) -> Decision {
    Decision {
        player,
        kind,
        candidates: Candidates::Objects(candidates),
        ask: Ask::Plain,
    }
}

fn options(player: Seat, kind: DecisionKind, opts: &[Opt]) -> Result<Decision, Full> {
    let mut list = Fixed::new();
    for &o in opts {
        list.push(o)?;
    }
    Ok(Decision {
        player,
        kind,
        candidates: Candidates::Options(list),
        ask: Ask::Plain,
    })
}

/// The priority holder's legal actions, passing first.
///
/// [CR 117.1](https://yawgatog.com/resources/magic-rules/#R1171): "The player with priority
/// may cast spells, activate abilities, and take special actions."
fn priority_decision(s: &State, t: &GameTable) -> Result<Decision, Full> {
    let player = s.turn.priority;
    // Built in place: the list is most of a decision's size.
    let mut d = Decision {
        player,
        kind: DecisionKind::Priority,
        candidates: Candidates::Options(Fixed::new()),
        ask: Ask::Plain,
    };
    let Candidates::Options(list) = &mut d.candidates else {
        unreachable!("built with options");
    };
    list.push(Opt::Pass)?;
    if turn::may_play_land(s, player) {
        let lands: Bitboard = turn::hand(s, player)
            .iter()
            .filter(|&slot| land_face(s, t, slot).is_some())
            .collect();
        for slot in collapse(s, lands).iter() {
            list.push(Opt::PlayLand(slot))?;
        }
    }
    cast::priority_options(s, t, player, list)?;
    face_down::face_up_options(s, t, player, list)?;
    Ok(d)
}

/// The first face of a card that is a land, which is the face it is played with
/// ([CR 712.12](https://yawgatog.com/resources/magic-rules/#R71212)): "A player playing a
/// modal double-faced card ... as a land chooses one of its faces that's a land before putting
/// it onto the battlefield." The cube's modal double-faced cards have one land face at most.
fn land_face(s: &State, t: &GameTable, slot: Slot) -> Option<FaceIx> {
    let def = t.def(s.object(slot).card);
    def.face_ixs()
        .find(|&f| def.face(f).types.intersects(Types::LAND))
}

/// Why the viewer may neither cast the card in `slot`, from their hand, nor play it as a land
/// at their priority now, for a client to say why it isn't offered: the first rule the most
/// nearly legal way of playing it breaks. None if one way is legal. It reads what their
/// priority's candidates do, so it tells them nothing they don't know.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "A player who has priority
/// may play a land card from their hand during a main phase of their turn when the stack is
/// empty." [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can
/// normally play one land during their turn".
pub fn refusal(obs: Observation<'_>, t: &GameTable, slot: Slot) -> Option<Refusal> {
    let (s, player) = (obs.shown(), obs.viewer());
    let land = match land_face(s, t, slot) {
        None => None,
        Some(_) if turn::may_play_land(s, player) => return None,
        Some(_) if cast::sorcery_timing(s, player) => Some(Refusal::LandPlayed),
        Some(_) => Some(Refusal::Timing { by: None }),
    };
    // A full capacity leaves the reason unknown.
    cast::refusal(s, t, player, slot, land).ok().flatten()
}

/// Answers a question of the casting machine, a paused resolution or a resolution's offer
/// to cast, if one is open.
fn perform_machine(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
    d: &Decision,
    c: Choice,
) -> Option<Result<(), Full>> {
    Some(match s.pending {
        Pending::Resolving(mut r) => r.record.push(c.0).and_then(|()| {
            s.pending = Pending::Resolving(r);
            resolve::resolve(s, t, x, a)
        }),
        Pending::Casting(casting) => cast::perform(
            s,
            t,
            &mut x.events,
            casting,
            d.candidates.option(c),
            d.candidates.object(c),
        ),
        Pending::TurningFaceUp { slot, remaining } => face_down::perform(
            s,
            t,
            &mut x.events,
            slot,
            remaining,
            d.candidates.option(c).expect("a payment"),
        ),
        Pending::CastOffer { player, free, .. } => {
            s.pending = Pending::None;
            match d.candidates.option(c).expect("an option choice") {
                Opt::Pass => Ok(()),
                cast_option => {
                    cast::begin_cast(s, t, &mut x.events, player, cast_option, Some(free))
                }
            }
        }
        _ => return None,
    })
}

/// Takes a priority action.
fn perform_priority(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
    player: Seat,
    option: Opt,
) -> Result<(), Full> {
    match option {
        Opt::PlayLand(slot) => play_land(s, t, &mut x.events, player, slot),
        cast_option @ Opt::Cast { .. } => {
            cast::begin_cast(s, t, &mut x.events, player, cast_option, None)
        }
        Opt::Activate { slot, ability } => {
            cast::begin_activate(s, t, &mut x.events, player, slot, ability)
        }
        Opt::TurnFaceUp(slot) => {
            face_down::begin_turning_face_up(s, t, &mut x.events, player, slot)
        }
        Opt::Mana { slot, color } => {
            mana::activate(s, t, &mut x.events, slot, color)?;
            // CR 117.3c: "If a player has priority when they ... activate an ability ...,
            // that player receives priority afterward."
            s.turn.passes = 0;
            Ok(())
        }
        _ => pass(s, t, x, a),
    }
}

fn perform(
    s: &mut State,
    t: &GameTable,
    x: &mut Scratch,
    a: &mut dyn Answers,
    d: &Decision,
    c: Choice,
) -> Result<(), Full> {
    let object = || d.candidates.object(c).expect("an object choice");
    let option = || d.candidates.option(c).expect("an option choice");
    if let Some(done) = perform_machine(s, t, x, a, d, c) {
        return done;
    }
    if d.kind == DecisionKind::Priority {
        return perform_priority(s, t, x, a, d.player, option());
    }
    let sink = &mut x.events;
    match d.kind {
        DecisionKind::Priority => unreachable!("handled above"),
        DecisionKind::TriggerOrder => match option() {
            Opt::Trigger(i) => trigger::put_on_stack(s, t, sink, i),
            _ => unreachable!("a trigger-order decision offers only triggers"),
        },
        DecisionKind::PayLife => {
            let Pending::EnterPayingLife { slot, .. } = s.pending else {
                unreachable!("a pay-life decision without its pending state");
            };
            s.pending = Pending::None;
            enter_from_hand(s, t, sink, slot, option() == Opt::Yes)
        }
        DecisionKind::Attacker | DecisionKind::AttackTarget => combat::attack(s, t, sink, option()),
        DecisionKind::Blocker => {
            match option() {
                Opt::Block(slot) => combat::block_with(s, slot),
                _ => combat::finish_blocks(s),
            }
            Ok(())
        }
        DecisionKind::BlockTarget => {
            combat::block_attacker(s, object());
            Ok(())
        }
        DecisionKind::DamageDivision => {
            let Pending::CombatDamage { done, lethal } = s.pending else {
                unreachable!("a damage division without its pending state");
            };
            combat::divide(s, t, sink, done, lethal, option())
        }
        DecisionKind::X
        | DecisionKind::TargetCount
        | DecisionKind::Target
        | DecisionKind::PaymentPlan
        | DecisionKind::ManaPayment
        | DecisionKind::Crew
        | DecisionKind::CastOffer
        | DecisionKind::YesNo
        | DecisionKind::ChooseObject
        | DecisionKind::ChooseUpTo
        | DecisionKind::Waterbend
        | DecisionKind::ExileCost => unreachable!("answered through the pending state"),
        DecisionKind::PlayOrDraw => {
            let starting = if option() == Opt::Play {
                d.player
            } else {
                d.player.opponent()
            };
            start::set_starting_player(s, t, sink, starting)
        }
        DecisionKind::KeepOrMulligan => {
            start::declare(s, t, sink, d.player, option() == Opt::Mulligan)
        }
        DecisionKind::PutOnBottom => start::put_on_bottom(s, t, sink, d.player, object()),
        DecisionKind::Discard => turn::cleanup_discard(s, t, sink, object()),
        DecisionKind::LegendRule => {
            let Pending::LegendRule { mut kept } = s.pending else {
                unreachable!("a legend-rule decision without its pending state");
            };
            kept.insert(object());
            if sba::unresolved_legends(s, t, kept).is_some() {
                s.pending = Pending::LegendRule { kept };
                Ok(())
            } else {
                sba::apply_legend_rule(s, t, sink, kept)
            }
        }
    }
}

/// Passes priority.
///
/// [CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d): "If a player has priority
/// and chooses not to take any actions, that player passes. ... Then the next player in turn
/// order receives priority."
///
/// [CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174): "If all players pass in
/// succession (that is, if all players pass without taking any actions in between passing),
/// the spell or ability on top of the stack resolves or, if the stack is empty, the phase or
/// step ends."
///
/// [CR 117.3b](https://yawgatog.com/resources/magic-rules/#R1173b): "The active player
/// receives priority after a spell or ability (other than a mana ability) resolves."
fn pass(s: &mut State, t: &GameTable, x: &mut Scratch, a: &mut dyn Answers) -> Result<(), Full> {
    s.turn.passes += 1;
    if s.turn.passes < 2 {
        s.turn.priority = s.turn.priority.opponent();
        return Ok(());
    }
    if s.stack.is_empty() {
        turn::end_step(s, t, &mut x.events)
    } else {
        // The passes are spent: a resolution that pauses for a question waits over this
        // state, and priority starts afresh once it resolves.
        s.turn.passes = 0;
        resolve::resolve(s, t, x, a)
    }
}

/// Plays a land: a special action that puts it onto the battlefield. A land that enters
/// tapped unless its controller pays life asks them first.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "Playing a land is a
/// special action; it doesn't use the stack (see rule 116). Rather, the player simply puts the
/// land onto the battlefield."
///
/// [CR 116.3](https://yawgatog.com/resources/magic-rules/#R1163): "If a player takes a special
/// action, that player receives priority afterward."
///
/// [CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194): "If a cost or effect allows
/// a player to pay an amount of life greater than 0, the player may do so only if their life
/// total is greater than or equal to the amount of the payment."
fn play_land(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    slot: Slot,
) -> Result<(), Full> {
    s.player_mut(player).lands_played += 1;
    s.turn.priority = player;
    s.turn.passes = 0;
    let face = land_face(s, t, slot).expect("a land face");
    match event::life_to_enter(t, s.object(slot).card, face) {
        Some(life) if s.player(player).life >= i16::from(life) => {
            s.pending = Pending::EnterPayingLife { slot, life };
            Ok(())
        }
        _ => enter_from_hand(s, t, sink, slot, false),
    }
}

fn enter_from_hand(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    slot: Slot,
    pay_life: bool,
) -> Result<(), Full> {
    event::move_batch(
        s,
        t,
        sink,
        Bitboard::single(slot),
        Zone::Battlefield,
        Reason::Play,
        Entry {
            pay_life,
            face: land_face(s, t, slot).expect("a land face"),
            ..Entry::default()
        },
    )
}

/// Collapses interchangeable objects to one candidate each, the lowest slot (§7.2): objects
/// that differ only in their incarnation and timestamp lead to the same game. Everything
/// else counts: the face up, the owner (a stolen Mana Crypt goes to another graveyard than
/// your own), copy, link and chosen value as much as card, counters and flags. An object a
/// pending reference names, a target on the stack or a cast permission, is never collapsed
/// with another; nor is one that combat relates another object to, since its signature
/// doesn't show what attacks or blocks it.
///
/// Nor is a face-down permanent: whether two are the same card is hidden from all but their
/// controller ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "You can't look
/// at face-down cards in any other zone or face-down spells or permanents controlled by
/// another player." Offering one of them, not both, would tell the other player.
pub(crate) fn collapse(s: &State, candidates: Bitboard) -> Bitboard {
    if candidates.count() < 2 {
        return candidates;
    }
    collapse_ordered(s, candidates.iter())
}

/// [`collapse`] keeping the first of interchangeable objects in the order of `slots`
/// rather than the lowest slot.
pub(crate) fn collapse_in_order(s: &State, slots: &[Slot]) -> Bitboard {
    collapse_ordered(s, slots.iter().copied())
}

fn collapse_ordered(s: &State, slots: impl Iterator<Item = Slot>) -> Bitboard {
    let signature = |slot: Slot| zone::signature(s, slot);
    // Found only once two candidates are of one card: until then every one is kept, named by
    // a reference or not.
    let mut referenced = None;
    let mut kept = Bitboard::EMPTY;
    for slot in slots {
        // The signature holds the card, so objects of other cards are told apart without
        // building theirs.
        let card = s.object(slot).card;
        if s.object(slot).face == FaceIx::DOWN || !kept.iter().any(|k| s.object(k).card == card) {
            kept.insert(slot);
            continue;
        }
        let referenced =
            *referenced.get_or_insert_with(|| cast::referenced(s) | combat::related(s));
        let mut own = None;
        if referenced.contains(slot)
            || !kept.iter().any(|k| {
                !referenced.contains(k)
                    && s.object(k).card == card
                    && signature(k) == *own.get_or_insert_with(|| signature(slot))
            })
        {
            kept.insert(slot);
        }
    }
    kept
}
