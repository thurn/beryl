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

use std::fmt::Write as _;

use beryl_cards::names::{
    COUNTERSPELL, FRACTURED_IDENTITY, ISLAND, MANA_DRAIN, OUST, PLAINS, REPRIEVE, SINK_INTO_STUPOR,
    STOCK_UP, SWAMP, THIEVING_SKYDIVER, TIME_SPIRAL,
};
use beryl_engine::card::{AbilityIx, AbilityKind, CardDef, Face, Needs, Timing, Types};
use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, Seat, TurnStep};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_session::protocol::{
    AskView, Candidate as Offer, Kind as Decision, PlayerView, Side, StackItem, Status, ZoneName,
};
use serde_json::json;

use super::{Moment, View, reach, slug};
use crate::fixture::Condition;

/// One card's generated moments, and the candidates dropped because their scripts could not
/// be played, each with why.
pub struct Generated {
    pub moments: Vec<Moment>,
    pub dropped: Vec<(String, String)>,
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
            ["decision ", "ask ", "resolving ", "casting "]
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
    let mut recorded = record(def, &candidates);
    let mut kept = select(def, &candidates, &recorded, &mut Vec::new());
    // What follows the card's moments, for a card whose abilities decide its stops.
    if !activated(def.front()).is_empty() {
        let follow: Vec<Candidate> = (0..candidates.len())
            .filter(|&i| kept[i])
            .filter_map(|i| Some((&candidates[i], recorded[i].as_ref().ok()?)))
            .flat_map(|(c, r)| follow_ons(c, r))
            .collect();
        recorded.extend(record(def, &follow));
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
    Generated { moments, dropped }
}

/// A playable setup and the interaction it exposes to a reviewer.
pub struct Checkpoint {
    pub moment: Moment,
    pub first: View,
    pub roles: [String; 2],
    pub actions: Vec<String>,
    pub review_casts: &'static [Action],
}

pub struct ReviewSet {
    pub examples: Vec<Checkpoint>,
    /// Scenarios whose setup never yielded a review checkpoint.
    pub unreached: Vec<&'static str>,
}

pub fn checkpoints(def: &'static CardDef, scenarios: &'static [Scenario]) -> ReviewSet {
    let mut candidates = from_definition(def);
    for scenario in scenarios {
        from_scenario(def, scenario, &mut candidates);
    }
    // Prefer the choice itself to arriving at it through an earlier decision.
    candidates.sort_by_key(|c| match c.kind {
        Kind::Ask => 0,
        Kind::Play | Kind::Probe | Kind::Ways | Kind::Bounced => 1,
        _ => 2,
    });
    let mut seen = Vec::new();
    let mut groups = Vec::new();
    let mut result = Vec::new();
    for c in candidates {
        if matches!(c.kind, Kind::Flip(_) | Kind::FollowOn)
            || c.kind == Kind::Whole && groups.contains(&c.group)
        {
            continue;
        }
        let m = moment(&c, c.title.clone(), None);
        let Ok((views, countered)) = super::review_position(&m) else {
            continue;
        };
        let Some((key, checkpoint)) = review_example(def, &c, m, &views, countered.is_some())
        else {
            continue;
        };
        groups.push(c.group);
        if !seen.contains(&key) {
            seen.push(key);
            let mut checkpoint = checkpoint;
            if let Some(card) = countered
                && c.kind == Kind::Play
                && checkpoint.moment.title == "Spell countered"
            {
                use_cast_before_counter(def, &mut checkpoint, &card);
            }
            result.push(checkpoint);
        }
    }
    ReviewSet {
        examples: result,
        unreached: scenarios
            .iter()
            .map(|s| heading(def, s))
            .filter(|name| !groups.contains(name))
            .collect(),
    }
}

/// Start the counter example before the original spell, so either cast can be tried.
fn use_cast_before_counter(def: &CardDef, checkpoint: &mut Checkpoint, countered: &str) {
    let m = &checkpoint.moment;
    let [
        Action::Cast {
            card,
            player: answerer,
            ..
        },
    ] = m.play
    else {
        return;
    };
    if *card != def.name {
        return;
    }
    let Some((
        index,
        &Action::Cast {
            player: caster,
            card,
            ..
        },
    )) = m.board.actions.iter().enumerate().rev().find(|(_, a)| {
        matches!(a, Action::Cast { card, player, .. }
            if *card == countered && player != answerer)
    })
    else {
        return;
    };
    // A recorded cast step can advance a cast, while separate scripted choices still
    // belong to the full replay.
    if index + 1 != m.board.actions.len() {
        return;
    }
    let first = View::ALL[caster.index()];
    let mut before = Moment::new(
        m.name,
        "Cast into a counter",
        m.board.actions(&m.board.actions[..index]),
    );
    before.group = m.group;
    let Ok((views, _)) = super::review_position(&before) else {
        return;
    };
    let view = &views[caster.index()];
    let Status::Deciding { decision } = &view.status else {
        return;
    };
    if !decision.candidates.iter().any(|c| {
        matches!(c.option, Offer::Cast { slot, .. } if view.objects.iter().any(|o|
            o.slot == slot && o.name.as_deref() == Some(card)))
    }) {
        return;
    }
    checkpoint.review_casts = leak(vec![m.board.actions[index], m.play[0]]);
    checkpoint.moment = before;
    checkpoint.first = first;
    checkpoint.roles[caster.index()] = format!("{card} caster");
    checkpoint.roles[answerer.index()] = format!("{} caster", def.name);
    checkpoint.actions = decision
        .candidates
        .iter()
        .map(|c| c.label.clone())
        .collect();
}

fn review_example(
    def: &'static CardDef,
    c: &Candidate,
    mut moment: Moment,
    views: &[PlayerView; 2],
    countered: bool,
) -> Option<(serde_json::Value, Checkpoint)> {
    let owner = usize::from(!matches!(views[0].status, Status::Deciding { .. }));
    let view = &views[owner];
    let decision = match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    };
    let name_of = |slot| {
        view.objects
            .iter()
            .find(|o| o.slot == slot)
            .and_then(|o| o.name.as_deref())
    };
    let card_item = |item: &StackItem| match item {
        StackItem::Spell { slot, .. } => name_of(*slot) == Some(def.name),
        StackItem::Ability { name, .. } => name == def.name,
    };
    let on_stack = view.stack.iter().rev().find(|item| card_item(item));
    let source = decision
        .and_then(|d| d.resolving.as_ref())
        .filter(|item| card_item(item))
        .or(on_stack);
    let card_is_asking = decision.is_some_and(|d| {
        d.kind != Decision::Priority
            && (d.resolving.as_ref().is_some_and(card_item)
                || d.source.and_then(name_of) == Some(def.name))
    });
    if c.kind == Kind::Ask && !card_is_asking {
        return None;
    }
    let title = if card_is_asking {
        review_prompt(decision?)
    } else if countered && on_stack.is_none() {
        "Spell countered".to_owned()
    } else if on_stack.is_some() {
        format!("Respond to {}", def.name)
    } else if c.kind == Kind::Whole {
        "After resolution".to_owned()
    } else {
        c.title.clone()
    };
    let source_side = match source {
        Some(StackItem::Spell { slot, .. }) => view
            .objects
            .iter()
            .find(|o| o.slot == *slot)
            .map(|o| o.controller),
        Some(StackItem::Ability { controller, .. }) => Some(*controller),
        None => None,
    };
    let paying = decision.is_some_and(|d| matches!(d.ask, AskView::UnlessPays { .. }));
    let roles = std::array::from_fn(|seat| match (seat == owner, source_side) {
        (true, _) if paying => "Paying player".to_owned(),
        (true, Some(Side::You)) | (false, Some(Side::Opponent)) => {
            let role = if matches!(source, Some(StackItem::Spell { .. })) {
                "caster"
            } else {
                "controller"
            };
            format!("{} {role}", def.name)
        }
        _ => format!("Player {}", seat + 1),
    });
    let actions = decision.map_or_else(Vec::new, |d| {
        d.candidates
            .iter()
            .map(|c| match (&d.ask, &c.option) {
                (AskView::UnlessPays { cost }, Offer::Yes) => format!("Pay {cost}"),
                (AskView::UnlessPays { .. }, Offer::No) => "Don't Pay".to_owned(),
                _ => c.label.clone(),
            })
            .collect()
    });
    let key = if card_is_asking {
        review_choice_key(decision?, owner, source_side)
    } else if on_stack.is_some() || countered {
        json!([title, owner, source_side])
    } else {
        let snapshots = views.each_ref().map(|v| snapshot(v, def));
        json!([title, snapshots.each_ref().map(|r| timeless(r))])
    };
    moment.name = Box::leak(format!("review-{}", moment.name).into_boxed_str());
    moment.title = Box::leak(title.into_boxed_str());
    Some((
        key,
        Checkpoint {
            moment,
            first: View::ALL[owner],
            roles,
            actions,
            review_casts: &[],
        },
    ))
}

/// Deduplicate choices by their presentation and options, independent of the surrounding
/// board's card names and slot IDs.
fn review_choice_key(
    decision: &beryl_session::protocol::DecisionView,
    owner: usize,
    source: Option<Side>,
) -> serde_json::Value {
    let mut ask = json!(decision.ask);
    if let Some(fields) = ask.as_object_mut() {
        fields.remove("card");
        if let Some(eligible) = fields.get_mut("eligible") {
            *eligible = json!(eligible.as_array().map(Vec::len));
        }
    }
    let options: Vec<_> = decision
        .candidates
        .iter()
        .map(|c| {
            let payments: Vec<_> = c.payments.iter().map(|p| (p.x, p.taps.len())).collect();
            json!([json!(c.option)["kind"], payments, c.targets.len()])
        })
        .collect();
    json!(["choice", owner, source, decision.kind, ask, options])
}

fn review_prompt(d: &beryl_session::protocol::DecisionView) -> String {
    match &d.ask {
        AskView::UnlessPays { cost } => format!("Pay {cost}?"),
        AskView::PayLife { life } => format!("Pay {life} life?"),
        AskView::Scry { count } => format!("Scry {count}"),
        ask => {
            let value = json!(ask);
            let kind = json!(d.kind);
            let label = value["kind"]
                .as_str()
                .filter(|&k| k != "plain")
                .or_else(|| kind.as_str())
                .unwrap_or("choice");
            let mut title = String::new();
            for (i, ch) in label.chars().enumerate() {
                if i == 0 {
                    title.extend(ch.to_uppercase());
                } else if ch.is_ascii_uppercase() {
                    title.push(' ');
                    title.push(ch.to_ascii_lowercase());
                } else {
                    title.push(ch);
                }
            }
            title
        }
    }
}

type Recorded = Result<[Vec<String>; 2], String>;

/// Each candidate played from both seats to its first live decision, and what held there.
fn record(def: &'static CardDef, candidates: &[Candidate]) -> Vec<Recorded> {
    candidates
        .iter()
        .map(|c| {
            let m: &'static Moment = Box::leak(Box::new(moment(c, c.title.clone(), None)));
            let yours = reach(m, View::Yours).map(|v| snapshot(&v, def));
            let theirs = reach(m, View::Theirs).map(|v| snapshot(&v, def));
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

/// A question's label, from what was recorded at it: "Ask: mayMove", or its kind.
fn question(recorded: &[String]) -> String {
    recorded
        .iter()
        .find_map(|c| c.strip_prefix("ask "))
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

// The definition's moments.

/// Twelve lands for each library: enough to draw from for the turns a moment plays.
const LANDS: &[&str] = &[
    PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND,
];

/// The opponent's lands: the {1}{U}{U} for Sink into Stupor, and two more.
const THEIR_LANDS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
];

/// A land in the human's hand: it keeps them stopping in their main phase after what a moment
/// plays, so the board shows what it left before the turn moves on.
const LAND_IN_HAND: &[&str] = &[PLAINS];

/// The opponent's lands and a tapped creature, for an ability that targets one.
const TARGETS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
    Permanent::new(THIEVING_SKYDIVER).tapped(),
];

/// The opponent's hand: Sink into Stupor answers any spell or ability with the card on the
/// battlefield, so from their seat the game stops on each one, as an opponent holding a
/// response does.
const THEIR_HAND: &[&str] = &[SINK_INTO_STUPOR, COUNTERSPELL, PLAINS];

/// A mid-game table around the card: life off 20, graveyards, the opponent's lands and hand.
/// The human's hand stays empty: a land in it would add a stop of its own.
fn table(name: &'static str, turn: u16, active: Seat, step: TurnStep) -> Scenario {
    Scenario::new(name)
        .at(turn, active, step)
        .life(P0, 17)
        .life(P1, 14)
        .graveyard(P0, &[STOCK_UP, REPRIEVE])
        .battlefield(P1, THEIR_LANDS)
        .hand(P1, THEIR_HAND)
        .graveyard(P1, &[OUST, MANA_DRAIN])
        .library(P0, LANDS)
        .library(P1, LANDS)
}

/// The abilities the definition's moments cover: activated abilities other than mana
/// abilities, of the front face, by index among its abilities.
fn activated(face: &Face) -> Vec<(u8, &'static beryl_engine::card::Ability)> {
    face.abilities
        .iter()
        .enumerate()
        .filter(|(_, a)| a.kind == AbilityKind::Activated)
        .filter(|(_, a)| !a.cost.discard_self && a.cost.crew == 0)
        .filter_map(|(i, a)| Some((u8::try_from(i).ok()?, a)))
        .collect()
}

/// The card's activated abilities that `snapshot` names by index, each with its label: its
/// cost as the card prints it, for a reader of `specs-describe`.
pub fn ability_labels(def: &'static CardDef) -> Vec<(u8, String)> {
    activated(def.front())
        .into_iter()
        .map(|(k, a)| (k, cost_label(a, k)))
        .collect()
}

/// An ability's label: its cost, as its card prints it ("{2}{T}", "+2", "−1"), or its index
/// for a cost that is none of those.
fn cost_label(a: &beryl_engine::card::Ability, k: u8) -> String {
    if let Some(l) = a.cost.loyalty {
        return match l {
            0 => "0".to_owned(),
            l if l > 0 => format!("+{l}"),
            l => format!("\u{2212}{}", l.unsigned_abs()),
        };
    }
    let mana = a.cost.mana;
    let mut out = String::new();
    if mana.generic > 0 {
        let _ = write!(out, "{{{}}}", mana.generic);
    }
    for (symbol, &n) in ["W", "U", "B", "R", "G", "C"].iter().zip(&mana.symbols) {
        for _ in 0..n {
            let _ = write!(out, "{{{symbol}}}");
        }
    }
    if a.cost.tap {
        out += "{T}";
    }
    if a.cost.sacrifice_self {
        out += if out.is_empty() {
            "Sacrifice"
        } else {
            ", sacrifice"
        };
    }
    if out.is_empty() {
        format!("Ability {k}")
    } else {
        out
    }
}

/// Lands that pay for every one of `abilities`, one at a time: each color's most, and
/// Islands for the rest. None if a cost needs mana no implemented basic land makes.
fn lands(abilities: &[(u8, &beryl_engine::card::Ability)]) -> Option<Vec<&'static str>> {
    // W, U, B, R, G, C, by `ManaColor` index.
    const BASICS: [Option<&str>; 6] = [Some(PLAINS), Some(ISLAND), Some(SWAMP), None, None, None];
    let mut most = [0u8; 6];
    let mut total = 0u8;
    for (_, a) in abilities {
        let cost = a.cost.mana;
        for (m, n) in most.iter_mut().zip(cost.symbols) {
            *m = (*m).max(n);
        }
        total = total.max(cost.generic + cost.symbols.iter().sum::<u8>());
    }
    let mut out = Vec::new();
    for (color, &n) in most.iter().enumerate() {
        if n > 0 {
            let land = BASICS[color]?;
            out.extend(std::iter::repeat_n(land, n.into()));
        }
    }
    while out.len() < total.into() {
        out.push(ISLAND);
    }
    Some(out)
}

/// The card as a permanent: with counters for an ability that removes them, and the loyalty
/// `ability` costs where its printed loyalty is less.
fn permanent(
    def: &'static CardDef,
    face: &Face,
    ability: Option<&beryl_engine::card::Ability>,
) -> Permanent {
    let p = Permanent::new(def.name);
    let cost = ability
        .and_then(|a| a.cost.loyalty)
        .map_or(0, i8::unsigned_abs);
    if face.loyalty.is_some_and(|l| l < cost) {
        return p.counters(CounterKind::Loyalty, cost);
    }
    match face.abilities.iter().find_map(|a| a.cost.remove) {
        Some(kind) => p.counters(kind, 2),
        None => p,
    }
}

fn probe(name: &str, title: &str, board: Scenario, play: Vec<Action>) -> Candidate {
    let cause = cause_of(&play);
    Candidate {
        name: name.to_owned(),
        title: title.to_owned(),
        board,
        play,
        kind: Kind::Probe,
        group: "",
        cause,
    }
}

/// The card the last cast or activation among `actions` was of.
fn cause_of(actions: &[Action]) -> Option<&'static str> {
    actions.iter().rev().find_map(|a| match *a {
        Action::Cast { card, .. } | Action::Activate { card, .. } => Some(card),
        _ => None,
    })
}

type Activated = [(u8, &'static beryl_engine::card::Ability)];

fn from_definition(def: &'static CardDef) -> Vec<Candidate> {
    let face = def.front();
    if face.types.intersects(Types::INSTANT) || face.types.intersects(Types::SORCERY) {
        return Vec::new();
    }
    let abilities = activated(face);
    if abilities.is_empty() {
        return Vec::new();
    }
    let Some(lands) = lands(&abilities) else {
        return Vec::new();
    };
    let board = |ability: Option<&beryl_engine::card::Ability>, tapped: bool| {
        let lands = lands.iter().map(|&l| {
            let land = Permanent::new(l);
            if tapped { land.tapped() } else { land }
        });
        leak(
            std::iter::once(permanent(def, face, ability))
                .chain(lands)
                .collect(),
        )
    };
    let mut out = stops(&abilities, board(None, false), board(None, true));
    activations(def, &abilities, &|a| board(Some(a), false), &mut out);
    out
}

/// The moments of when the card stops its controller, and when it must not.
fn stops(
    abilities: &Activated,
    open: &'static [Permanent],
    tapped: &'static [Permanent],
) -> Vec<Candidate> {
    let mut out = Vec::new();
    // Loyalty abilities are activated only as a sorcery could be cast (CR 606.3), so only
    // the others can stop a player outside their own main phase.
    if abilities
        .iter()
        .any(|(_, a)| a.timing == Timing::Instant && a.cost.loyalty.is_none())
    {
        let main = table("stops-main", 5, P0, TurnStep::PostcombatMain);
        out.push(probe(
            "stops-main",
            "Stops: main 2",
            main.battlefield(P0, open),
            Vec::new(),
        ));
        let end = table("stops-their-end", 6, P1, TurnStep::PrecombatMain);
        out.push(probe(
            "stops-their-end",
            "Stops: their end",
            end.battlefield(P0, open),
            Vec::new(),
        ));
    }
    if abilities
        .iter()
        .any(|(_, a)| a.cost.mana.generic > 0 || a.cost.mana.symbols.iter().any(|&n| n > 0))
    {
        // A land in hand stops the human here, to see what stays offered unpaid.
        let board = table("lands-tapped", 5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, tapped)
            .hand(P0, LAND_IN_HAND);
        out.push(probe("lands-tapped", "Lands tapped", board, Vec::new()));
    }
    if abilities.iter().any(|(_, a)| a.needs != Needs::Nothing) {
        // Nothing paid for and nothing for the ability that needs something: no stop.
        let board = table("nothing-to-do", 5, P0, TurnStep::PostcombatMain).battlefield(P0, tapped);
        out.push(probe("nothing-to-do", "Nothing to do", board, Vec::new()));
    }
    // The same board with what the ability needs: it stops the human here.
    for need in abilities
        .iter()
        .map(|(_, a)| a.needs)
        .filter(|&n| n != Needs::Nothing)
    {
        let (name, title, board) = match need {
            Needs::ExiledWithSource => {
                let mut met = tapped.to_vec();
                met[0] = met[0].exiled_with(&[OUST]);
                ("card-exiled", "Stops: card exiled", leak(met))
            }
            Needs::Nothing => continue,
        };
        if out.iter().any(|c| c.name == name) {
            continue;
        }
        let board = table(name, 5, P0, TurnStep::PostcombatMain).battlefield(P0, board);
        out.push(probe(name, title, board, Vec::new()));
    }
    out
}

/// Each ability activated, and the card bounced in response to one.
fn activations(
    def: &'static CardDef,
    abilities: &Activated,
    open: &dyn Fn(&beryl_engine::card::Ability) -> &'static [Permanent],
    out: &mut Vec<Candidate>,
) {
    for &(k, a) in abilities {
        let board = table("ability", 5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, open(a))
            .battlefield(P1, TARGETS)
            .hand(P0, LAND_IN_HAND);
        let play = vec![Action::activate(P0, def.name, k)];
        out.push(probe(
            &format!("ability-{k}"),
            &cost_label(a, k),
            board,
            play,
        ));
    }
    let bounceable = !def.front().types.intersects(Types::LAND);
    if let Some(&(k, a)) = abilities
        .iter()
        .find(|(_, a)| a.timing == Timing::Instant && !a.cost.sacrifice_self)
        .filter(|_| bounceable)
    {
        let board = table("bounced", 5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, with_decoy(open(a)))
            .hand(P0, LAND_IN_HAND);
        let play = vec![
            Action::activate(P0, def.name, k),
            Action::cast(P1, SINK_INTO_STUPOR),
            Action::choose(P1, DecisionKind::Target, def.name),
        ];
        let title = format!("{}, bounced", cost_label(a, k));
        out.push(probe("bounced", &title, board, play));
    }
}

/// The board with a tapped Thieving Skydiver too: a second permanent for Sink into Stupor to
/// target, so that its target is asked rather than answered silently.
fn with_decoy(board: &'static [Permanent]) -> &'static [Permanent] {
    leak(
        board
            .iter()
            .copied()
            .chain(std::iter::once(Permanent::new(THIEVING_SKYDIVER).tapped()))
            .collect(),
    )
}

// The scenarios' moments.

/// Whether an action answers one decision, as a spec's scripts play them.
fn decides(action: &Action) -> bool {
    matches!(
        action,
        Action::PlayLand { .. }
            | Action::Cast { .. }
            | Action::Activate { .. }
            | Action::TurnFaceUp { .. }
            | Action::Answer { .. }
    )
}

/// A scenario's actions as a spec's scripts play them: its decisions and its turns to advance
/// to. Passing is what the scripts do anyway short of their next action, its assertions are
/// its own, and the session orders simultaneous triggers itself.
fn playable(scenario: &Scenario) -> Vec<Action> {
    scenario
        .actions
        .iter()
        .filter(|a| decides(a) || matches!(a, Action::AdvanceTo { .. }))
        .filter(|a| {
            !matches!(
                a,
                Action::Answer {
                    kind: DecisionKind::TriggerOrder | DecisionKind::Priority,
                    ..
                }
            )
        })
        .copied()
        .collect()
}

/// The scenario on a table that looks like a game: life off 20, graveyards, and the
/// opponent's lands and hand, where it has none of its own, lands under each library so that
/// the game goes on after its actions, and a twin of the card in the human's hand where it
/// starts there, so that some moment of every card in hand holds two cards just alike (which
/// the UI must keep apart: which one moves, which one is offered). Each part is kept only if
/// the scenario still passes, its assertions and its scripted decisions alike, so nothing
/// added changes it: the table without the twin, then the libraries, are tried alone if the
/// whole fails.
fn dressed(def: &'static CardDef, scenario: &Scenario) -> Scenario {
    let passes = |d: &Scenario| {
        let checked = d.actions(leak(d.actions.iter().map(deeper).collect()));
        beryl_scenario::run(&beryl_cards::CARDS, &checked).is_ok()
    };
    let deep = with_libraries(scenario);
    let full = with_table(&deep);
    let twinned = with_twin(def, &full);
    if twinned.is_some_and(|t| passes(&t)) {
        twinned.unwrap_or(full)
    } else if passes(&full) {
        full
    } else if passes(&deep) {
        deep
    } else {
        *scenario
    }
}

/// `scenario` with a second copy of `def` in the human's hand, if one starts there.
fn with_twin(def: &'static CardDef, scenario: &Scenario) -> Option<Scenario> {
    let hand = scenario.setup.players[P0.index()].hand;
    hand.contains(&def.name)
        .then(|| scenario.hand(P0, leak([hand, &[def.name]].concat())))
}

/// An assertion counting a whole library, counting the lands [`with_libraries`] adds too.
fn deeper(action: &Action) -> Action {
    match *action {
        Action::Assert {
            assertion:
                Assertion::Count {
                    player,
                    zone: beryl_engine::Zone::Library,
                    card: None,
                    count,
                },
        } => Action::assert(Assertion::Count {
            player,
            zone: beryl_engine::Zone::Library,
            card: None,
            count: count + u8::try_from(FILLER.len()).unwrap_or(u8::MAX),
        }),
        a => a,
    }
}

/// Cards under each player's library that neither seat can play from a scenario's board: no
/// land drop and no affordable spell, so whatever stops a player after the scenario is its
/// cards'.
const FILLER: &[&str] = &[
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
];

/// [`FILLER`] under each player's library.
fn with_libraries(scenario: &Scenario) -> Scenario {
    let mut d = *scenario;
    for seat in [P0, P1] {
        let library = d.setup.players[seat.index()].library;
        d = d.library(seat, leak([library, FILLER].concat()));
    }
    d
}

/// Life off 20, graveyards, and the opponent's lands and hand, where it has none of its own.
fn with_table(scenario: &Scenario) -> Scenario {
    const THEIRS: &[Permanent] = &[
        Permanent::new(ISLAND),
        Permanent::new(ISLAND),
        Permanent::new(ISLAND),
        Permanent::new(PLAINS).tapped(),
    ];
    let mut d = *scenario;
    let life_asserted = scenario.actions.iter().any(|a| {
        matches!(
            a,
            Action::Assert {
                assertion: Assertion::Life { .. }
            }
        )
    });
    if !life_asserted && d.setup.players.iter().all(|p| p.life == 20) {
        d = d.life(P0, 17).life(P1, 14);
    }
    let [mine, theirs] = d.setup.players;
    if mine.graveyard.is_empty() {
        d = d.graveyard(P0, &[STOCK_UP, REPRIEVE]);
    }
    if theirs.graveyard.is_empty() {
        d = d.graveyard(P1, &[OUST, MANA_DRAIN]);
    }
    if theirs.battlefield.is_empty() {
        d = d.battlefield(P1, THEIRS);
    }
    if theirs.hand.is_empty() {
        d = d.hand(P1, THEIR_HAND);
    }
    d
}

fn flipped(action: &Action) -> Option<(Action, &'static str)> {
    match *action {
        Action::Answer {
            player,
            kind: DecisionKind::YesNo,
            pick,
        } => match pick {
            Pick::Yes => Some((Action::answer(player, DecisionKind::YesNo, Pick::No), "No")),
            Pick::No => Some((
                Action::answer(player, DecisionKind::YesNo, Pick::Yes),
                "Yes",
            )),
            _ => None,
        },
        _ => None,
    }
}

fn from_scenario(def: &'static CardDef, scenario: &Scenario, out: &mut Vec<Candidate>) {
    let board = dressed(def, scenario);
    let group = heading(def, scenario);
    let actions = playable(scenario);
    let card = slug(def.name);
    let full = slug(scenario.name);
    let short = full
        .strip_prefix(&card)
        .map_or(full.as_str(), |s| s.trim_start_matches('-'))
        .to_owned();
    let at = |name: String, title: String, setup: &[Action], play: Vec<Action>, kind| Candidate {
        name,
        title,
        board: board.actions(leak(setup.to_vec())),
        cause: cause_of(setup)
            .filter(|_| play.is_empty())
            .or(cause_of(&play)),
        play,
        kind,
        group,
    };
    out.push(at(
        short.clone(),
        "Start".to_owned(),
        &[],
        actions.clone(),
        Kind::Whole,
    ));
    for (i, action) in actions.iter().enumerate() {
        let setup = &actions[..i];
        let mine = label_of(def, action);
        if matches!(*action,
            Action::Cast { player: P0, card, .. }
            | Action::PlayLand { player: P0, card }
            | Action::Activate { player: P0, card, .. } if card == def.name)
            && ways_to_play(def)
        {
            out.push(at(
                format!("{short}-ways-{i}"),
                "Ways to play".to_owned(),
                setup,
                Vec::new(),
                Kind::Ways,
            ));
        }
        if let Some(what) = mine.clone() {
            out.push(at(
                format!("{short}-{i}"),
                what,
                setup,
                vec![*action],
                Kind::Play,
            ));
        }
        if let Action::Activate {
            player: P0,
            card,
            ability: AbilityIx::Own(_),
        } = *action
            && card == def.name
            && !def.front().types.intersects(Types::LAND)
        {
            let play = bounce(def, &actions, i);
            out.push(Candidate {
                board: with_a_bounce(&board).actions(leak(setup.to_vec())),
                ..at(
                    format!("{short}-bounced-{i}"),
                    format!("{}, bounced", mine.clone().unwrap_or_default()),
                    setup,
                    play,
                    Kind::Bounced,
                )
            });
        }
        if let Action::Answer { kind, .. } = *action
            && kind != DecisionKind::Priority
        {
            let ask = out.len();
            out.push(at(
                format!("{short}-ask-{i}"),
                String::new(),
                setup,
                Vec::new(),
                Kind::Ask,
            ));
            if let Some((flip, answer)) = flipped(action) {
                let mut c = at(
                    format!("{short}-{}-{i}", answer.to_lowercase()),
                    format!("Answer {answer}"),
                    setup,
                    vec![flip],
                    Kind::Flip(ask),
                );
                c.cause = cause_of(setup);
                out.push(c);
            }
        }
    }
}

/// Whether a card in hand offers more than one way to play it: an alternative or additional
/// cost ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189): "Some spells have
/// alternative costs ... An alternative cost is a cost listed in a spell's text, or applied to
/// it from another effect, that its controller may pay rather than paying the spell's mana
/// cost"), an ability activated from the hand by discarding it (cycling, CR 702.29a), or a
/// second castable face.
fn ways_to_play(def: &'static CardDef) -> bool {
    let front = def.front();
    !front.casting.is_empty()
        || front
            .abilities
            .iter()
            .any(|a| a.kind == AbilityKind::Activated && a.cost.discard_self)
        || def.faces.len() > 1 && !def.faces.iter().skip(1).all(|f| f.transformed)
}

/// A scenario's heading: its name, less the card's name it starts with ("Exiles its own
/// loot's discard for a Rogue", "Ability returns a card after the Converter left").
fn heading(def: &'static CardDef, scenario: &Scenario) -> &'static str {
    let name = scenario.name;
    let rest = name.strip_prefix(def.name).map_or(name, |r| {
        r.strip_prefix("'s ")
            .or_else(|| r.strip_prefix(": "))
            .or_else(|| r.strip_prefix(' '))
            .unwrap_or(name)
    });
    let mut chars = rest.chars();
    let heading = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect::<String>())
        .unwrap_or_default();
    Box::leak(heading.into_boxed_str())
}

/// What a scenario's action of the card is, as its moment's label: "Cast", "{T}", "−3".
fn label_of(def: &'static CardDef, action: &Action) -> Option<String> {
    match *action {
        Action::Cast { card, .. } if card == def.name => Some("Cast".to_owned()),
        Action::PlayLand { card, .. } if card == def.name => Some("Play".to_owned()),
        Action::TurnFaceUp { card, .. } if card == def.name => Some("Turn up".to_owned()),
        Action::Activate {
            card,
            ability: AbilityIx::Own(k),
            ..
        } if card == def.name => Some(
            def.front()
                .abilities
                .get(usize::from(k))
                .map_or_else(|| format!("Ability {k}"), |a| cost_label(a, k)),
        ),
        _ => None,
    }
}

/// The activation at `i` with its own answers (its targets, its payment), then the opponent
/// bouncing the card with Sink into Stupor in response.
fn bounce(def: &'static CardDef, actions: &[Action], i: usize) -> Vec<Action> {
    let own = actions[i + 1..]
        .iter()
        .take_while(|a| {
            matches!(a, Action::Answer { player: P0, kind, .. } if *kind != DecisionKind::Priority)
        })
        .copied();
    std::iter::once(actions[i])
        .chain(own)
        .chain([
            Action::cast(P1, SINK_INTO_STUPOR),
            Action::choose(P1, DecisionKind::Target, def.name),
        ])
        .collect()
}

/// The board with Sink into Stupor in the opponent's hand and three Islands to cast it.
fn with_a_bounce(board: &Scenario) -> Scenario {
    let theirs = board.setup.players[P1.index()];
    let hand = leak([&[SINK_INTO_STUPOR][..], theirs.hand].concat());
    let lands = leak(
        [
            theirs.battlefield,
            &[
                Permanent::new(ISLAND),
                Permanent::new(ISLAND),
                Permanent::new(ISLAND),
            ][..],
        ]
        .concat(),
    );
    board.hand(P1, hand).battlefield(P1, lands)
}

// What holds at a moment's first live decision.

/// The conditions that hold of `view` at its first live decision, in the fixture conditions'
/// words: the turn, the decision, the stack, and the card's and its tokens' places. Each is
/// checked to hold, so the vocabulary and this stay in step.
pub fn snapshot(view: &PlayerView, def: &'static CardDef) -> Vec<String> {
    let mut out = Vec::new();
    let name_of = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot)
            .and_then(|o| o.name.clone())
    };
    let protocol = |v: serde_json::Value| v.as_str().map(str::to_owned).unwrap_or_default();
    out.push(format!("turn {}", view.turn.number));
    out.push(
        if view.turn.active == Side::You {
            "your turn"
        } else {
            "their turn"
        }
        .to_owned(),
    );
    out.push(format!("step {}", protocol(json!(view.turn.step))));
    if let Status::Deciding { decision } = &view.status {
        if !decision.halt {
            out.push(format!("decision {}", protocol(json!(decision.kind))));
        }
        if let Some(kind) = serde_json::to_value(&decision.ask)
            .ok()
            .and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_owned)))
            .filter(|k| k != "plain")
        {
            out.push(format!("ask {kind}"));
        }
        if json!(decision.kind) != json!("priority") {
            out.push(format!("{} candidates", decision.candidates.len()));
        }
        match &decision.resolving {
            Some(StackItem::Spell { slot, .. }) => {
                if let Some(n) = name_of(*slot) {
                    out.push(format!("resolving {n}"));
                }
            }
            Some(StackItem::Ability { name, .. }) => out.push(format!("resolving {name}")),
            None => {}
        }
        let offered: Vec<u8> = decision
            .candidates
            .iter()
            .filter_map(|c| match c.option {
                Offer::Activate {
                    slot,
                    ability,
                    granted: false,
                } if name_of(slot).as_deref() == Some(def.name) => Some(ability),
                _ => None,
            })
            .collect();
        let mine_on_battlefield = view.objects.iter().any(|o| {
            o.zone == ZoneName::Battlefield
                && o.controller == Side::You
                && o.name.as_deref() == Some(def.name)
        });
        if json!(decision.kind) == json!("priority") && mine_on_battlefield {
            for (k, _) in activated(def.front()) {
                if offered.contains(&k) {
                    out.push(format!("offers {} ability {k}", def.name));
                } else {
                    out.push(format!("not offers {} ability {k}", def.name));
                }
            }
        }
    }
    places(view, def, &mut out);
    let castable = format!("castable {}", def.name);
    let ways = format!("ways to play {}", def.name);
    for flag in [
        castable.as_str(),
        ways.as_str(),
        "attacking",
        "blocking",
        "castable from exile",
        "castable from the graveyard",
        "mana floating",
        "stolen permanent",
        "known card in the opponent's hand",
        "X on the stack",
        "emblems a side",
        "exiled beneath a permanent",
        "game over",
    ] {
        if Condition::parse(flag).is_ok_and(|c| c.holds(view)) {
            out.push(flag.to_owned());
        }
    }
    debug_assert!(
        out.iter()
            .all(|c| Condition::parse(c).is_ok_and(|c| c.holds(view))),
        "a recorded condition does not hold: {out:?}"
    );
    out
}

/// The stack's top, and where the card and its tokens are.
fn places(view: &PlayerView, def: &'static CardDef, out: &mut Vec<String>) {
    let name_of = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot)
            .and_then(|o| o.name.clone())
    };
    match view.stack.last() {
        Some(StackItem::Spell { slot, .. }) => {
            if let Some(n) = name_of(*slot) {
                out.push(format!("stack {n}"));
            }
        }
        Some(StackItem::Ability { name, .. }) => out.push(format!("stack {name}")),
        None => {}
    }
    let names = std::iter::once(def.name).chain(def.tokens.iter().map(|t| t.name));
    for name in names {
        let count = |side: Side| {
            view.objects
                .iter()
                .filter(|o| {
                    o.zone == ZoneName::Battlefield
                        && o.controller == side
                        && o.name.as_deref() == Some(name)
                })
                .count()
        };
        out.push(format!("controls {} {name}", count(Side::You)));
        let theirs = count(Side::Opponent);
        if theirs > 0 {
            out.push(format!("they control {theirs} {name}"));
        }
    }
    let tapped = view.objects.iter().filter(|o| {
        o.zone == ZoneName::Battlefield
            && o.controller == Side::You
            && o.name.as_deref() == Some(def.name)
    });
    let tapped: Vec<bool> = tapped.map(|o| o.tapped).collect();
    if tapped.iter().any(|&t| t) {
        out.push(format!("tapped {}", def.name));
    } else if !tapped.is_empty() {
        out.push(format!("not tapped {}", def.name));
    }
    if let Some(host) = view
        .objects
        .iter()
        .find(|o| o.zone == ZoneName::Battlefield && o.name.as_deref() == Some(def.name))
    {
        for c in &host.counters {
            let kind = serde_json::to_value(c.kind).unwrap_or_default();
            out.push(format!(
                "{} has {} {}",
                def.name,
                c.count,
                kind.as_str().unwrap_or("")
            ));
        }
        let n = view
            .objects
            .iter()
            .filter(|o| o.zone == ZoneName::Exile && o.exiled_with == Some(host.slot))
            .count();
        if n > 0 {
            out.push(format!("{n} exiled with {}", def.name));
        }
    }
}
