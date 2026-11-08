//! UI fixtures (decision D11): a seeded game between named agents, or a board of
//! [`crate::boards`] its script plays, played through the app's [`Sessions`] with an agent
//! standing in for the human seat, recorded as the messages the app would have sent the UI up
//! to a named stopping condition.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::channel;

use beryl_cards::CARDS;
use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_search::GameAgent;
use beryl_session::ipc::{self, Fixture, FixtureMessage};
use beryl_session::protocol::{
    Candidate, CandidateView, CardType, Combat, Counter, Kind, ObjectView, Side, StackItem, Step,
    TargetRef, ZoneName,
};
use beryl_session::{
    PlayerView, Prepared, Seat, SessionError, SessionId, Sessions, StartGame, Status, StopsSetting,
    agents,
};
use serde::Deserialize;
use serde_json::json;

use crate::boards::{self, Scripted, Shared};

/// The definitions file.
#[derive(Debug, Deserialize)]
struct Definitions {
    fixtures: Vec<Definition>,
}

/// One fixture: the game to play and where to stop.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    name: String,
    /// A seeded game's seed, with `decks`; or none, with `board`.
    seed: Option<u64>,
    /// Each seat's deck: `wu-control` (the mirror) or `fuzz:<seed>` for a fuzz deck.
    decks: Option<[String; 2]>,
    /// The board the game starts from, by its name in [`crate::boards::BOARDS`], in place of
    /// a seeded deal.
    board: Option<String>,
    /// The agent playing each seat of a seeded game. The stopping condition's seat is the
    /// human's. A board's own actions play it.
    agents: Option<[String; 2]>,
    stops: Option<StopsSetting>,
    until: String,
    /// What else must hold of the view the recording stops at (see [`Condition::parse`]).
    #[serde(default, rename = "where")]
    conditions: Vec<String>,
    /// How many more of the human's decisions to record past the stop, as the stand-in plays
    /// on: what the decision there led to.
    #[serde(default)]
    after: usize,
}

/// Where a recording stops: the human's first decision of a kind, or first halt
/// (`DecisionView::halt`), from a turn on, or the end of the game.
#[derive(Debug, PartialEq)]
enum Until {
    First { kind: Kind, seat: Seat, turn: u16 },
    Halt { seat: Seat, turn: u16 },
    GameOver,
}

impl Until {
    /// Parses "first <kind> decision for seat <n>" or "first halt for seat <n>", optionally
    /// followed by "from turn <t>", with the kind as the protocol names it ("target",
    /// "chooseUpTo"), or "game over". A decision of a kind is the engine's, never a halt.
    fn parse(text: &str) -> Result<Until, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let (words, turn) = match words.as_slice() {
            [first @ .., "from", "turn", turn] => (
                first,
                turn.parse()
                    .map_err(|_| format!("{text:?}: no turn {turn:?}"))?,
            ),
            all => (all, 0),
        };
        let seat = |seat: &str| {
            seat.parse()
                .ok()
                .and_then(Seat::try_new)
                .ok_or_else(|| format!("no seat {seat:?}"))
        };
        match words {
            ["game", "over"] if turn == 0 => Ok(Until::GameOver),
            ["first", kind, "decision", "for", "seat", n] => Ok(Until::First {
                turn,
                kind: serde_json::from_value(json!(kind))
                    .map_err(|_| format!("no decision kind is named {kind:?}"))?,
                seat: seat(n)?,
            }),
            ["first", "halt", "for", "seat", n] => Ok(Until::Halt {
                turn,
                seat: seat(n)?,
            }),
            _ => Err(format!(
                "{text:?}: expected \"first <kind> decision for seat <n> [from turn <t>]\", \
                 \"first halt for seat <n> [from turn <t>]\" or \"game over\""
            )),
        }
    }

    fn seat(&self) -> Seat {
        match self {
            Until::First { seat, .. } | Until::Halt { seat, .. } => *seat,
            Until::GameOver => Seat::P0,
        }
    }

    fn reached(&self, view: &PlayerView) -> bool {
        let decision = match &view.status {
            Status::Deciding { decision } => Some(decision),
            _ => None,
        };
        match self {
            Until::First { kind, turn, .. } => {
                view.turn.number >= *turn && decision.is_some_and(|d| d.kind == *kind && !d.halt)
            }
            Until::Halt { turn, .. } => {
                view.turn.number >= *turn && decision.is_some_and(|d| d.halt)
            }
            Until::GameOver => matches!(view.status, Status::Over { .. }),
        }
    }
}

/// A condition on the view a recording stops at, so a fixture can stop at a state rather
/// than only at a kind of decision.
#[derive(Debug, PartialEq)]
pub(crate) enum Condition {
    /// The turn is at this step.
    Step(Step),
    /// It is this turn.
    Turn(u16),
    /// It is the human's turn (true) or the opponent's.
    YourTurn(bool),
    /// The human's open decision is of this kind.
    Decision(Kind),
    /// The human's open decision asks this ([`beryl_session::protocol::AskView`]'s kind, as
    /// the protocol names it: "mayMove").
    Ask(String),
    /// The human's open decision has this many candidates.
    Candidates(usize),
    /// A permanent of this name the human controls is tapped.
    Tapped(String),
    /// The human controls this many permanents of this name.
    Controls(usize, String),
    /// The opponent controls this many permanents of this name.
    TheyControl(usize, String),
    /// The human's priority offers activating the ability at this index of a permanent of
    /// this name ([`Candidate::Activate`]).
    Offers(String, u8),
    /// This many cards lie in exile "exiled with" a permanent of this name.
    ExiledWithCard(usize, String),
    /// The condition does not hold.
    Not(Box<Condition>),
    /// Some creature is attacking.
    Attacking,
    /// Some creature is blocking.
    Blocking,
    /// The top of the stack is a triggered ability with a target.
    TargetedTrigger,
    /// The top of the stack is a spell that targets a permanent.
    TargetedSpell,
    /// The top of the stack targets a player.
    PlayerTarget,
    /// The top of the stack is an ability of a permanent that targets another permanent its
    /// controller controls, which the table sets in the same row.
    NeighbourTarget,
    /// The top of the stack is a spell the opponent controls.
    OpponentsSpell,
    /// The human may cast a card of this name.
    Castable(String),
    /// The human may play a card of this name as a land.
    Playable(String),
    /// The human may play a card of this name off the battlefield more than one way: cast with
    /// another cost, played as a land, or an ability activated from where it lies (cycling).
    Ways(String),
    /// The human may activate more than one ability of a permanent of this name, so clicking
    /// it opens a menu.
    Abilities(String),
    /// The open decision offers two objects of one name.
    NamesakeCandidates,
    /// The open decision is asked by the resolution of a spell of this name or an ability of
    /// a card of this name.
    Resolving(String),
    /// The top of the stack is a spell of this name or an ability of a card of this name.
    OnStack(String),
    /// The open decision is asked by the casting of a spell of this name: its X, a target or
    /// its payment.
    Casting(String),
    /// The human may play or cast a card from their hand of which they hold another just like
    /// it, which the engine offers as one candidate for both (architecture §7.2).
    PlayablePair,
    /// A card lies in exile beneath the permanent that exiled it until it leaves (CR 610.3).
    ExiledBeneath,
    /// A card lies in exile "exiled with" a permanent whose abilities may find it
    /// (CR 607.2a).
    ExiledWith,
    /// An Aura or Equipment one player controls is attached to a permanent the other
    /// controls.
    AttachedAcross,
    /// A permanent bears this mark and no other while another of the same name bears none, so
    /// the table shows the mark alone beside a plain twin.
    Marked(Mark),
    /// The human may cast a card from exile, or from their graveyard.
    CastableFrom(ZoneName),
    /// The human knows a card in the opponent's hand.
    KnownOpponentsHand,
    /// Two tokens one player controls are alike in all the table shows, so it piles them.
    TokenTwins,
    /// A token differs from another of its name its controller controls by its types or an
    /// attachment, so the table must not pile them.
    ChangedToken,
    /// A token stands beside a card of its name under one controller, as an offspring copy.
    TokenBesideCard,
    /// A permanent is controlled by the player who doesn't own it.
    Stolen,
    /// The human has unspent mana (CR 106.4).
    ManaFloating,
    /// A spell or ability on the stack has an announced X above 0.
    XOnStack,
    /// Each player controls at least this many nonland permanents.
    Crowded(usize),
    /// The top of the stack is a spell.
    SpellOnStack,
    /// Each player has an emblem.
    Emblems,
    /// The game is over.
    GameOver,
    /// A permanent of this name has this many counters of this kind.
    Has(String, u8, Counter),
}

/// What the table draws on a permanent beyond its frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mark {
    /// +1/+1 counters on a creature.
    PlusOne,
    /// Damage marked on a creature (CR 120.3).
    Damage,
    /// A creature its controller can't attack with yet (CR 302.6).
    Sick,
    /// Story or time counters on a permanent.
    StoryOrTime,
}

impl Mark {
    const ALL: [Mark; 4] = [Mark::PlusOne, Mark::Damage, Mark::Sick, Mark::StoryOrTime];

    /// The kinds of counter this mark is.
    fn kinds(self) -> &'static [Counter] {
        match self {
            Mark::PlusOne => &[Counter::PlusOne],
            Mark::StoryOrTime => &[Counter::Story, Counter::Time],
            Mark::Damage | Mark::Sick => &[],
        }
    }

    /// Whether `o` bears exactly the mark `only`, or none: no other mark, and no counter but
    /// loyalty (which its plaque shows) and the mark's own.
    fn only(only: Option<Mark>, o: &ObjectView) -> bool {
        Mark::ALL.iter().all(|&m| m.on(o) == (Some(m) == only))
            && o.counters.iter().all(|c| {
                c.kind == Counter::Loyalty || only.is_some_and(|m| m.kinds().contains(&c.kind))
            })
    }

    fn on(self, o: &ObjectView) -> bool {
        let creature = o.types.contains(&CardType::Creature);
        let counters = |kinds: &[Counter]| o.counters.iter().any(|c| kinds.contains(&c.kind));
        match self {
            Mark::PlusOne => creature && counters(self.kinds()),
            Mark::Damage => creature && o.damage > 0,
            Mark::Sick => creature && o.summoning_sick,
            Mark::StoryOrTime => counters(self.kinds()),
        }
    }
}

impl Condition {
    /// Parses "step <step>" (as the protocol names steps: "declareBlockers"), "attacking",
    /// "blocking", "targeted trigger on the stack", "targeted spell on the stack", "player
    /// targeted on the stack", "ability targeting beside its source", "opponent's spell on
    /// the stack", "castable <card name>", "playable <card name>", "ways to play <card name>", "abilities of <card name>",
    /// "namesake candidates", "resolving <card name>", "stack <card name>", "casting <card name>",
    /// "playable pair in hand", "exiled beneath a permanent", "exiled with a permanent",
    /// "attached across the table", "castable from exile", "castable from the graveyard",
    /// "known card in the opponent's hand", "token twins", "changed token", "token beside its
    /// card", "stolen permanent", "mana floating", "X on the stack", "spell on the stack",
    /// "emblems a side", "game over", "<card> has <n> <counter kind>" (as the protocol names
    /// counters: "loyalty"), "<n> nonland permanents a side", or "<mark> beside a twin" with the
    /// mark "+1/+1 counters", "damage", "summoning sickness" or "story or time counters".
    /// Specs add "turn <n>", "your turn", "their turn", "decision <kind>", "ask <kind>",
    /// "<n> candidates", "tapped <card>", "controls <n> <card>", "they control <n> <card>", "offers <card> ability <n>",
    /// "<n> exiled with <card>" and "not <condition>".
    pub(crate) fn parse(text: &str) -> Result<Condition, String> {
        if let Some(rest) = text.strip_prefix("not ") {
            return Ok(Condition::Not(Box::new(Condition::parse(rest)?)));
        }
        if let Some((n, card)) = text.split_once(" exiled with ")
            && let Ok(n) = n.parse()
        {
            return Ok(Condition::ExiledWithCard(n, card.to_owned()));
        }
        if let Some(rest) = text.strip_prefix("offers ")
            && let Some((card, n)) = rest.rsplit_once(" ability ")
        {
            return Ok(Condition::Offers(card.to_owned(), number(text, n)?));
        }
        if let Some(rest) = text.strip_prefix("controls ")
            && let Some((n, card)) = rest.split_once(' ')
        {
            return Ok(Condition::Controls(number(text, n)?, card.to_owned()));
        }
        if let Some(rest) = text.strip_prefix("they control ")
            && let Some((n, card)) = rest.split_once(' ')
        {
            return Ok(Condition::TheyControl(number(text, n)?, card.to_owned()));
        }
        if let Some(has) = Condition::parse_has(text) {
            return has;
        }
        if let Some(n) = text.strip_suffix(" candidates")
            && let Ok(n) = n.parse()
        {
            return Ok(Condition::Candidates(n));
        }
        let condition = match text {
            "your turn" => Condition::YourTurn(true),
            "their turn" => Condition::YourTurn(false),
            "attacking" => Condition::Attacking,
            "blocking" => Condition::Blocking,
            "targeted trigger on the stack" => Condition::TargetedTrigger,
            "targeted spell on the stack" => Condition::TargetedSpell,
            "player targeted on the stack" => Condition::PlayerTarget,
            "ability targeting beside its source" => Condition::NeighbourTarget,
            "opponent's spell on the stack" => Condition::OpponentsSpell,
            "playable pair in hand" => Condition::PlayablePair,
            "exiled beneath a permanent" => Condition::ExiledBeneath,
            "exiled with a permanent" => Condition::ExiledWith,
            "attached across the table" => Condition::AttachedAcross,
            "+1/+1 counters beside a twin" => Condition::Marked(Mark::PlusOne),
            "damage beside a twin" => Condition::Marked(Mark::Damage),
            "summoning sickness beside a twin" => Condition::Marked(Mark::Sick),
            "story or time counters beside a twin" => Condition::Marked(Mark::StoryOrTime),
            "castable from exile" => Condition::CastableFrom(ZoneName::Exile),
            "castable from the graveyard" => Condition::CastableFrom(ZoneName::Graveyard),
            "known card in the opponent's hand" => Condition::KnownOpponentsHand,
            "token twins" => Condition::TokenTwins,
            "changed token" => Condition::ChangedToken,
            "token beside its card" => Condition::TokenBesideCard,
            "stolen permanent" => Condition::Stolen,
            "mana floating" => Condition::ManaFloating,
            "X on the stack" => Condition::XOnStack,
            "namesake candidates" => Condition::NamesakeCandidates,
            "spell on the stack" => Condition::SpellOnStack,
            "emblems a side" => Condition::Emblems,
            "game over" => Condition::GameOver,
            _ => {
                if let Some(n) = text.strip_prefix("turn ") {
                    Condition::Turn(number(text, n)?)
                } else if let Some(kind) = text.strip_prefix("decision ") {
                    Condition::Decision(
                        serde_json::from_value(json!(kind))
                            .map_err(|_| format!("no decision kind is named {kind:?}"))?,
                    )
                } else if let Some(kind) = text.strip_prefix("ask ") {
                    Condition::Ask(kind.to_owned())
                } else if let Some(card) = text.strip_prefix("tapped ") {
                    Condition::Tapped(card.to_owned())
                } else if let Some(step) = text.strip_prefix("step ") {
                    Condition::Step(
                        serde_json::from_value(json!(step))
                            .map_err(|_| format!("no step is named {step:?}"))?,
                    )
                } else if let Some(name) = text.strip_prefix("castable ") {
                    Condition::Castable(name.to_owned())
                } else if let Some(name) = text.strip_prefix("ways to play ") {
                    Condition::Ways(name.to_owned())
                } else if let Some(name) = text.strip_prefix("playable ") {
                    Condition::Playable(name.to_owned())
                } else if let Some(name) = text.strip_prefix("abilities of ") {
                    Condition::Abilities(name.to_owned())
                } else if let Some(name) = text.strip_prefix("resolving ") {
                    Condition::Resolving(name.to_owned())
                } else if let Some(name) = text.strip_prefix("stack ") {
                    Condition::OnStack(name.to_owned())
                } else if let Some(name) = text.strip_prefix("casting ") {
                    Condition::Casting(name.to_owned())
                } else if let Some(n) = text.strip_suffix(" nonland permanents a side") {
                    Condition::Crowded(n.parse().map_err(|_| format!("bad count {n:?}"))?)
                } else {
                    return Err(format!("{text:?}: no such condition"));
                }
            }
        };
        Ok(condition)
    }

    /// "<card> has <n> <counter kind>", if `text` says that.
    fn parse_has(text: &str) -> Option<Result<Condition, String>> {
        let (card, rest) = text.rsplit_once(" has ")?;
        let (n, kind) = rest.split_once(' ')?;
        let n = n.parse().ok()?;
        Some(
            serde_json::from_value(json!(kind))
                .map(|kind| Condition::Has(card.to_owned(), n, kind))
                .map_err(|_| format!("{text:?}: no counter is named {kind:?}")),
        )
    }

    pub(crate) fn holds(&self, view: &PlayerView) -> bool {
        let name = |slot: u16| {
            view.objects
                .iter()
                .find(|o| o.slot == slot)
                .and_then(|o| o.name.as_deref())
        };
        match self {
            Condition::Step(step) => view.turn.step == *step,
            Condition::Attacking => view
                .objects
                .iter()
                .any(|o| matches!(o.combat, Some(Combat::Attacking { .. }))),
            Condition::Blocking => view
                .objects
                .iter()
                .any(|o| matches!(o.combat, Some(Combat::Blocking { .. }))),
            Condition::TargetedTrigger => matches!(
                view.stack.last(),
                Some(StackItem::Ability { triggered: true, targets, .. }) if !targets.is_empty()
            ),
            Condition::TargetedSpell => matches!(
                view.stack.last(),
                Some(StackItem::Spell { targets, .. })
                    if targets.iter().any(|t| matches!(t, TargetRef::Object { .. }))
            ),
            Condition::PlayerTarget => view.stack.last().is_some_and(|item| {
                let (StackItem::Spell { targets, .. } | StackItem::Ability { targets, .. }) = item;
                targets
                    .iter()
                    .any(|t| matches!(t, TargetRef::Player { .. }))
            }),
            Condition::NeighbourTarget => targets_beside_source(view),
            Condition::OpponentsSpell => matches!(
                view.stack.last(),
                Some(StackItem::Spell { slot, .. })
                    if view.objects.iter().any(|o| o.slot == *slot && o.controller == Side::Opponent)
            ),
            Condition::Castable(card) => matches!(
                &view.status,
                Status::Deciding { decision } if decision.candidates.iter().any(|c| matches!(
                    c.option,
                    Candidate::Cast { slot, .. } if name(slot) == Some(card.as_str())
                ))
            ),
            Condition::Playable(card) => candidates(view).any(|c| {
                matches!(c.option, Candidate::PlayLand { slot } if name(slot) == Some(card.as_str()))
            }),
            Condition::Ways(card) => {
                let off_battlefield = |slot: u16| {
                    view.objects.iter().any(|o| {
                        o.slot == slot
                            && o.zone != ZoneName::Battlefield
                            && o.name.as_deref() == Some(card.as_str())
                    })
                };
                candidates(view)
                    .filter(|c| match c.option {
                        Candidate::Cast { slot, .. }
                        | Candidate::PlayLand { slot }
                        | Candidate::Activate { slot, .. } => off_battlefield(slot),
                        _ => false,
                    })
                    .count()
                    > 1
            }
            Condition::Abilities(card) => abilities_of(view, card),
            Condition::NamesakeCandidates => namesake_candidates(view),
            Condition::Resolving(card) => match &view.status {
                Status::Deciding { decision } => match &decision.resolving {
                    Some(StackItem::Spell { slot, .. }) => name(*slot) == Some(card.as_str()),
                    Some(StackItem::Ability { name, .. }) => name == card,
                    None => false,
                },
                _ => false,
            },
            Condition::Casting(card) => match &view.status {
                Status::Deciding { decision } => {
                    decision.resolving.is_none()
                        && decision.source.is_some_and(|slot| {
                            name(slot) == Some(card.as_str())
                                && view
                                    .objects
                                    .iter()
                                    .any(|o| o.slot == slot && o.zone == ZoneName::Stack)
                        })
                }
                _ => false,
            },
            Condition::PlayablePair => playable_pair(view),
            Condition::ExiledBeneath => exiled_by_permanent(view, |o| o.exiled_by),
            Condition::ExiledWith => exiled_by_permanent(view, |o| o.exiled_with),
            Condition::AttachedAcross => attached_across(view),
            Condition::Marked(mark) => marked_beside_twin(view, *mark),
            Condition::CastableFrom(zone) => castable_from(view, *zone),
            Condition::KnownOpponentsHand => knows_opponents_hand(view),
            Condition::TokenTwins => token_pairs(view).any(|(a, b)| plain_twins(view, a, b)),
            Condition::ChangedToken => changed_token(view),
            Condition::TokenBesideCard => token_beside_card(view),
            Condition::Stolen => stolen(view),
            Condition::ManaFloating => mana_floating(view),
            Condition::XOnStack => x_on_stack(view),
            Condition::Crowded(n) => crowded(view, *n),
            Condition::SpellOnStack => matches!(view.stack.last(), Some(StackItem::Spell { .. })),
            Condition::Emblems => emblems_a_side(view),
            // The conditions specs add.
            _ => self.holds_for_spec(view),
        }
    }
    /// Whether one of the conditions specs add holds.
    fn holds_for_spec(&self, view: &PlayerView) -> bool {
        let name = |slot: u16| {
            view.objects
                .iter()
                .find(|o| o.slot == slot)
                .and_then(|o| o.name.as_deref())
        };
        let decision = match &view.status {
            Status::Deciding { decision } => Some(decision),
            _ => None,
        };
        match self {
            Condition::Not(c) => !c.holds(view),
            Condition::Turn(n) => view.turn.number == *n,
            Condition::YourTurn(yours) => (view.turn.active == Side::You) == *yours,
            Condition::Decision(kind) => decision.is_some_and(|d| d.kind == *kind && !d.halt),
            Condition::Ask(kind) => decision.is_some_and(|d| {
                serde_json::to_value(&d.ask)
                    .ok()
                    .and_then(|v| v.get("kind").cloned())
                    == Some(json!(kind))
            }),
            Condition::Candidates(n) => decision.is_some_and(|d| d.candidates.len() == *n),
            Condition::Tapped(card) => mine(view, card).any(|o| o.tapped),
            Condition::Controls(n, card) => mine(view, card).count() == *n,
            Condition::TheyControl(n, card) => theirs(view, card).count() == *n,
            Condition::OnStack(card) => match view.stack.last() {
                Some(StackItem::Spell { slot, .. }) => name(*slot) == Some(card.as_str()),
                Some(StackItem::Ability { name, .. }) => name == card,
                None => false,
            },
            Condition::Offers(card, ability) => candidates(view).any(|c| {
                matches!(c.option, Candidate::Activate { slot, ability: a, granted: false }
                    if a == *ability && name(slot) == Some(card.as_str()))
            }),
            Condition::ExiledWithCard(n, card) => view
                .objects
                .iter()
                .filter(|o| o.zone == ZoneName::Battlefield && o.name.as_deref() == Some(card))
                .any(|host| {
                    view.objects
                        .iter()
                        .filter(|o| o.zone == ZoneName::Exile && o.exiled_with == Some(host.slot))
                        .count()
                        == *n
                }),
            Condition::GameOver => matches!(view.status, Status::Over { .. }),
            Condition::Has(card, n, kind) => view.objects.iter().any(|o| {
                o.zone == ZoneName::Battlefield
                    && o.name.as_deref() == Some(card)
                    && o.counters
                        .iter()
                        .find(|c| c.kind == *kind)
                        .map_or(0, |c| c.count)
                        == *n
            }),
            _ => unreachable!("not a spec's condition"),
        }
    }
}

/// Whether the human may activate more than one ability of a permanent named `card`.
fn abilities_of(view: &PlayerView, card: &str) -> bool {
    view.battlefield
        .iter()
        .filter(|&&slot| {
            view.objects
                .iter()
                .any(|o| o.slot == slot && o.name.as_deref() == Some(card))
        })
        .any(|&slot| {
            candidates(view)
                .filter(|c| matches!(c.option, Candidate::Activate { slot: s, .. } if s == slot))
                .count()
                > 1
        })
}

/// Whether the open decision offers two objects of one name.
fn namesake_candidates(view: &PlayerView) -> bool {
    let names: Vec<_> = candidates(view)
        .filter_map(|c| match c.option {
            Candidate::Object { slot } => view
                .objects
                .iter()
                .find(|o| o.slot == slot)
                .and_then(|o| o.name.as_deref()),
            _ => None,
        })
        .collect();
    names
        .iter()
        .enumerate()
        .any(|(i, n)| names[i + 1..].contains(n))
}

/// The open decision's candidates, if any.
fn candidates(view: &PlayerView) -> impl Iterator<Item = &CandidateView> {
    match &view.status {
        Status::Deciding { decision } => decision.candidates.as_slice(),
        _ => &[],
    }
    .iter()
}

/// Whether the human may play or cast a card from their hand of which they hold another just
/// like it.
fn playable_pair(view: &PlayerView) -> bool {
    let Status::Deciding { decision } = &view.status else {
        return false;
    };
    let in_hand = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot && o.zone == ZoneName::Hand)
    };
    decision.candidates.iter().any(|c| {
        let (Candidate::Cast { slot, .. } | Candidate::PlayLand { slot }) = c.option else {
            return false;
        };
        in_hand(slot).is_some_and(|card| {
            view.objects.iter().any(|o| {
                o.slot != slot
                    && o.zone == ZoneName::Hand
                    && o.owner == Side::You
                    && o.name.is_some()
                    && o.name == card.name
            })
        })
    })
}

/// Whether a card in exile links, by `host`, to a permanent on the battlefield.
fn exiled_by_permanent(view: &PlayerView, host: impl Fn(&ObjectView) -> Option<u16>) -> bool {
    view.objects.iter().any(|o| {
        o.zone == ZoneName::Exile
            && host(o).is_some_and(|host| {
                view.objects
                    .iter()
                    .any(|h| h.slot == host && h.zone == ZoneName::Battlefield)
            })
    })
}

/// Whether each player has an emblem (CR 114.1).
fn emblems_a_side(view: &PlayerView) -> bool {
    !view.players.you.emblems.is_empty() && !view.players.opponent.emblems.is_empty()
}

/// Whether the human has unspent mana (CR 106.4).
fn mana_floating(view: &PlayerView) -> bool {
    let pool = view.players.you.mana;
    [
        pool.white,
        pool.blue,
        pool.black,
        pool.red,
        pool.green,
        pool.colorless,
    ]
    .iter()
    .any(|&n| n > 0)
}

/// Whether a spell or ability on the stack was announced with X above 0.
fn x_on_stack(view: &PlayerView) -> bool {
    view.stack.iter().any(|item| {
        let (StackItem::Spell { x, .. } | StackItem::Ability { x, .. }) = item;
        *x > 0
    })
}

/// Whether a permanent one player controls is attached to a permanent the other controls.
fn attached_across(view: &PlayerView) -> bool {
    view.objects.iter().any(|o| {
        o.zone == ZoneName::Battlefield
            && o.attached_to.is_some_and(|host| {
                view.objects.iter().any(|h| {
                    h.slot == host
                        && h.zone == ZoneName::Battlefield
                        && h.controller != o.controller
                })
            })
    })
}

/// Whether a permanent bears `mark` and no other while another of the same name and types,
/// and a token if it is one, bears none.
fn marked_beside_twin(view: &PlayerView, mark: Mark) -> bool {
    let field = || {
        view.objects
            .iter()
            .filter(|o| o.zone == ZoneName::Battlefield && o.name.is_some())
    };
    field().any(|o| {
        Mark::only(Some(mark), o)
            && field().any(|t| {
                t.slot != o.slot
                    && t.name == o.name
                    && t.token == o.token
                    && t.types == o.types
                    && Mark::only(None, t)
            })
    })
}

/// Every pair of distinct tokens of one name that one player controls on the battlefield.
fn token_pairs(view: &PlayerView) -> impl Iterator<Item = (&ObjectView, &ObjectView)> {
    let tokens = || {
        view.objects
            .iter()
            .filter(|o| o.zone == ZoneName::Battlefield && o.token && o.name.is_some())
    };
    tokens().flat_map(move |a| {
        tokens()
            .filter(move |b| a.slot < b.slot && a.name == b.name && a.controller == b.controller)
            .map(move |b| (a, b))
    })
}

/// Whether anything on the battlefield is attached to `o`.
fn bears_attachment(view: &PlayerView, o: &ObjectView) -> bool {
    view.objects
        .iter()
        .any(|a| a.zone == ZoneName::Battlefield && a.attached_to == Some(o.slot))
}

/// Whether two tokens are alike in all the table shows and bear nothing: no counter, damage,
/// combat or attachment.
fn plain_twins(view: &PlayerView, a: &ObjectView, b: &ObjectView) -> bool {
    let plain = |o: &ObjectView| {
        o.counters.is_empty()
            && o.damage == 0
            && o.combat.is_none()
            && o.attached_to.is_none()
            && !bears_attachment(view, o)
    };
    plain(a)
        && plain(b)
        && a.tapped == b.tapped
        && a.summoning_sick == b.summoning_sick
        && a.types == b.types
        && a.pt == b.pt
}

/// Whether a token differs from another of its name its controller controls by its types or
/// an attachment.
fn changed_token(view: &PlayerView) -> bool {
    token_pairs(view).any(|(a, b)| {
        a.types != b.types
            || [a, b]
                .iter()
                .any(|o| o.attached_to.is_some() || bears_attachment(view, o))
    })
}

/// Whether a token stands beside a card of its name under one controller.
fn token_beside_card(view: &PlayerView) -> bool {
    let field = || {
        view.objects
            .iter()
            .filter(|o| o.zone == ZoneName::Battlefield)
    };
    field().any(|t| {
        t.token
            && t.name.is_some()
            && field().any(|c| !c.token && c.name == t.name && c.controller == t.controller)
    })
}

/// Whether each player controls at least `n` nonland permanents.
fn crowded(view: &PlayerView, n: usize) -> bool {
    [Side::You, Side::Opponent].iter().all(|side| {
        view.objects
            .iter()
            .filter(|o| {
                o.zone == ZoneName::Battlefield
                    && o.controller == *side
                    && !o.types.contains(&CardType::Land)
            })
            .count()
            >= n
    })
}

/// Whether a permanent is controlled by the player who doesn't own it.
fn stolen(view: &PlayerView) -> bool {
    view.objects
        .iter()
        .any(|o| o.zone == ZoneName::Battlefield && o.controller != o.owner)
}

/// Whether the human may cast a card from `zone`.
fn castable_from(view: &PlayerView, zone: ZoneName) -> bool {
    let Status::Deciding { decision } = &view.status else {
        return false;
    };
    decision.candidates.iter().any(|c| {
        matches!(c.option, Candidate::Cast { slot, .. } if view
            .objects
            .iter()
            .any(|o| o.slot == slot && o.zone == zone))
    })
}

/// Whether the human knows a card in the opponent's hand.
fn knows_opponents_hand(view: &PlayerView) -> bool {
    view.objects
        .iter()
        .any(|o| o.zone == ZoneName::Hand && o.owner == Side::Opponent && o.name.is_some())
}

/// Whether the top of the stack is an ability of a permanent that targets another permanent
/// its controller controls.
fn targets_beside_source(view: &PlayerView) -> bool {
    let Some(StackItem::Ability {
        source: Some(source),
        targets,
        ..
    }) = view.stack.last()
    else {
        return false;
    };
    let permanent = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot && o.zone == ZoneName::Battlefield)
    };
    permanent(*source).is_some_and(|s| {
        targets.iter().any(|t| {
            matches!(t, TargetRef::Object { slot } if *slot != *source
                && permanent(*slot).is_some_and(|o| o.controller == s.controller))
        })
    })
}

/// Records the fixtures in `definitions` named in `only`, or all of them, into `out`.
pub fn record_all(definitions: &Path, out: &Path, only: &[String]) -> Result<(), String> {
    let text = std::fs::read_to_string(definitions)
        .map_err(|e| format!("{}: {e}", definitions.display()))?;
    let defs: Definitions =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", definitions.display()))?;
    if let Some(name) = only
        .iter()
        .find(|n| !defs.fixtures.iter().any(|f| &f.name == *n))
    {
        return Err(format!("no fixture is named {name:?}"));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for def in &defs.fixtures {
        if !only.is_empty() && !only.contains(&def.name) {
            continue;
        }
        let messages = record(def).map_err(|e| format!("fixture {}: {e}", def.name))?;
        let path = out.join(format!("{}.json", def.name));
        let json = serde_json::to_string(&Fixture { messages }).expect("a fixture serializes");
        std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

fn record(def: &Definition) -> Result<Vec<FixtureMessage>, String> {
    let until = Until::parse(&def.until)?;
    let conditions = def
        .conditions
        .iter()
        .map(|c| Condition::parse(c))
        .collect::<Result<Vec<_>, _>>()?;
    let human = until.seat();
    let (sent, updates) = channel();
    let sessions = Sessions::new(None, move |update| {
        // The recording stops listening once it has what it came for.
        let _ = sent.send(update);
    });
    let progress = Shared::default();
    let session = start(def, &sessions, human, &progress)?;
    let mut messages = vec![
        FixtureMessage::response(ipc::LIST_AGENTS, &agents()),
        FixtureMessage::response(ipc::START_GAME, &session),
        // A stops change returns nothing; its view is the open decision's again.
        FixtureMessage::response(ipc::SET_STOPS, &()),
    ];
    // The human's decisions still to record once the stop is reached.
    let mut left = None;
    loop {
        let update = updates.recv();
        if let Some(failure) = boards::lock(&progress).failure.take() {
            return Err(failure);
        }
        let update = update.map_err(|_| "the session ended without a view".to_owned())?;
        let reached = left.is_none()
            && until.reached(&update.view)
            && conditions.iter().all(|c| c.holds(&update.view));
        let status = &update.view.status;
        let (deciding, ended) = (
            matches!(status, Status::Deciding { .. }),
            matches!(status, Status::Over { .. } | Status::Stopped),
        );
        messages.push(FixtureMessage::game_update(&update));
        if reached {
            left = Some(def.after);
        } else if let Some(n) = left.filter(|_| deciding) {
            left = Some(n - 1);
        }
        if left == Some(0) || (ended && left.is_some()) {
            // A board's actions all lead up to where its recording ends.
            if let Some(action) = boards::lock(&progress).next.iter().flatten().next() {
                return Err(format!("the board's {action:?} was never taken"));
            }
            return Ok(messages);
        }
        if ended {
            return Err(format!("the game ended before {:?}", def.until));
        }
        if deciding {
            // The stand-in answered; the UI's `choose` would have returned nothing.
            messages.push(FixtureMessage::response(ipc::CHOOSE, &()));
        }
    }
}

/// Starts the fixture's game in `sessions`: its seeded deal, or its board, whose scripts
/// report to `progress`.
fn start(
    def: &Definition,
    sessions: &Sessions,
    human: Seat,
    progress: &Shared,
) -> Result<SessionId, String> {
    let stops = def.stops.unwrap_or(StopsSetting::SMART);
    let started: Result<SessionId, SessionError> =
        match (&def.board, def.seed, &def.decks, &def.agents) {
            (None, Some(seed), Some(decks), Some(agents)) => sessions.start_game(
                &StartGame {
                    seed,
                    decks: [deck(&decks[0])?, deck(&decks[1])?],
                    human,
                    opponent: agents[human.opponent().index()].clone(),
                    stops,
                },
                Some(&agents[human.index()]),
            ),
            (Some(name), None, None, None) => {
                let board = boards::named(name)?;
                let (table, state) =
                    beryl_scenario::position(&CARDS, board).map_err(|e| e.to_string())?;
                let agent = |seat: Seat| {
                    Scripted::new(board, seat, &table, Arc::clone(progress))
                        .map(|a| Box::new(a) as Box<dyn GameAgent + Send>)
                };
                sessions.start_prepared(Prepared {
                    opponent: agent(human.opponent())?,
                    stand_in: agent(human)?,
                    scripted: true,
                    hand_over: None,
                    table,
                    state,
                    human,
                    stops,
                })
            }
            _ => {
                return Err("a fixture has a seed, decks and agents, or a board".to_owned());
            }
        };
    started.map_err(|e| e.to_string())
}

fn deck(name: &str) -> Result<Vec<(u8, String)>, String> {
    let list = if name == "wu-control" {
        WU_CONTROL.to_vec()
    } else if let Some(seed) = name.strip_prefix("fuzz:") {
        fuzz_deck(
            seed.parse()
                .map_err(|_| format!("bad fuzz seed {seed:?}"))?,
        )
    } else {
        return Err(format!("no deck is named {name:?}"));
    };
    Ok(list.into_iter().map(|(n, c)| (n, c.to_owned())).collect())
}

fn number<T: std::str::FromStr>(text: &str, n: &str) -> Result<T, String> {
    n.parse().map_err(|_| format!("{text:?}: bad number {n:?}"))
}

/// The permanents named `card` the human controls.
fn mine<'a>(view: &'a PlayerView, card: &'a str) -> impl Iterator<Item = &'a ObjectView> {
    controlled(view, card, Side::You)
}

fn theirs<'a>(view: &'a PlayerView, card: &'a str) -> impl Iterator<Item = &'a ObjectView> {
    controlled(view, card, Side::Opponent)
}

fn controlled<'a>(
    view: &'a PlayerView,
    card: &'a str,
    side: Side,
) -> impl Iterator<Item = &'a ObjectView> {
    view.objects.iter().filter(move |o| {
        o.zone == ZoneName::Battlefield && o.controller == side && o.name.as_deref() == Some(card)
    })
}

#[cfg(test)]
mod tests {
    use beryl_session::GameUpdate;

    use super::*;

    #[test]
    fn stopping_conditions_parse() {
        assert_eq!(
            Until::parse("first chooseUpTo decision for seat 1"),
            Ok(Until::First {
                kind: Kind::ChooseUpTo,
                seat: Seat::P1,
                turn: 0,
            })
        );
        assert_eq!(
            Until::parse("first priority decision for seat 0 from turn 9"),
            Ok(Until::First {
                kind: Kind::Priority,
                seat: Seat::P0,
                turn: 9,
            })
        );
        assert_eq!(
            Until::parse("first halt for seat 1 from turn 4"),
            Ok(Until::Halt {
                seat: Seat::P1,
                turn: 4,
            })
        );
        assert!(Until::parse("game over from turn 3").is_err());
        assert_eq!(Until::parse("game over"), Ok(Until::GameOver));
        assert!(Until::parse("first scrying decision for seat 0").is_err());
        assert!(Until::parse("first target decision for seat 2").is_err());
    }

    #[test]
    fn conditions_parse() {
        assert_eq!(
            Condition::parse("step declareBlockers"),
            Ok(Condition::Step(Step::DeclareBlockers))
        );
        assert_eq!(
            Condition::parse("castable Mana Drain"),
            Ok(Condition::Castable("Mana Drain".to_owned()))
        );
        assert_eq!(Condition::parse("blocking"), Ok(Condition::Blocking));
        assert_eq!(
            Condition::parse("playable Sink into Stupor"),
            Ok(Condition::Playable("Sink into Stupor".to_owned()))
        );
        assert_eq!(
            Condition::parse("abilities of The Wandering Emperor"),
            Ok(Condition::Abilities("The Wandering Emperor".to_owned()))
        );
        assert_eq!(
            Condition::parse("namesake candidates"),
            Ok(Condition::NamesakeCandidates)
        );
        assert_eq!(
            Condition::parse("resolving Mana Tithe"),
            Ok(Condition::Resolving("Mana Tithe".to_owned()))
        );
        assert_eq!(
            Condition::parse("casting Thieving Skydiver"),
            Ok(Condition::Casting("Thieving Skydiver".to_owned()))
        );
        assert_eq!(
            Condition::parse("playable pair in hand"),
            Ok(Condition::PlayablePair)
        );
        assert_eq!(
            Condition::parse("exiled beneath a permanent"),
            Ok(Condition::ExiledBeneath)
        );
        assert_eq!(
            Condition::parse("exiled with a permanent"),
            Ok(Condition::ExiledWith)
        );
        assert_eq!(
            Condition::parse("attached across the table"),
            Ok(Condition::AttachedAcross)
        );
        assert_eq!(
            Condition::parse("damage beside a twin"),
            Ok(Condition::Marked(Mark::Damage))
        );
        assert_eq!(
            Condition::parse("castable from exile"),
            Ok(Condition::CastableFrom(ZoneName::Exile))
        );
        assert_eq!(
            Condition::parse("known card in the opponent's hand"),
            Ok(Condition::KnownOpponentsHand)
        );
        assert_eq!(
            Condition::parse("ability targeting beside its source"),
            Ok(Condition::NeighbourTarget)
        );
        assert_eq!(Condition::parse("token twins"), Ok(Condition::TokenTwins));
        assert_eq!(Condition::parse("emblems a side"), Ok(Condition::Emblems));
        assert_eq!(
            Condition::parse("changed token"),
            Ok(Condition::ChangedToken)
        );
        assert_eq!(
            Condition::parse("token beside its card"),
            Ok(Condition::TokenBesideCard)
        );
        assert_eq!(Condition::parse("stolen permanent"), Ok(Condition::Stolen));
        assert_eq!(
            Condition::parse("mana floating"),
            Ok(Condition::ManaFloating)
        );
        assert_eq!(Condition::parse("X on the stack"), Ok(Condition::XOnStack));
        assert_eq!(
            Condition::parse("8 nonland permanents a side"),
            Ok(Condition::Crowded(8))
        );
        assert_eq!(
            Condition::parse("spell on the stack"),
            Ok(Condition::SpellOnStack)
        );
        assert!(Condition::parse("many nonland permanents a side").is_err());
        assert!(Condition::parse("step lunch").is_err());
        assert!(Condition::parse("raining").is_err());
    }

    /// Every board fixture the UI records still plays out to its stop, in a fraction of a
    /// second, so a change that breaks one fails here rather than in the UI's checks.
    #[test]
    fn every_board_fixture_records() {
        // Read at run time: a test binary from the shared target directory may have been
        // built in a worktree that no longer exists.
        let path = Path::new(
            &std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("../../../ui/fixtures.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let defs: Definitions = serde_json::from_str(&text).unwrap();
        let boards: Vec<&Definition> = defs.fixtures.iter().filter(|d| d.board.is_some()).collect();
        assert!(!boards.is_empty());
        for def in boards {
            record(def).unwrap_or_else(|e| panic!("fixture {}: {e}", def.name));
        }
    }

    #[test]
    fn a_recording_stops_at_its_condition() {
        let def = Definition {
            name: "t".to_owned(),
            seed: Some(4),
            decks: Some(["fuzz:1".to_owned(), "fuzz:2".to_owned()]),
            board: None,
            agents: Some(["random".to_owned(), "random".to_owned()]),
            stops: None,
            until: "first target decision for seat 1".to_owned(),
            conditions: vec![],
            after: 0,
        };
        let messages = record(&def).unwrap();
        let Some(FixtureMessage::Event { payload, .. }) = messages.last() else {
            panic!("a recording ends on a view");
        };
        let update: GameUpdate = serde_json::from_value(payload.clone()).unwrap();
        let Status::Deciding { decision } = update.view.status else {
            panic!("a recording stops at a decision");
        };
        assert_eq!(decision.kind, Kind::Target);
    }
}
