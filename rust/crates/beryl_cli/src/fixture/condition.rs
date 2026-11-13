//! The conditions on the view a recording stops at.

use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{
    Candidate, CardType, Combat, Counter, Kind, ObjectView, Side, StackItem, Step, TargetRef,
    ZoneName,
};
use beryl_session::{PlayerView, Status};
use serde_json::json;

use super::number;
use super::view_predicates::{
    abilities_of, attached_across, candidates, castable_from, changed_token, crowded,
    emblems_a_side, exiled_by_permanent, knows_opponents_hand, mana_floating, marked_beside_twin,
    mine, namesake_candidates, plain_twins, playable_pair, stolen, targets_beside_source, theirs,
    token_beside_card, token_pairs, ways_to_play, x_on_stack,
};

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
    /// The human's open decision is a pick of a prompt of this purpose and, if it says, this
    /// part of it ([`beryl_session::prompt::PromptView`]'s, as on the wire: "arrange first").
    Prompt(String),
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
    /// A card lies in exile "exiled with" a permanent whose abilities may find it (CR 607.2a).
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
    pub(super) const ALL: [Mark; 4] = [Mark::PlusOne, Mark::Damage, Mark::Sick, Mark::StoryOrTime];

    /// The kinds of counter this mark is.
    pub(super) fn kinds(self) -> &'static [Counter] {
        match self {
            Mark::PlusOne => &[Counter::PlusOne],
            Mark::StoryOrTime => &[Counter::Story, Counter::Time],
            Mark::Damage | Mark::Sick => &[],
        }
    }

    /// Whether `o` bears exactly the mark `only`, or none: no other mark, and no counter but
    /// loyalty (which its plaque shows) and the mark's own.
    pub(super) fn only(only: Option<Mark>, o: &ObjectView) -> bool {
        Mark::ALL.iter().all(|&m| m.on(o) == (Some(m) == only))
            && o.counters.iter().all(|c| {
                c.kind == Counter::Loyalty || only.is_some_and(|m| m.kinds().contains(&c.kind))
            })
    }

    pub(super) fn on(self, o: &ObjectView) -> bool {
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
    /// the stack", "castable <card name>", "playable <card name>", "ways to play <card name>",
    /// "abilities of <card name>", "namesake candidates", "resolving <card name>", "stack <card name>", "casting <card name>",
    /// "playable pair in hand", "exiled beneath a permanent", "exiled with a permanent",
    /// "attached across the table", "castable from exile", "castable from the graveyard",
    /// "known card in the opponent's hand", "token twins", "changed token", "token beside its
    /// card", "stolen permanent", "mana floating", "X on the stack", "spell on the stack",
    /// "emblems a side", "game over", "<card> has <n> <counter kind>" (as the protocol names
    /// counters: "loyalty"), "<n> nonland permanents a side", or "<mark> beside a twin" with the
    /// mark "+1/+1 counters", "damage", "summoning sickness" or "story or time counters".
    /// Specs add "turn <n>", "your turn", "their turn", "decision <kind>", "prompt <purpose> [<part>]",
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
                } else if let Some(prompt) = text.strip_prefix("prompt ") {
                    Condition::Prompt(prompt.to_owned())
                } else if let Some(card) = text.strip_prefix("tapped ") {
                    Condition::Tapped(card.to_owned())
                } else if let Some(step) = text.strip_prefix("step ") {
                    Condition::Step(
                        serde_json::from_value(json!(step))
                            .map_err(|_| format!("no step is named {step:?}"))?,
                    )
                } else if let Some(name) = text.strip_prefix("castable ") {
                    Condition::Castable(name.to_owned())
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
        if let Some(card) = text.strip_prefix("ways to play ") {
            return Some(Ok(Condition::Ways(card.to_owned())));
        }
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
        let name = |slot: ObjectId| {
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
            Condition::Ways(card) => ways_to_play(view, card),
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
        let name = |slot: ObjectId| {
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
            Condition::Prompt(asked) => decision.is_some_and(|d| {
                let (purpose, part) = asked.split_once(' ').unwrap_or((asked, ""));
                d.prompt.purpose.name() == purpose
                    && (part.is_empty() || json!(d.prompt.part) == part)
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
