//! The engine loop: `next` advances to the next decision, `apply` consumes one answer
//! (architecture §3, §7).

use std::cell::OnceCell;

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, Prompt, Purpose, Types};
use crate::decision::{
    Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Refusal, StallReason, Step,
};
use crate::event::{self, Entry, Event, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, index};
use crate::observation::Observation;
use crate::profile::{self, Stops};
#[cfg(debug_assertions)]
use crate::scratch::assert_sound;
pub use crate::scratch::{Counts, Scratch};
use crate::state::{Pending, State, TurnStep, Zone};
use crate::table::GameTable;
use crate::{cast, combat, face_down, layers, mana, resolve, sba, start, trigger, turn};

/// The position a player sees the open decision in. That is the state itself, except while
/// a resolution is paused (§7.3): the committed state is then the one before the resolution
/// began, so the paused body is replayed into scratch up to its question.
pub fn view<'a, L: Layout>(s: &'a State<L>, t: &GameTable, x: &'a mut Scratch<L>) -> &'a State<L> {
    if matches!(s.pending(), Pending::Resolving(_))
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
pub fn concede<L: Layout>(s: &mut State<L>, x: &mut Scratch<L>, player: Seat) {
    if s.outcome().is_none() {
        sba::end_game(s, &mut x.events, player, crate::event::LossCause::Concede);
    }
}

/// The events a paused resolution's body executes on its way to its question (§7.3), which
/// [`view`]'s position shows done: the card a loot draws before it asks for the discard, the
/// card an explore reveals before it asks where it goes. None while no resolution is paused.
/// They are executed again, and so emitted, once the answers let the body finish.
pub fn paused_events<L: Layout>(s: &State<L>, t: &GameTable, x: &mut Scratch<L>) -> Vec<Event<L>> {
    let mut seen = Vec::new();
    if matches!(s.pending(), Pending::Resolving(_)) {
        let _ = resolve::replay_to_question(s, t, x, Some(&mut seen));
    }
    seen
}

/// What `viewer` sees of the open decision: [`view`]'s position, rooted at the committed
/// state `s` so a search can play on from it.
pub fn observe<'a, L: Layout>(
    s: &'a State<L>,
    t: &GameTable,
    x: &'a mut Scratch<L>,
    viewer: Seat,
) -> Observation<'a, L> {
    Observation::paused(view(s, t, x), s, viewer)
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
pub fn next<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Step<L> {
    let step = advance::<false, L>(s, t, x, a, (s.turn().number(), s.turn().step()));
    x.settle_counts();
    #[cfg(debug_assertions)]
    assert_sound(s, t, x);
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
pub fn next_in_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    seen: (u16, TurnStep),
) -> Option<Step<L>> {
    let step = advance::<true, L>(s, t, x, a, seen);
    x.settle_counts();
    // Stopped at a new step, the game has yet to check state-based actions: a player who drew
    // from an empty library has not lost yet, so the invariants need not hold.
    #[cfg(debug_assertions)]
    if step.is_some() {
        assert_sound(s, t, x);
    }
    step
}

/// The engine loop; with `STEPS`, it returns `None` once the turn is past `seen`.
fn advance<const STEPS: bool, L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    seen: (u16, TurnStep),
) -> Option<Step<L>> {
    // Whether a state-based check found nothing and priority has only been passed since,
    // from one player to the other: a pass changes nothing the check reads, so the next
    // check would find nothing again and is skipped, though still counted.
    let mut settled = false;
    loop {
        let unchanged = std::mem::take(&mut settled);
        if let Some(outcome) = layers::over_or_refreshed(s, t, &mut x.events) {
            return Some(Step::Over(outcome));
        }
        if combat::in_combat(s.turn().step()) {
            combat::prune(s);
        }
        if STEPS && (s.turn().number(), s.turn().step()) != seen {
            return None;
        }
        if *s.pending() == Pending::None {
            // CR 117.5: "Each time a player would get priority, the game first performs all
            // applicable state-based actions as a single event".
            x.counts.state_based_checks += 1;
            match if unchanged {
                Ok(false)
            } else {
                sba::check(s, t, &mut x.events)
            } {
                Ok(true) => {
                    s.set_cleanup_check(false);
                    continue;
                }
                Ok(false) => {}
                Err(Full) => {
                    stall(s, StallReason::Capacity);
                    continue;
                }
            }
            if s.turn().cleanup_check() {
                s.set_cleanup_check(false);
                // CR 514.3a: with no state-based actions and no waiting triggers, no player
                // receives priority and the turn ends.
                if s.triggers().is_empty() {
                    stall_if_full(turn::begin_turn(s, t, &mut x.events), s);
                    continue;
                }
            }
        }
        if passes_silently(s) {
            x.counts.silent += 1;
            let passes = s.turn().passes();
            stall_if_full(
                perform_priority(s, t, x, a, s.turn().priority(), Opt::Pass),
                s,
            );
            settled = handed_over(s, passes);
            continue;
        }
        let Ok(d) = decision(s, t, &mut x.events) else {
            stall(s, StallReason::Capacity);
            continue;
        };
        debug_assert!(!d.is_empty());
        let priority = d.kind == DecisionKind::Priority;
        let forced = (d.len() == 1 || only_idle(s, t, &d))
            && (priority || s.profile().stops != Stops::Forced)
            && !profile::asks_lone(s.profile(), s, &d);
        let silent = (forced || (priority && !profile::stops_for(s.profile(), s, d.player)))
            && !(priority && s.profile().asks_pass(*s.turn(), d.player));
        if silent {
            x.counts.silent += 1;
            let passes = s.turn().passes();
            stall_if_full(perform(s, t, x, a, &d, Choice(0)), s);
            // A priority decision's first candidate is passing.
            settled = priority && handed_over(s, passes);
            continue;
        }
        if s.budgets().decisions >= s.profile().limits.decisions_per_turn {
            stall(s, StallReason::DecisionsPerTurn);
            continue;
        }
        s.count_decision();
        x.counts.decisions += 1;
        return Some(Step::Decide(d));
    }
}

/// Whether the pass just made, with `passes` passes in succession before it, only handed
/// priority to the other player
/// ([CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d)): "Then the next player
/// in turn order receives priority." The first of two passes in succession resolves nothing
/// and ends no step.
fn handed_over<L: Layout>(s: &State<L>, passes: u8) -> bool {
    passes == 0 && s.turn().passes() == 1 && s.outcome().is_none() && *s.pending() == Pending::None
}

/// Whether the open decision is priority for a player away from a stop, who passes silently
/// whatever else they could do, so `next` passes without listing their options.
fn passes_silently<L: Layout>(s: &State<L>) -> bool {
    *s.pending() == Pending::None
        && s.triggers().is_empty()
        && !profile::stops_for(s.profile(), s, s.turn().priority())
        && !s.profile().breaks_at(*s.turn())
}

/// Applies an answer to the decision `next` returned for this state.
pub fn apply<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    c: Choice,
) {
    debug_assert!(s.outcome().is_none(), "apply after the game ended");
    if layers::refresh(s, t, &mut x.events).is_err() {
        return stall(s, StallReason::Capacity);
    }
    #[cfg(debug_assertions)]
    assert_sound(s, t, x);
    match decision(s, t, &mut x.events) {
        Ok(d) => {
            debug_assert!(usize::from(c.0) < d.len(), "choice out of range");
            stall_if_full(perform(s, t, x, a, &d, c), s);
        }
        Err(Full) => stall(s, StallReason::Capacity),
    }
    x.settle_counts();
}

/// Applies an answer to `d` as [`apply`] does, without building the decision again: `d` must
/// be the decision `next` returned for this state, unchanged since, which debug builds check.
/// The hot loops of search, which hold that decision, save rebuilding every option of it.
pub fn apply_decided<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    d: &Decision<L>,
    c: Choice,
) {
    debug_assert!(s.outcome().is_none(), "apply after the game ended");
    if layers::refresh(s, t, &mut x.events).is_err() {
        return stall(s, StallReason::Capacity);
    }
    #[cfg(debug_assertions)]
    {
        assert_sound(s, t, x);
        let work = x.events.work;
        let built = decision(s, t, &mut x.events);
        x.events.work = work;
        debug_assert_eq!(built.as_ref(), Ok(d), "a decision other than the open one");
    }
    debug_assert!(usize::from(c.0) < d.len(), "choice out of range");
    stall_if_full(perform(s, t, x, a, d, c), s);
    x.settle_counts();
}

#[cold]
fn stall<L: Layout>(s: &mut State<L>, reason: StallReason) {
    s.end_game(Outcome::Stalled(reason));
}

/// Ends the game stalled if `done` ran out of a capacity (§12.3).
fn stall_if_full<L: Layout>(done: Result<(), Full>, s: &mut State<L>) {
    if done.is_err() {
        stall(s, StallReason::Capacity);
    }
}

/// The open decision, a pure function of the state, so `apply` recomputes rather than
/// remembers it. The sink only counts the payment solver's runs.
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<Decision<L>, Full> {
    // Each arm returns its result as it is, so a decision is built once, in place.
    match *s.pending() {
        Pending::None => match trigger::next_to_stack(s) {
            Some((player, triggers)) => {
                let mut list: L::Queue<Opt<L>, 64> = Default::default();
                for &i in triggers.iter() {
                    list.push(Opt::Trigger(i))?;
                }
                let kind = DecisionKind::TriggerOrder;
                let order = Prompt::new(Purpose::TriggerOrder);
                Ok(Decision::options(player, kind, list, order))
            }
            None => priority_decision(s, t),
        },
        Pending::PlayOrDraw { chooser } => options(
            chooser,
            DecisionKind::PlayOrDraw,
            &[Opt::Play, Opt::Draw],
            Prompt::new(Purpose::PlayOrDraw),
        ),
        Pending::Mulligans => match start::mulligan_question(s) {
            (p, true) => Ok(Decision::objects(
                p,
                DecisionKind::ChooseObject,
                collapse(s, turn::hand(s, p)),
                Prompt::new(Purpose::PutOnBottom).most(s.player(p).to_bottom()),
            )),
            // The question is about the hand to keep.
            (p, false) => Ok(options(
                p,
                DecisionKind::KeepOrMulligan,
                &[Opt::Keep, Opt::Mulligan],
                Prompt::new(Purpose::Mulligan),
            )?
            .about(turn::hand(s, p))),
        },
        Pending::Discard => {
            let p = s.turn().active();
            let count = s.hand_size(p).saturating_sub(turn::MAX_HAND_SIZE);
            Ok(Decision::objects(
                p,
                DecisionKind::ChooseObject,
                collapse(s, turn::hand(s, p)),
                Prompt::new(Purpose::Cleanup).most(u8::try_from(count).unwrap_or(u8::MAX)),
            ))
        }
        Pending::LegendRule { kept } => {
            let (p, group) =
                sba::unresolved_legends(s, t, kept).expect("a legend-rule group without a keeper");
            let keep = Prompt::new(Purpose::LegendRule).most(1);
            let kind = DecisionKind::ChooseObject;
            Ok(Decision::objects(p, kind, collapse(s, group), keep))
        }
        // The question is about the land entering, for this much life.
        Pending::EnterPayingLife { slot, life } => Ok(options(
            s.object(slot).owner(),
            DecisionKind::YesNo,
            &[Opt::Yes, Opt::No],
            Prompt::new(Purpose::PayLife).count(life),
        )?
        .about(Bitboard::single(slot))),
        Pending::DeclareAttackers { attacker } => combat::attack_decision(s, attacker),
        Pending::DeclareBlockers { blocker } => combat::block_decision(s, blocker),
        Pending::CombatDamage { done, lethal } => combat::division_decision(s, done, lethal),
        Pending::Casting(c) => cast::decision(s, t, sink, &c),
        Pending::TurningFaceUp { slot, remaining } => {
            face_down::decision(s, t, sink, slot, remaining)
        }
        Pending::Resolving(ref r) => Ok(r.question.clone()),
        Pending::CastOffer {
            object,
            player,
            free,
        } => Ok(Decision::options(
            player,
            DecisionKind::CastOffer,
            cast::offer_options(s, t, object, player, free)?,
            Prompt::new(Purpose::CastOffer),
        )
        // The question is about the card offered.
        .about(Bitboard::single(object.slot))),
    }
}

fn options<L: Layout>(
    player: Seat,
    kind: DecisionKind,
    opts: &[Opt<L>],
    prompt: Prompt,
) -> Result<Decision<L>, Full> {
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    for &o in opts {
        list.push(o)?;
    }
    Ok(Decision::options(player, kind, list, prompt))
}

/// The priority holder's legal actions, passing first.
///
/// [CR 117.1](https://yawgatog.com/resources/magic-rules/#R1171): "The player with priority
/// may cast spells, activate abilities, and take special actions."
fn priority_decision<L: Layout>(s: &State<L>, t: &GameTable) -> Result<Decision<L>, Full> {
    let player = s.turn().priority();
    // Built in place: the list is most of a decision's size.
    let mut d: Decision<L> = Decision {
        player,
        kind: DecisionKind::Priority,
        candidates: Candidates::Options(Default::default()),
        prompt: Prompt::new(Purpose::Priority),
        about: Bitboard::EMPTY,
    };
    let Candidates::Options(list) = &mut d.candidates else {
        unreachable!("built with options");
    };
    list.push(Opt::Pass)?;
    if turn::may_play_land(s, player) {
        let lands = turn::hand(s, player).filter(|slot| land_face(s, t, slot).is_some());
        // Candidates are listed in the candidate order, which a `Choice` counts in.
        for slot in s.in_candidate_order(collapse(s, lands)) {
            list.push(Opt::PlayLand(slot))?;
        }
    }
    cast::priority_options(s, t, player, list)?;
    face_down::face_up_options(s, t, player, list)?;
    // Last, as the options a full list leaves out: tapping ahead is never needed, the payment
    // taps for itself.
    if s.profile().floats_for(player) {
        mana::standalone_options(s, t, player, list);
    }
    Ok(d)
}

/// Whether `d` is a priority decision whose only action is passing, but for activating mana
/// abilities on their own: the engine answers it as a lone pass where it would one, as floating
/// mana that nothing else can spend now changes nothing
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "Each player's mana pool
/// empties at the end of each step and phase").
///
/// So is a priority whose other actions are activations that would do nothing, for the player
/// offered them quietly ([`crate::Profile::quiet_pointless`]): paying a cost for no effect is
/// legal ([CR 602.2](https://yawgatog.com/resources/magic-rules/#R6022)), but no reason to stop.
fn only_idle<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> bool {
    let floats = s.profile().floats_for(d.player);
    let quiet = s.profile().quiet_pointless == Some(d.player);
    if d.kind != DecisionKind::Priority || !(floats || quiet) {
        return false;
    }
    match &d.candidates {
        Candidates::Options(list) => list.iter().all(|o| match *o {
            Opt::Pass => true,
            Opt::Mana { .. } => floats,
            Opt::Activate { slot, ability } => quiet && cast::pointless(s, t, slot, ability),
            _ => false,
        }),
        Candidates::Objects(_) => false,
    }
}

/// The first face of a card that is a land, which is the face it is played with
/// ([CR 712.9](https://yawgatog.com/resources/magic-rules/#R7129)): "A player playing a
/// modal double-faced card ... as a land chooses one of its faces that's a land before putting
/// it onto the battlefield." The cube's modal double-faced cards have one land face at most.
fn land_face<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Option<FaceIx> {
    let def = s.card_def(t, slot)?;
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
pub fn refusal<L: Layout>(
    obs: Observation<'_, L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<Refusal<L>> {
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
/// to cast, if one is open, or one of the rules' own questions that a general kind asks: the
/// pending state says which question it is, where the kind says only the form of its answer.
fn perform_machine<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    d: &Decision<L>,
    c: Choice,
) -> Option<Result<(), Full>> {
    Some(match *s.pending() {
        Pending::Resolving(_) => s
            .record_answer(index::<L>(usize::from(c.0)))
            .and_then(|()| resolve::resolve(s, t, x, a)),
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
            s.settle_question();
            match d.candidates.option(c).expect("an option choice") {
                Opt::Pass => Ok(()),
                cast_option => {
                    cast::begin_cast(s, t, &mut x.events, player, cast_option, Some(free))
                }
            }
        }
        Pending::EnterPayingLife { slot, .. } => {
            s.settle_question();
            let pay = d.candidates.option(c) == Some(Opt::Yes);
            enter_from_hand(s, t, &mut x.events, slot, pay)
        }
        // The other question of a mulligan, keep or mulligan, has a kind of its own.
        Pending::Mulligans if d.kind == DecisionKind::ChooseObject => {
            let card = d.candidates.object(c).expect("an object choice");
            start::put_on_bottom(s, t, &mut x.events, d.player, card)
        }
        Pending::Discard => {
            let card = d.candidates.object(c).expect("an object choice");
            turn::cleanup_discard(s, t, &mut x.events, card)
        }
        Pending::LegendRule { mut kept } => {
            kept.insert(d.candidates.object(c).expect("an object choice"));
            if sba::unresolved_legends(s, t, kept).is_some() {
                s.ask(Pending::LegendRule { kept });
                Ok(())
            } else {
                sba::apply_legend_rule(s, t, &mut x.events, kept)
            }
        }
        _ => return None,
    })
}

/// Takes a priority action.
fn perform_priority<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    player: Seat,
    option: Opt<L>,
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
            s.reset_passes();
            Ok(())
        }
        _ => pass(s, t, x, a),
    }
}

fn perform<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    d: &Decision<L>,
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
            let Pending::CombatDamage { done, lethal } = *s.pending() else {
                unreachable!("a damage division without its pending state");
            };
            combat::divide(s, t, sink, done, lethal, option())
        }
        DecisionKind::X
        | DecisionKind::TargetCount
        | DecisionKind::Target
        | DecisionKind::PaymentPlan
        | DecisionKind::ManaPayment
        | DecisionKind::CastOffer
        | DecisionKind::YesNo
        | DecisionKind::ChooseObject
        | DecisionKind::ChooseUpTo => unreachable!("answered through the pending state"),
        DecisionKind::PlayOrDraw => {
            // Of two: with more players the chooser names who takes the first turn (CR 103.1).
            let starting = if option() == Opt::Play {
                d.player
            } else {
                s.next_player(d.player)
            };
            start::set_starting_player(s, t, sink, starting)
        }
        DecisionKind::KeepOrMulligan => {
            start::declare(s, t, sink, d.player, option() == Opt::Mulligan)
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
fn pass<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Result<(), Full> {
    if !s.pass_priority() {
        return Ok(());
    }
    if s.stack().is_empty() {
        turn::end_step(s, t, &mut x.events)
    } else {
        // The passes are spent: a resolution that pauses for a question waits over this
        // state, and priority starts afresh once it resolves.
        s.reset_passes();
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
fn play_land<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
) -> Result<(), Full> {
    s.count_land_played(player);
    s.give_priority(player);
    let face = land_face(s, t, slot).expect("a land face");
    match event::life_to_enter(t, s.copiable(slot).turned(face)) {
        Some(life) if s.player(player).life() >= i16::from(life) => {
            s.ask(Pending::EnterPayingLife { slot, life });
            Ok(())
        }
        _ => enter_from_hand(s, t, sink, slot, false),
    }
}

fn enter_from_hand<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
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
/// your own), copy, link and chosen value as much as card, counters and flags. An object
/// another record of the state names ([`State::named`]: a target on the stack, a blocked
/// attacker, what a card is exiled with) is never collapsed with another, since its
/// signature doesn't show what refers to it.
///
/// Nor is a face-down permanent: whether two are the same card is hidden from all but their
/// controller ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "You can't look
/// at face-down cards in any other zone or face-down spells or permanents controlled by
/// another player." Offering one of them, not both, would tell the other player.
///
/// Nor are two copies of a card that different players know: in a hidden zone
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)), "Library and hand are
/// hidden zones, even if all the cards in one such zone happen to be revealed", a copy the
/// other player has seen and one they haven't lead to different games, as playing the one they
/// know tells them nothing new and leaves them the other unknown.
pub(crate) fn collapse<L: Layout>(
    s: &State<L>,
    candidates: Bitboard<L::Slot>,
) -> Bitboard<L::Slot> {
    if candidates.count() < 2 {
        return candidates;
    }
    // Of interchangeable objects the candidate is the first in candidate order.
    collapse_ordered(s, s.in_candidate_order(candidates))
}

/// Whether two object slots are represented by one candidate (architecture §7.2). Read-only
/// payment previews use the same equivalence as real decisions, including named objects.
pub fn interchangeable<L: Layout>(s: &State<L>, a: L::Slot, b: L::Slot) -> bool {
    collapse(s, Bitboard::single(a) | Bitboard::single(b)).count() == 1
}

/// [`collapse`] keeping the first of interchangeable objects in the order of `slots`
/// rather than the lowest slot.
pub(crate) fn collapse_in_order<L: Layout>(
    s: &State<L>,
    slots: impl Iterator<Item = L::Slot>,
) -> Bitboard<L::Slot> {
    collapse_ordered(s, slots)
}

fn collapse_ordered<L: Layout>(
    s: &State<L>,
    slots: impl Iterator<Item = L::Slot>,
) -> Bitboard<L::Slot> {
    let signature = |slot: L::Slot| s.signature(slot);
    let known = |slot: L::Slot| {
        matches!(s.object(slot).zone(), Zone::Hand | Zone::Library).then(|| s.who_knows(slot))
    };
    // Found only once two candidates are of one card: until then every one is kept, named by
    // another record or not.
    let named = OnceCell::new();
    let mut kept = Bitboard::EMPTY;
    for slot in slots {
        // The signature holds the card, so objects of other cards are told apart without
        // building theirs.
        let of_card = |k: L::Slot| s.copiable(k).key() == s.copiable(slot).key();
        if s.object(slot).face() == FaceIx::DOWN || !kept.any(of_card) {
            kept.insert(slot);
            continue;
        }
        let named = *named.get_or_init(|| s.named());
        let own = OnceCell::new();
        if named.contains(slot)
            || !kept.any(|k| {
                !named.contains(k)
                    && of_card(k)
                    && known(k) == known(slot)
                    && signature(k) == *own.get_or_init(|| signature(slot))
            })
        {
            kept.insert(slot);
        }
    }
    kept
}
