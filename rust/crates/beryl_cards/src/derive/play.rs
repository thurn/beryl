//! What a generated scenario does: its openings, and each run of one played to a quiet
//! position through [`Live`].

use std::collections::BTreeSet;

use beryl_engine::card::{
    Ability, CardDef, FaceIx, ObjectAbility, TargetKind, TriggerEvent, Types,
};

use beryl_engine::{DecisionKind, Outcome, Step, TurnStep};
use beryl_scenario::{Action, Assertion, Ending, Live, P0, Scenario};

use super::position::{LAST_TURN, Place, TURN};
use crate::generate::unambiguous;
use crate::{CARDS, props};

/// The most actions one run takes before it is cut off, a bound no card reaches today.
const MOST_ACTIONS: usize = 80;

/// Answers given, each as a question (its kind and the actions that answer it) and the index
/// of the answer.
pub(super) type Answers = BTreeSet<(String, usize)>;

/// One thing a run does when no question is open.
#[derive(Clone, Copy, Debug)]
pub(super) enum Stimulus {
    /// An action taken at its owner's priority with the stack empty.
    Act(Action),
    /// An action player 0 takes at their priority with something on the stack: a response.
    Respond(Action),
    /// Pass priority, the stack empty, until the game reaches step `.1` of turn `.0`. Unlike
    /// [`Action::AdvanceTo`], it lets every question on the way be asked: an attacker to
    /// declare, a trigger's target.
    PassUntil(u16, TurnStep),
    /// Stop here, whatever is on the stack.
    Hold,
}

/// One run of an opening.
#[derive(Clone)]
pub(super) struct Run {
    pub(super) actions: Vec<Action>,
    /// Whether the card did anything: it was played or activated, something of its or its
    /// tokens' went on the stack, or, as mana, it was tapped.
    pub(super) touched: bool,
    /// Whether a question followed an order of the card's own triggers that a pick could name
    /// only by the card: the oracles may put them on the stack either way, and then ask that
    /// question of another trigger.
    pub(super) unordered: bool,
}

/// The cards player 0 casts a spell of the card's own in response to: a sorcery, and a
/// creature spell, which some counters want.
const LEADS: [&str; 2] = [props::SORCERY, props::SMALL_CREATURE];

/// Every opening of a scenario of `def` starting in `place`, labelled: each action naming the
/// card its owner can take at the first priority in their main phase, each they can take in
/// response to a spell of theirs that they could not take there, and each event a triggered
/// ability of the card's waits for that the position can bring about. As mana, each spell
/// its owner can cast.
pub(super) fn openings(
    def: &'static CardDef,
    place: Place,
    start: &Scenario,
    prelude: &[Action],
    preamble: &[Stimulus],
) -> Vec<(String, Vec<Stimulus>)> {
    let with = |more: &[Stimulus]| preamble.iter().chain(more).copied().collect::<Vec<_>>();
    // On to the end step after it, for a card with an ability that waits for a step: a
    // delayed trigger's, or a countdown's.
    let waits = def
        .faces
        .iter()
        .flat_map(|face| face.abilities)
        .filter_map(Ability::trigger)
        .any(|t| matches!(t.event, TriggerEvent::Step(_) | TriggerEvent::MainPhase));
    let then: &[Stimulus] = if waits && !matches!(place, Place::Mana(..)) {
        &[Stimulus::PassUntil(TURN, TurnStep::End)]
    } else {
        &[]
    };
    // A card that targets a spell, with its spell ability or as it enters, responds to each
    // lead even where it can be cast without.
    let targets_spells = def.faces.iter().flat_map(|face| face.abilities).any(|a| {
        a.targets()
            .iter()
            .any(|t| matches!(t.kind, TargetKind::Spell | TargetKind::SpellOrPermanent))
    });
    let mut out: Vec<(String, Vec<Stimulus>)> = Vec::new();
    let mut leads: Vec<Option<&'static str>> = vec![None];
    if !matches!(place, Place::Mana(..)) {
        leads.extend(
            LEADS
                .iter()
                .filter(|&&lead| lead != def.name)
                .map(|&l| Some(l)),
        );
    }
    for lead in leads {
        let cast = lead.map(|card| Action::cast(P0, card));
        let mut to: Vec<Stimulus> = cast.into_iter().map(Stimulus::Act).collect();
        to.push(Stimulus::Hold);
        let ran = Walk::new(start, prelude).and_then(|w| {
            walk(
                def,
                &with(&to),
                w,
                None,
                &mut Answers::new(),
                &mut Vec::new(),
            )
        });
        let Ok(Walk { mut live, .. }) = ran else {
            continue;
        };
        let Ok(Step::Decide(d)) = live.position(Action::pass(P0)) else {
            continue;
        };
        if d.kind != DecisionKind::Priority || d.player != P0 {
            continue;
        }
        for action in unambiguous(&live, &d, false) {
            let label = match place {
                Place::Mana(..) => match action {
                    Action::Cast { card, .. } if !is(def, card) => format!("cast {card}"),
                    _ => continue,
                },
                _ => match label(action, def) {
                    Some(label) => label,
                    None => continue,
                },
            };
            match cast {
                None => out.push((label, with(&[&[Stimulus::Act(action)], then].concat()))),
                Some(cast) if targets_spells || !out.iter().any(|(l, _)| *l == label) => {
                    let lead = lead.unwrap_or_default();
                    let stimuli = [&[Stimulus::Act(cast), Stimulus::Respond(action)], then];
                    out.push((
                        format!("{label} in response to {lead}"),
                        with(&stimuli.concat()),
                    ));
                }
                Some(_) => {}
            }
        }
    }
    if let Place::Battlefield(f) = place {
        out.extend(
            awaited(def, f)
                .into_iter()
                .map(|(label, stimuli)| (label, with(&stimuli))),
        );
    }
    out
}

/// The openings for the events the triggered abilities of face `f` of `def` wait for, on the
/// battlefield: an attack, each step, a spell cast.
fn awaited(def: &'static CardDef, f: FaceIx) -> Vec<(String, Vec<Stimulus>)> {
    let face = def.face(f);
    let mut events: Vec<TriggerEvent> = face
        .abilities
        .iter()
        .filter_map(Ability::trigger)
        .filter(|t| !t.delayed)
        .map(|t| t.event)
        .collect();
    events.dedup_by(|a, b| format!("{a:?}") == format!("{b:?}"));
    let mut out = Vec::new();
    let attacks = events.iter().any(|e| {
        matches!(
            e,
            TriggerEvent::Attacks
                | TriggerEvent::PlayerAttacks
                | TriggerEvent::DealsCombatDamageToPlayer
                | TriggerEvent::Dies
        )
    });
    if attacks && face.types.contains(Types::CREATURE) {
        let combat = vec![Stimulus::PassUntil(TURN, TurnStep::EndOfCombat)];
        out.push(("attack".to_owned(), combat));
    }
    for event in &events {
        match *event {
            TriggerEvent::Step(step) => {
                for turn in [TURN, LAST_TURN] {
                    // The preamble plays this turn's beginning phase.
                    if turn > TURN || (step as u8) > TurnStep::PrecombatMain as u8 {
                        let until = Stimulus::PassUntil(turn, step);
                        out.push((format!("to {step:?} of turn {turn}"), vec![until]));
                    }
                }
            }
            TriggerEvent::Casts => {
                let cast = Stimulus::Act(Action::cast(P0, props::SORCERY));
                out.push(("cast a sorcery".to_owned(), vec![cast]));
            }
            _ => {}
        }
    }
    out
}

/// A run in progress: the game, what it did so far, and the index of the stimulus it is on.
/// A copy goes on from the same position, as a run that answers a question otherwise does.
#[derive(Clone)]
pub(super) struct Walk {
    live: Live,
    run: Run,
    next: usize,
}

impl Walk {
    /// A walk from `start`, with player 0 having taken the actions of `prelude`: a witness's
    /// (`super::witness`), each question in it answered.
    pub(super) fn new(start: &Scenario, prelude: &[Action]) -> Result<Walk, String> {
        let mut live = Live::new(&CARDS, start).map_err(|f| f.message)?;
        for &action in prelude {
            live.act(action)
                .map_err(|e| format!("the witness's prelude: {e}"))?;
        }
        Ok(Walk {
            live,
            run: Run {
                actions: prelude.to_vec(),
                touched: false,
                unordered: false,
            },
            next: 0,
        })
    }

    /// The run, done: a script that ends the game says so, or the runner, which plays no
    /// further than its last action asks, would stop before the end. As mana, the card did
    /// something if it was tapped: its owner tapped nothing else.
    pub(super) fn finish(self, def: &'static CardDef, place: Place) -> Run {
        let Walk { live, mut run, .. } = self;
        let s = live.state();
        let ending = match s.outcome() {
            Some(Outcome::Win(player)) => Some(Ending::Win { player }),
            Some(Outcome::Draw) => Some(Ending::Draw),
            _ => None,
        };
        let outcome = ending.map(|outcome| Action::assert(Assertion::Outcome { outcome }));
        run.actions.extend(outcome);
        if let Place::Mana(..) = place {
            run.touched = s
                .battlefield()
                .iter()
                .any(|&slot| is(def, live.name(slot)) && s.object(slot).is_tapped());
        }
        run
    }
}

/// `walk` played on through `stimuli` to a quiet position: the stack resolved and every
/// question answered. The first question takes answer `first` where it is given; each other
/// takes its first answer, and each of its other answers not yet in `taken` joins `taken`
/// and goes to `branches` with a copy of the walk at that question, for a later run.
pub(super) fn walk(
    def: &'static CardDef,
    stimuli: &[Stimulus],
    mut walk: Walk,
    mut first: Option<usize>,
    taken: &mut Answers,
    branches: &mut Vec<(Walk, usize)>,
) -> Result<Walk, String> {
    let ours = |live: &Live| {
        let t = live.table();
        live.state().stack().iter().any(|entry| {
            let of = t.def(entry.card());
            std::ptr::eq(of, def) || def.tokens.iter().any(|&token| std::ptr::eq(of, token))
        })
    };
    let mut ordered_by_name = false;
    for _ in 0..MOST_ACTIONS {
        let s = walk.live.state();
        if s.outcome().is_some() || s.turn().number() > LAST_TURN {
            break;
        }
        let pending = stimuli.get(walk.next).copied();
        let probe = match (s.stack().is_empty(), pending) {
            (_, Some(Stimulus::Hold)) => break,
            // Done, unless what was done triggered something not yet on the stack.
            (true, None | Some(Stimulus::PassUntil(..))) => Action::pass(P0),
            (true, Some(Stimulus::Act(action))) | (false, Some(Stimulus::Respond(action))) => {
                action
            }
            (true, Some(Stimulus::Respond(_))) => {
                return Err("nothing on the stack to respond to".to_owned());
            }
            (false, _) => Action::Resolve,
        };
        let Step::Decide(d) = walk.live.position(probe)? else {
            break;
        };
        walk.run.touched |= ours(&walk.live);
        let now = *walk.live.state().turn();
        if now.number() > LAST_TURN {
            break;
        }
        let action = if d.kind == DecisionKind::Priority {
            let stack = !walk.live.state().stack().is_empty();
            // The action to take, and whether it carries out the pending stimulus.
            let (action, done) = match pending {
                Some(Stimulus::Respond(action)) if stack && d.player == P0 => (action, true),
                _ if stack => (Action::Resolve, false),
                Some(Stimulus::Act(action)) => (action, true),
                Some(Stimulus::PassUntil(turn, step)) => {
                    if (now.number(), now.step() as u8) >= (turn, step as u8) {
                        walk.next += 1;
                        continue;
                    }
                    // Straight there where nobody is asked anything on the way, which a copy
                    // of the game finds out; else one pass at a time.
                    let advance = Action::AdvanceTo { turn, step };
                    let mut trial = walk.live.clone();
                    if trial.act(advance).is_ok() && trial.state().outcome().is_none() {
                        walk.live = trial;
                        walk.run.actions.push(advance);
                        walk.next += 1;
                        continue;
                    }
                    (Action::pass(d.player), false)
                }
                Some(Stimulus::Respond(_) | Stimulus::Hold) | None => break,
            };
            if done {
                walk.run.touched |= names_card(action, def);
                walk.next += 1;
            }
            action
        } else {
            let mut picks = unambiguous(&walk.live, &d, true);
            if picks.is_empty() && d.kind == DecisionKind::TriggerOrder {
                // Two triggers of the card's own: a pick names the first by its ability's
                // index, as the hand-written scenarios answer it.
                picks.extend(walk.live.picks(&d).first().map(|&(_, action)| action));
                ordered_by_name = true;
            } else if ordered_by_name {
                walk.run.unordered = true;
            }
            if picks.is_empty() {
                return Err(format!("no scripted answer to a {:?} decision", d.kind));
            }
            let question = format!("{:?} {picks:?}", d.kind);
            let answer = first.take().unwrap_or_else(|| {
                for other in 1..picks.len() {
                    if taken.insert((question.clone(), other)) {
                        branches.push((walk.clone(), other));
                    }
                }
                taken.insert((question, 0));
                0
            });
            *picks
                .get(answer)
                .ok_or_else(|| format!("no answer {answer} to a {:?} decision", d.kind))?
        };
        if act(&mut walk.live, &mut walk.run.actions, action)? {
            break;
        }
    }
    Ok(walk)
}

/// Runs `action`, adding it to `actions`, and whether the game is over after it: an action that ends the game ends
/// the run, and is no failure.
fn act(live: &mut Live, actions: &mut Vec<Action>, action: Action) -> Result<bool, String> {
    match live.act(action) {
        Ok(()) => {}
        Err(_) if live.state().outcome().is_some() => {}
        Err(e) => return Err(e),
    }
    actions.push(action);
    Ok(live.state().outcome().is_some())
}

/// Whether `action` plays the card named `card` or activates one of its abilities, which may
/// put nothing on the stack: a land, or a mana ability.
fn names_card(action: Action, def: &CardDef) -> bool {
    match action {
        Action::PlayLand { card, .. }
        | Action::Cast { card, .. }
        | Action::Activate { card, .. }
        | Action::TurnFaceUp { card, .. } => is(def, card),
        _ => false,
    }
}

/// Whether `name` is `def`'s name or a face's, which a transformed permanent is known by
/// (CR 712.6d).
fn is(def: &CardDef, name: &str) -> bool {
    name == def.name || def.faces.iter().any(|face| face.name == name)
}

/// How an opening's action naming the card `card` is labelled, or `None` for one that names
/// another card.
fn label(action: Action, def: &CardDef) -> Option<String> {
    let card = |c: &str| is(def, c);
    let label = match action {
        Action::PlayLand { card: c, .. } if card(c) => "play".to_owned(),
        Action::Cast {
            card: c,
            face,
            costs,
            ..
        } if card(c) => {
            let face = if face == FaceIx::FRONT {
                String::new()
            } else {
                " back face".to_owned()
            };
            let costs = if costs.is_empty() {
                String::new()
            } else {
                format!(" with options {:b}", costs.bits())
            };
            format!("cast{face}{costs}")
        }
        Action::Activate {
            card: c, ability, ..
        } if card(c) => match ability {
            ObjectAbility::Own(i) => format!("activate ability {}", i.index()),
            ObjectAbility::Granted(k) => format!("activate granted ability {k}"),
        },
        Action::TurnFaceUp { card: c, .. } if card(c) => "turn face up".to_owned(),
        _ => return None,
    };
    Some(label)
}
