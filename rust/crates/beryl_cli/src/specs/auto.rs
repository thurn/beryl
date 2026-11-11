//! Generated specs: every card's moments, read from its definition and its scenarios rather
//! than written by hand.
//!
//! Two sources give the candidate moments:
//!
//! - **The card's definition.** A permanent with activated abilities other than mana
//!   abilities gets a stop in its controller's second main phase and on the opponent's end
//!   step, a board with every land tapped (which abilities stay offered), each ability
//!   activated, and the permanent bounced in response to one.
//! - **The card's scenarios** (`beryl_cards::SCENARIOS`). Each is cut at each cast or
//!   activation of the card (played out once shown), at each question its resolution or casting
//!   asks (with a yes or no flipped as well), and at its end.
//!
//! Each candidate is played headless from both seats to its first live decision, and what
//! holds there is recorded in the fixture conditions' words ([`snapshot`]). A candidate whose
//! script cannot be played is dropped and reported; a question that is not the card's is
//! dropped; a candidate that records the same as an earlier one of its scenario is dropped.
//!
//! Labels are short and general: what made the moment, not prose.

use beryl_cards::names::ISLAND;
use beryl_engine::TurnStep;
use beryl_engine::card::CardDef;
use beryl_scenario::{Action, P0, Permanent, Scenario};

use super::{Moment, Unreached, View, reach};
use definition::{activated, from_definition};
use scenarios::from_scenario;

mod definition;
mod review;
mod scenarios;
mod snapshot;

pub use definition::ability_labels;
pub use review::{Checkpoint, ReviewSet, checkpoints};
pub(super) use scenarios::{heading, label_of, playground_action, with_libraries};
pub use snapshot::snapshot;

/// One card's generated moments, and the candidates dropped because their scripts could not
/// be played, each with why; and every candidate as played from each seat.
pub struct Generated {
    pub moments: Vec<Moment>,
    pub dropped: Vec<(String, String)>,
    pub played: Vec<Played>,
}

/// One candidate played from one seat to its first live decision: how long that took, and the
/// panic that stopped it, if one did (`script-check` reports each as a check).
pub struct Played {
    pub name: String,
    pub view: View,
    pub seconds: f64,
    pub panic: Option<String>,
}

/// What made a candidate, which decides whether it is kept once recorded.
#[derive(Clone, PartialEq, Eq)]
enum Kind {
    /// From the definition: always kept.
    Probe,
    /// A scenario's cast or activation of the card, played out once shown: kept unless an
    /// earlier one of the same label stopped at the same question.
    Play,
    /// A question in a scenario: kept if the card's resolution or casting asks it, unless an
    /// earlier moment stopped at the same question.
    Ask,
    /// The other answer to the question at this index: kept if the card asks it, unless an
    /// earlier moment gave the same answer to the same question.
    Flip(usize),
    /// A whole scenario from its start, played out once shown: always kept.
    Whole,
    /// A scenario's activation of the card, the card bounced in response: kept unless an
    /// earlier one of the same label stopped at the same question.
    Bounced,
    /// What follows a scenario's end or flipped answer where the human holds priority on their
    /// turn with the stack empty (they pass, or the game goes on to their next turn): kept
    /// unless an earlier moment ended the same.
    FollowOn,
    /// The board just before a scenario's cast of a card with more than one way to play it
    /// (an alternative or additional cost, cycling), the human live with more than one of them
    /// offered: the first one kept.
    Ways,
}

struct Candidate {
    name: String,
    title: String,
    board: Scenario,
    play: Vec<Action>,
    kind: Kind,
    /// The scenario it is cut from, by its name: the page's heading over its moments. Empty
    /// for the definition's moments.
    group: &'static str,
    /// The card whose cast or activation last came before it, which a question is asked
    /// for: one question asked for two causes is two moments.
    cause: Option<&'static str>,
}

/// What a recorded seat's decision is: its kind, what it asks, what asks it.
fn question_of(recorded: &[String]) -> Vec<&str> {
    recorded
        .iter()
        .map(String::as_str)
        .filter(|c| {
            ["decision ", "prompt ", "resolving ", "casting "]
                .iter()
                .any(|p| c.starts_with(p))
        })
        .collect()
}

/// A record with its turn number left out: two moments that end the same way a turn apart
/// show the same thing. Its step stays: stopping in another step is another moment.
fn timeless(recorded: &[String]) -> Vec<&str> {
    recorded
        .iter()
        .map(String::as_str)
        .filter(|c| !c.starts_with("turn "))
        .collect()
}

pub fn generate(def: &'static CardDef, scenarios: &'static [Scenario]) -> Generated {
    let mut candidates = from_definition(def);
    for scenario in scenarios {
        from_scenario(def, scenario, &mut candidates);
    }
    let mut played = Vec::new();
    let mut recorded = record(def, &candidates, &mut played);
    let mut kept = select(def, &candidates, &recorded, &mut Vec::new());
    // What follows the card's moments, for a card whose abilities decide its stops.
    if !activated(def.front()).is_empty() {
        let follow: Vec<Candidate> = (0..candidates.len())
            .filter(|&i| kept[i])
            .filter_map(|i| Some((&candidates[i], recorded[i].as_ref().ok()?)))
            .flat_map(|(c, r)| follow_ons(c, r))
            .collect();
        recorded.extend(record(def, &follow, &mut played));
        candidates.extend(follow);
    }
    let mut dropped = Vec::new();
    kept = select(def, &candidates, &recorded, &mut dropped);
    // Each scenario's moments together, the follow-ons after the rest, in the order the
    // scenarios first appear: the definition's first.
    let first = |group: &str| candidates.iter().position(|c| c.group == group);
    let mut order: Vec<usize> = (0..candidates.len()).filter(|&i| kept[i]).collect();
    order.sort_by_key(|&i| first(candidates[i].group));
    let moments = order
        .into_iter()
        .map(|i| (&candidates[i], &recorded[i]))
        .filter_map(|(c, r)| {
            let r = r.as_ref().ok()?;
            let title = match c.kind {
                Kind::Ask => format!("{}{}", c.title, question(&r[0])),
                _ => c.title.clone(),
            };
            Some(moment(c, title, Some(r)))
        })
        .collect();
    Generated {
        moments,
        dropped,
        played,
    }
}

type Recorded = Result<[Vec<String>; 2], String>;

/// Each candidate played from both seats to its first live decision, and what held there; each
/// seat's play goes to `played`.
fn record(
    def: &'static CardDef,
    candidates: &[Candidate],
    played: &mut Vec<Played>,
) -> Vec<Recorded> {
    candidates
        .iter()
        .map(|c| {
            let m: &'static Moment = Box::leak(Box::new(moment(c, c.title.clone(), None)));
            let [yours, theirs] = View::ALL.map(|view| {
                let started = std::time::Instant::now();
                let reached = reach(m, view);
                played.push(Played {
                    name: c.name.clone(),
                    view,
                    seconds: started.elapsed().as_secs_f64(),
                    panic: match &reached {
                        Err(Unreached::Panicked(why)) => Some(why.clone()),
                        _ => None,
                    },
                });
                reached.map(|v| snapshot(&v, def)).map_err(String::from)
            });
            Ok([yours?, theirs?])
        })
        .collect()
}

/// Which candidates are kept, by their kinds' rules; those that could not be played go to
/// `dropped`, with why.
fn select(
    def: &'static CardDef,
    candidates: &[Candidate],
    recorded: &[Recorded],
    dropped: &mut Vec<(String, String)>,
) -> Vec<bool> {
    let asks = |r: &[Vec<String>; 2]| {
        let mine = [
            format!("resolving {}", def.name),
            format!("casting {}", def.name),
        ];
        r.iter().flatten().any(|c| mine.contains(c))
    };
    let label = |c: &Candidate| c.title.rsplit(" · ").next().map(str::to_owned);
    let mut kept = vec![false; candidates.len()];
    for (i, c) in candidates.iter().enumerate() {
        let r = match &recorded[i] {
            Ok(r) => r,
            Err(e) => {
                dropped.push((c.name.clone(), e.clone()));
                continue;
            }
        };
        let earlier = |same: &dyn Fn(&Candidate, &[Vec<String>; 2]) -> bool| {
            (0..i).any(|j| {
                kept[j]
                    && recorded[j]
                        .as_ref()
                        .is_ok_and(|rj| same(&candidates[j], rj))
            })
        };
        let same_question = |cj: &Candidate, rj: &[Vec<String>; 2]| {
            cj.cause == c.cause && question_of(&rj[0]) == question_of(&r[0])
        };
        let ended_alike = |_: &Candidate, rj: &[Vec<String>; 2]| {
            timeless(&rj[0]) == timeless(&r[0]) && timeless(&rj[1]) == timeless(&r[1])
        };
        kept[i] = match c.kind {
            Kind::Probe | Kind::Whole => true,
            // An activation that ends its scenario shows what the whole of it shows.
            Kind::Play | Kind::Bounced => !earlier(&|cj, rj| {
                (cj.kind == c.kind || cj.kind == Kind::Probe)
                    && label(cj) == label(c)
                    && question_of(&rj[0]) == question_of(&r[0])
                    || cj.kind == Kind::Whole && cj.group == c.group && ended_alike(cj, rj)
            }),
            Kind::Ask => {
                asks(r)
                    && !earlier(&|cj, rj| {
                        matches!(cj.kind, Kind::Ask | Kind::Play | Kind::Probe)
                            && same_question(cj, rj)
                    })
            }
            // The other answer to a question the card asks, unless an earlier moment gave it
            // to the same question for the same cause.
            Kind::Flip(j) => recorded[j].as_ref().is_ok_and(|rj| {
                asks(rj)
                    && !(0..i).any(|k| {
                        kept[k]
                            && label(&candidates[k]) == label(c)
                            && candidates[k].cause == c.cause
                            && matches!(candidates[k].kind, Kind::Flip(p)
                            if recorded[p].as_ref().is_ok_and(|rp| {
                                question_of(&rp[0]) == question_of(&rj[0])
                            }))
                    })
            }),
            Kind::FollowOn => !earlier(&ended_alike),
            Kind::Ways => {
                r[0].contains(&format!("ways to play {}", def.name))
                    && !earlier(&|cj, _| cj.kind == Kind::Ways)
            }
        };
    }
    kept
}

/// What follows a scenario's moment where the human holds priority on their turn with the
/// stack empty: they pass (where does the card stop them next?), and the game goes on to their
/// next turn (what does it offer once their permanents untap?).
fn follow_ons(c: &Candidate, r: &[Vec<String>; 2]) -> Vec<Candidate> {
    if !matches!(c.kind, Kind::Whole | Kind::Flip(_)) {
        return Vec::new();
    }
    let yours = &r[0];
    let holds = |c: &str| yours.iter().any(|y| y == c);
    if !holds("decision priority")
        || !holds("your turn")
        || yours.iter().any(|y| y.starts_with("stack "))
    {
        return Vec::new();
    }
    let Some(turn) = yours
        .iter()
        .find_map(|y| y.strip_prefix("turn ")?.parse::<u16>().ok())
    else {
        return Vec::new();
    };
    let setup = leak([c.board.actions, &c.play[..]].concat());
    let base = c.board.actions(setup);
    let next = |name: &str, title: &str, play: Vec<Action>| Candidate {
        name: format!("{}-{name}", c.name),
        title: if c.kind == Kind::Whole {
            title.to_owned()
        } else {
            format!("{}, {}", c.title, title.to_lowercase())
        },
        board: base,
        play,
        kind: Kind::FollowOn,
        group: c.group,
        cause: c.cause,
    };
    vec![
        next("pass", "Then pass", vec![Action::pass(P0)]),
        next(
            "next-turn",
            "Next turn",
            vec![Action::AdvanceTo {
                turn: turn + 2,
                step: TurnStep::PrecombatMain,
            }],
        ),
    ]
}

/// A question's label, from what was recorded at it: "Ask: move", or its kind.
fn question(recorded: &[String]) -> String {
    recorded
        .iter()
        .find_map(|c| c.strip_prefix("prompt "))
        .or_else(|| recorded.iter().find_map(|c| c.strip_prefix("decision ")))
        .map_or_else(|| "Question".to_owned(), |q| format!("Ask: {q}"))
}

fn leak<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}

fn moment(c: &Candidate, title: String, recorded: Option<&[Vec<String>; 2]>) -> Moment {
    let strs = |v: &Vec<String>| -> &'static [&'static str] {
        leak(
            v.iter()
                .map(|s| &*Box::leak(s.clone().into_boxed_str()))
                .collect(),
        )
    };
    let mut m = Moment::new(
        Box::leak(c.name.clone().into_boxed_str()),
        Box::leak(title.into_boxed_str()),
        c.board,
    )
    .play(leak(c.play.clone()));
    m.group = c.group;
    m.recorded = recorded.map(|[y, t]| [strs(y), strs(t)]);
    m
}

/// Reuse the definition's prepared ability boards, without its stop and failure probes.
pub(super) fn playground_probes(def: &'static CardDef) -> Vec<Moment> {
    from_definition(def)
        .into_iter()
        .filter(|c| c.name.starts_with("ability-"))
        .map(|mut c| {
            c.name = format!("playground-{}", c.name);
            moment(&c, c.title.clone(), None)
        })
        .collect()
}

/// Supply printed casting resources to a play-from-hand fallback. The engine still owns
/// affordability and payment; this only builds a testing position.
pub(super) fn fund_cast(def: &CardDef, mut board: Scenario) -> Scenario {
    let cost = def.front().cost;
    let mut permanents = board.setup.players[0].battlefield.to_vec();
    let basics = ["Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"];
    for (name, count) in basics.into_iter().zip(cost.symbols) {
        if beryl_cards::CARDS.cards().iter().any(|d| d.name == name) {
            permanents.extend(std::iter::repeat_n(
                Permanent::new(name),
                usize::from(count),
            ));
        }
    }
    permanents.extend(std::iter::repeat_n(
        Permanent::new(ISLAND),
        usize::from(cost.generic) + 2,
    ));
    board = board.battlefield(P0, leak(permanents));
    board
}
