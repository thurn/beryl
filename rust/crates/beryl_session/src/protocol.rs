//! The `PlayerView` protocol (architecture §19.2, decision D14): what the UI draws, built from
//! one player's [`Observation`] so it carries nothing that player does not know. Every type
//! here has a generated TypeScript twin in `ui/src/generated/protocol.ts`
//! ([`crate::typescript`]).

use beryl_engine::card::{ManaColor, Types};
use beryl_engine::event::{Event, Reason, Recipient};
use beryl_engine::state::{CounterKind, NONE, Object, ObjectFlags, StackEntry, TurnStep, opponent};
use beryl_engine::{
    Candidates, Decision, DecisionKind, GameTable, Observation, Opt, Outcome, StallReason, Stops,
    UNKNOWN, Zone,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Everything the UI draws for one player at one moment: the game as they see it, where it
/// stands (their open decision, a stop or the end), and what happened since the previous view.
/// Players are named relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub status: Status,
    pub turn: TurnView,
    pub players: Players,
    /// Permanents, in the order they entered.
    pub battlefield: Vec<u8>,
    /// Every object in a zone, by slot: the battlefield, the stack, and each player's library,
    /// hand, graveyard and exile.
    pub objects: Vec<ObjectView>,
    /// Bottom first.
    pub stack: Vec<StackItem>,
    /// The viewer's priority stops.
    pub stops: StopsSetting,
    /// What happened since the previous view, oldest first, for animation.
    pub events: Vec<GameEvent>,
}

/// Where the game stands for the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Status {
    /// The viewer must decide; the game waits for their answer.
    Deciding {
        decision: DecisionView,
    },
    /// The session stopped while the agent was deciding (it was replaced or closed): no
    /// decision opens again.
    Stopped,
    Over {
        outcome: OutcomeView,
    },
}

/// A player, relative to the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    You,
    Opponent,
}

/// A game session, numbered from 1 in the order sessions start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
pub struct SessionId(pub u32);

/// Names one decision of the human's in one session. The UI echoes the one it received to
/// answer it, and never looks inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DecisionId {
    pub session: SessionId,
    /// Numbers the session's decisions of the human's from 1.
    pub seq: u32,
}

/// A candidate of a decision, by its position in [`DecisionView::candidates`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CandidateIndex(pub u8);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnView {
    /// 0 before the first turn.
    pub number: u16,
    pub active: Side,
    pub step: Step,
    pub priority: Side,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Players {
    pub you: PlayerState,
    pub opponent: PlayerState,
}

/// One player's public state and their ordered zones. Their hand and exile are the objects
/// they own in those zones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub life: i16,
    /// Unspent mana.
    pub mana: ManaPool,
    pub lands_played: u8,
    pub mulligans: u8,
    /// Bottom first.
    pub library: Vec<u8>,
    /// Bottom first.
    pub graveyard: Vec<u8>,
    /// The emblems' names.
    pub emblems: Vec<String>,
}

/// Mana by type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManaPool {
    pub white: u8,
    pub blue: u8,
    pub black: u8,
    pub red: u8,
    pub green: u8,
    pub colorless: u8,
}

/// One object, with its identity only if the viewer knows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ObjectView {
    pub slot: u8,
    pub zone: ZoneName,
    pub owner: Side,
    pub controller: Side,
    /// The name of the face up, which keys the card data in `data/cards.json`; none for a card
    /// the viewer has not seen.
    pub name: Option<String>,
    pub token: bool,
    pub tapped: bool,
    /// Can't attack or use its tap abilities yet (CR 302.6), if it is a creature.
    pub summoning_sick: bool,
    pub damage: u8,
    pub counters: Vec<CounterView>,
    /// On the battlefield: the card types after continuous effects.
    pub types: Vec<CardType>,
    /// On the battlefield, for creatures: power and toughness after continuous effects.
    pub pt: Option<PowerToughness>,
    pub attached_to: Option<u8>,
    /// The permanent that exiled it until it leaves (CR 610.3).
    pub exiled_by: Option<u8>,
    pub combat: Option<Combat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PowerToughness {
    pub power: i8,
    pub toughness: i8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CounterView {
    pub kind: Counter,
    pub count: u8,
}

/// A creature's part in combat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Combat {
    /// Attacking a player, or the planeswalker in `planeswalker`.
    #[serde(rename_all = "camelCase")]
    Attacking {
        planeswalker: Option<u8>,
        blocked: bool,
    },
    /// Blocking the attacker in `attacker`, if it is still in combat.
    #[serde(rename_all = "camelCase")]
    Blocking { attacker: Option<u8> },
}

/// A spell or ability on the stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StackItem {
    /// The spell in `slot`, whose object says what it is and who controls it.
    Spell {
        slot: u8,
        targets: Vec<TargetRef>,
        /// The announced value of X.
        x: u8,
    },
    /// The ability at index `ability` of the face named `name`, from the object in `source`
    /// (which may have left its zone since).
    Ability {
        source: u8,
        ability: u8,
        name: String,
        controller: Side,
        targets: Vec<TargetRef>,
        /// The announced value of X.
        x: u8,
    },
}

/// A player or an object, as a target or a recipient of damage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetRef {
    Player { player: Side },
    Object { slot: u8 },
}

/// The viewer's decision, its candidates in the order a [`CandidateIndex`] counts them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DecisionView {
    /// What an answer names.
    pub id: DecisionId,
    pub kind: Kind,
    pub candidates: Vec<CandidateView>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CandidateView {
    /// A short English description, such as "Cast Mana Drain".
    pub label: String,
    pub option: Candidate,
}

/// What a candidate does, by public slot (architecture §7.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Candidate {
    /// The object in this slot, for a decision among objects.
    Object {
        slot: u8,
    },
    Pass,
    PlayLand {
        slot: u8,
    },
    Play,
    Draw,
    Keep,
    Mulligan,
    /// The waiting triggered ability at `index`, from the object in `source`.
    Trigger {
        index: u8,
        source: Option<u8>,
    },
    Yes,
    No,
    Done,
    Attack {
        slot: u8,
    },
    /// Attack the planeswalker in `planeswalker`, or the defending player.
    Defender {
        planeswalker: Option<u8>,
    },
    Block {
        slot: u8,
    },
    Lethal {
        slot: u8,
    },
    /// Cast with face `face` up, paying the face's casting options at the indices in `costs`.
    Cast {
        slot: u8,
        face: u8,
        costs: Vec<u8>,
    },
    Activate {
        slot: u8,
        ability: u8,
    },
    Mana {
        slot: u8,
        color: Mana,
    },
    Pool {
        color: Mana,
    },
    Number {
        n: u8,
    },
    Plan {
        index: u8,
    },
    Player {
        player: Side,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutcomeView {
    Win {
        winner: Side,
    },
    Draw,
    /// The engine gave up when a budget or capacity ran out; not a rules outcome.
    Stalled {
        reason: Stall,
    },
}

/// Where the viewer gets priority (architecture §15, decision D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StopsSetting {
    /// The smart stops: main phases, combat after declarations, the opponent's end step, and
    /// whenever the stack is non-empty.
    Smart,
    /// Every priority with a legal action.
    Exact,
}

impl StopsSetting {
    pub fn stops(self) -> Stops {
        match self {
            StopsSetting::Smart => Stops::Smart,
            StopsSetting::Exact => Stops::Exact,
        }
    }
}

/// One thing that happened (architecture §8.1), by public slot and relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GameEvent {
    /// The object now in `slot` arrived from another zone.
    ZoneChange {
        slot: u8,
        from: ZoneName,
        to: ZoneName,
        reason: Why,
    },
    Damage {
        source: u8,
        to: TargetRef,
        amount: u8,
        combat: bool,
    },
    Draw {
        player: Side,
        cards: u8,
    },
    LifeGain {
        player: Side,
        amount: u8,
    },
    LifeLoss {
        player: Side,
        amount: u8,
    },
    CountersAdded {
        slot: u8,
        counter: Counter,
        amount: u8,
    },
    CountersRemoved {
        slot: u8,
        counter: Counter,
        amount: u8,
    },
    Tap {
        slot: u8,
    },
    Untap {
        slot: u8,
    },
    TokensCreated {
        controller: Side,
        tokens: Vec<u8>,
    },
    Cast {
        slot: u8,
        controller: Side,
    },
    Attack {
        player: Side,
        attackers: Vec<u8>,
    },
    StepBegins {
        step: Step,
        active: Side,
    },
    GainControl {
        slot: u8,
        player: Side,
    },
    CoinFlip {
        player: Side,
        won: bool,
    },
    Lose {
        player: Side,
    },
    Win {
        player: Side,
    },
}

/// The engine's enums, mirrored so the protocol owns its wire names.
macro_rules! mirror {
    ($(#[$meta:meta])* $name:ident from $engine:ty { $($variant:ident),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub enum $name {
            $($variant),*
        }

        impl From<$engine> for $name {
            fn from(v: $engine) -> Self {
                match v {
                    $(<$engine>::$variant => $name::$variant),*
                }
            }
        }
    };
}

mirror! {
    /// What is being decided ([`DecisionKind`]).
    Kind from DecisionKind {
        Priority, PlayOrDraw, KeepOrMulligan, PutOnBottom, Discard, LegendRule, TriggerOrder,
        PayLife, Attacker, AttackTarget, Blocker, BlockTarget, DamageDivision, X, TargetCount,
        Target, PaymentPlan, ManaPayment, CastOffer, YesNo, ChooseObject, Crew, ChooseUpTo, Waterbend,
    }
}

mirror! {
    /// A step of the turn ([`TurnStep`]).
    Step from TurnStep {
        Untap, Upkeep, Draw, PrecombatMain, BeginningOfCombat, DeclareAttackers,
        DeclareBlockers, FirstStrikeDamage, CombatDamage, EndOfCombat, PostcombatMain, End,
        Cleanup,
    }
}

mirror! {
    /// A zone ([`Zone`]).
    ZoneName from Zone { Library, Hand, Battlefield, Graveyard, Exile, Stack, Nowhere }
}

mirror! {
    /// Why an object changed zones ([`Reason`]).
    Why from Reason {
        Play, Draw, Discard, Destroy, Sacrifice, StateBased, Exile, Created, Return, Effect,
        Cast, Counter, Resolve,
    }
}

mirror! {
    /// A kind of counter ([`CounterKind`]).
    Counter from CounterKind { PlusOne, MinusOne, Loyalty, Time, Story, Chorus }
}

mirror! {
    /// Why the engine gave up on a game ([`StallReason`]).
    Stall from StallReason { DecisionsPerTurn, Turns, StateBasedIterations, Capacity }
}

mirror! {
    /// A type of mana ([`ManaColor`]).
    Mana from ManaColor { White, Blue, Black, Red, Green, Colorless }
}

/// A card type (CR 300.1), those the cube's cards use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CardType {
    Artifact,
    Creature,
    Enchantment,
    Instant,
    Land,
    Planeswalker,
    Sorcery,
}

impl CardType {
    const ALL: [(Types, CardType); 7] = [
        (Types::ARTIFACT, CardType::Artifact),
        (Types::CREATURE, CardType::Creature),
        (Types::ENCHANTMENT, CardType::Enchantment),
        (Types::INSTANT, CardType::Instant),
        (Types::LAND, CardType::Land),
        (Types::PLANESWALKER, CardType::Planeswalker),
        (Types::SORCERY, CardType::Sorcery),
    ];
}

/// Where the game stands, as the session knows it: what [`player_view`] turns into a [`Status`].
#[derive(Clone, Copy, Debug)]
pub enum Moment<'a> {
    /// The viewer's decision `id` is open.
    Deciding(DecisionId, &'a Decision),
    Stopped,
    Over(Outcome),
}

/// Builds the viewer's view of `obs`'s position (the position the open decision is made in,
/// `beryl_engine::view`) at `moment`, with `events`, the engine's events since the previous view.
/// Everything in it, events included, comes through `obs`.
pub fn player_view(
    t: &GameTable,
    obs: Observation<'_>,
    moment: Moment<'_>,
    stops: StopsSetting,
    events: &[Event],
) -> PlayerView {
    let turn = obs.turn();
    let mut objects = Vec::new();
    let mut player_state = |p: u8| {
        let player = obs.player(p);
        let [white, blue, black, red, green, colorless] = player.mana;
        let state = PlayerState {
            life: player.life,
            mana: ManaPool {
                white,
                blue,
                black,
                red,
                green,
                colorless,
            },
            lands_played: player.lands_played,
            mulligans: player.mulligans,
            library: obs.library(p).to_vec(),
            graveyard: obs.graveyard(p).to_vec(),
            emblems: player
                .emblems
                .iter()
                .map(|&e| t.def(e).name.to_owned())
                .collect(),
        };
        for slots in [&state.library, &state.graveyard] {
            objects.extend(slots.iter().map(|&slot| object_view(t, obs, slot)));
        }
        // Unordered zones: their objects say where they are.
        for slots in [Zone::Hand, Zone::Exile].map(|zone| obs.zone(p, zone)) {
            objects.extend(slots.iter().map(|slot| object_view(t, obs, slot)));
        }
        state
    };
    let viewer = obs.viewer();
    let players = Players {
        you: player_state(viewer),
        opponent: player_state(opponent(viewer)),
    };
    objects.extend(obs.battlefield().iter().map(|&s| object_view(t, obs, s)));
    let stack = obs.stack().iter().map(|e| stack_item(t, obs, e)).collect();
    objects.extend(
        obs.stack()
            .iter()
            .filter(|e| e.is_spell())
            .map(|e| object_view(t, obs, e.object.slot)),
    );
    objects.sort_by_key(|o| o.slot);
    let status = match moment {
        Moment::Deciding(id, d) => {
            debug_assert_eq!(d.player, viewer, "a view shows only its viewer's decision");
            Status::Deciding {
                decision: decision_view(t, obs, id, d),
            }
        }
        Moment::Stopped => Status::Stopped,
        Moment::Over(outcome) => Status::Over {
            outcome: outcome_view(obs, outcome),
        },
    };
    PlayerView {
        status,
        turn: TurnView {
            number: turn.number,
            active: side(obs, turn.active),
            step: turn.step.into(),
            priority: side(obs, turn.priority),
        },
        players,
        battlefield: obs.battlefield().to_vec(),
        objects,
        stack,
        stops,
        events: events.iter().map(|e| game_event(obs, e)).collect(),
    }
}

/// Player `p` as the viewer of `obs` sees them.
fn side(obs: Observation<'_>, p: u8) -> Side {
    if p == obs.viewer() {
        Side::You
    } else {
        Side::Opponent
    }
}

/// The name of the face up of the object in `slot`, if the viewer knows it: a copy's is the
/// copied card's (CR 707.2).
fn name(t: &GameTable, obs: Observation<'_>, slot: u8) -> Option<&'static str> {
    let o = obs.object(slot);
    (o.card != UNKNOWN).then(|| face_name(t, &o))
}

fn face_name(t: &GameTable, o: &Object) -> &'static str {
    let card = if o.copy_of == NONE { o.card } else { o.copy_of };
    t.def(card).face_name(o.face)
}

fn object_view(t: &GameTable, obs: Observation<'_>, slot: u8) -> ObjectView {
    let o = obs.object(slot);
    let flags = o.flags;
    let on_battlefield = o.zone == Zone::Battlefield;
    let derived = on_battlefield.then(|| obs.derived(slot));
    let types = derived.map_or(Types::NONE, |d| Types(d.types));
    let creature = types.contains(Types::CREATURE);
    let counters = [
        CounterKind::PlusOne,
        CounterKind::MinusOne,
        CounterKind::Loyalty,
        CounterKind::Time,
        CounterKind::Story,
        CounterKind::Chorus,
    ]
    .into_iter()
    .filter_map(|kind| {
        let count = o.counters.get(kind);
        (count > 0).then(|| CounterView {
            kind: kind.into(),
            count,
        })
    })
    .collect();
    let some = |s: u8| (s != NONE).then_some(s);
    ObjectView {
        slot,
        zone: o.zone.into(),
        owner: side(obs, o.owner),
        controller: side(obs, derived.map_or(o.controller, |d| d.controller)),
        name: name(t, obs, slot).map(str::to_owned),
        token: flags.has(ObjectFlags::TOKEN),
        tapped: flags.has(ObjectFlags::TAPPED),
        summoning_sick: creature && flags.has(ObjectFlags::SUMMONING_SICK),
        damage: o.damage,
        counters,
        types: CardType::ALL
            .iter()
            .filter(|&&(bit, _)| types.contains(bit))
            .map(|&(_, ty)| ty)
            .collect(),
        pt: derived.filter(|_| creature).map(|d| PowerToughness {
            power: d.power,
            toughness: d.toughness,
        }),
        attached_to: some(o.attached_to),
        exiled_by: if flags.has(ObjectFlags::EXILED_UNTIL) {
            some(o.linked)
        } else {
            None
        },
        combat: if flags.has(ObjectFlags::ATTACKING) {
            Some(Combat::Attacking {
                planeswalker: if flags.has(ObjectFlags::ATTACKS_PLANESWALKER) {
                    some(o.combat)
                } else {
                    None
                },
                blocked: flags.has(ObjectFlags::BLOCKED),
            })
        } else if flags.has(ObjectFlags::BLOCKING) {
            Some(Combat::Blocking {
                attacker: some(o.combat),
            })
        } else {
            None
        },
    }
}

fn stack_item(t: &GameTable, obs: Observation<'_>, e: &StackEntry) -> StackItem {
    let targets = e
        .targets
        .iter()
        .map(|target| match (target.as_player(), target.as_object()) {
            (Some(player), _) => TargetRef::Player {
                player: side(obs, player),
            },
            (None, Some(o)) => TargetRef::Object { slot: o.slot },
            (None, None) => unreachable!("a target is a player or an object"),
        })
        .collect();
    if e.is_spell() {
        StackItem::Spell {
            slot: e.object.slot,
            targets,
            x: e.x,
        }
    } else {
        StackItem::Ability {
            source: e.object.slot,
            ability: e.ability,
            name: t.def(e.card).face_name(e.face).to_owned(),
            controller: side(obs, e.controller),
            targets,
            x: e.x,
        }
    }
}

fn decision_view(
    t: &GameTable,
    obs: Observation<'_>,
    id: DecisionId,
    d: &Decision,
) -> DecisionView {
    let candidates = match &d.candidates {
        Candidates::Objects(b) => b
            .iter()
            .map(|slot| CandidateView {
                label: name(t, obs, slot).unwrap_or("a hidden card").to_owned(),
                option: Candidate::Object { slot },
            })
            .collect(),
        Candidates::Options(opts) => opts.iter().map(|&o| candidate(t, obs, o)).collect(),
    };
    DecisionView {
        id,
        kind: d.kind.into(),
        candidates,
    }
}

fn candidate(t: &GameTable, obs: Observation<'_>, o: Opt) -> CandidateView {
    let named = |slot: u8| name(t, obs, slot).unwrap_or("a hidden card");
    let player = |p: u8| match side(obs, p) {
        Side::You => "you",
        Side::Opponent => "your opponent",
    };
    let mana = |color: u8| (mana_color(color), MANA_SYMBOLS[usize::from(color)]);
    let (label, option) = match o {
        Opt::Pass => ("Pass".to_owned(), Candidate::Pass),
        Opt::PlayLand(slot) => (
            format!("Play {}", named(slot)),
            Candidate::PlayLand { slot },
        ),
        Opt::Play => ("Play first".to_owned(), Candidate::Play),
        Opt::Draw => ("Draw first".to_owned(), Candidate::Draw),
        Opt::Keep => ("Keep".to_owned(), Candidate::Keep),
        Opt::Mulligan => ("Mulligan".to_owned(), Candidate::Mulligan),
        Opt::Trigger(index) => trigger(t, obs, index),
        Opt::Yes => ("Yes".to_owned(), Candidate::Yes),
        Opt::No => ("No".to_owned(), Candidate::No),
        Opt::Done => ("Done".to_owned(), Candidate::Done),
        Opt::Attack(slot) => (
            format!("Attack with {}", named(slot)),
            Candidate::Attack { slot },
        ),
        Opt::Defender(slot) if slot == NONE => (
            format!("Attack {}", player(opponent(obs.turn().active))),
            Candidate::Defender { planeswalker: None },
        ),
        Opt::Defender(slot) => (
            format!("Attack {}", named(slot)),
            Candidate::Defender {
                planeswalker: Some(slot),
            },
        ),
        Opt::Block(slot) => (
            format!("Block with {}", named(slot)),
            Candidate::Block { slot },
        ),
        Opt::Lethal(slot) => (
            format!("Assign lethal damage to {}", named(slot)),
            Candidate::Lethal { slot },
        ),
        Opt::Cast { slot, face, costs } => {
            let o = Object {
                face,
                ..obs.object(slot)
            };
            let spell = if o.card == UNKNOWN {
                "a hidden card"
            } else {
                face_name(t, &o)
            };
            (
                format!("Cast {spell}"),
                Candidate::Cast {
                    slot,
                    face,
                    costs: (0..8).filter(|i| costs & (1 << i) != 0).collect(),
                },
            )
        }
        Opt::Activate { slot, ability } => (
            format!("Activate {}", named(slot)),
            Candidate::Activate { slot, ability },
        ),
        Opt::Mana { slot, color } => {
            let (color, symbol) = mana(color);
            (
                format!("Tap {} for {symbol}", named(slot)),
                Candidate::Mana { slot, color },
            )
        }
        Opt::Pool(color) => {
            let (color, symbol) = mana(color);
            (format!("Spend {symbol}"), Candidate::Pool { color })
        }
        Opt::Number(n) => (n.to_string(), Candidate::Number { n }),
        Opt::Plan(index) => (format!("Payment {}", index + 1), Candidate::Plan { index }),
        Opt::Player(p) => (
            match side(obs, p) {
                Side::You => "You",
                Side::Opponent => "Your opponent",
            }
            .to_owned(),
            Candidate::Player {
                player: side(obs, p),
            },
        ),
        Opt::Object(slot) => (named(slot).to_owned(), Candidate::Object { slot }),
    };
    CandidateView { label, option }
}

/// Putting the waiting trigger at `index` on the stack.
fn trigger(t: &GameTable, obs: Observation<'_>, index: u8) -> (String, Candidate) {
    let trigger = obs.triggers()[usize::from(index)];
    let source = trigger.source.slot;
    (
        format!(
            "Put {}'s trigger on the stack",
            t.def(trigger.card).face_name(trigger.face)
        ),
        Candidate::Trigger {
            index,
            source: (source != NONE).then_some(source),
        },
    )
}

/// Mana symbols by [`ManaColor`] index.
const MANA_SYMBOLS: [&str; 6] = ["{W}", "{U}", "{B}", "{R}", "{G}", "{C}"];

fn mana_color(index: u8) -> Mana {
    [
        ManaColor::White,
        ManaColor::Blue,
        ManaColor::Black,
        ManaColor::Red,
        ManaColor::Green,
        ManaColor::Colorless,
    ][usize::from(index)]
    .into()
}

fn outcome_view(obs: Observation<'_>, o: Outcome) -> OutcomeView {
    match o {
        Outcome::Win(player) => OutcomeView::Win {
            winner: side(obs, player),
        },
        Outcome::Draw => OutcomeView::Draw,
        Outcome::Stalled(reason) => OutcomeView::Stalled {
            reason: reason.into(),
        },
    }
}

/// An engine event as the viewer of `obs` is told it. Events name objects by public slot, so
/// none carries an identity; one that did would have to come from `obs`.
fn game_event(obs: Observation<'_>, e: &Event) -> GameEvent {
    let side = |p| side(obs, p);
    match *e {
        Event::ZoneChange {
            object,
            from,
            to,
            reason,
        } => GameEvent::ZoneChange {
            slot: object.slot,
            from: from.into(),
            to: to.into(),
            reason: reason.into(),
        },
        Event::Damage {
            source,
            to,
            amount,
            combat,
        } => GameEvent::Damage {
            source: source.slot,
            to: match to {
                Recipient::Player(player) => TargetRef::Player {
                    player: side(player),
                },
                Recipient::Permanent(o) => TargetRef::Object { slot: o.slot },
            },
            amount,
            combat,
        },
        Event::Draw { player, cards } => GameEvent::Draw {
            player: side(player),
            cards,
        },
        Event::LifeGain { player, amount } => GameEvent::LifeGain {
            player: side(player),
            amount,
        },
        Event::LifeLoss { player, amount } => GameEvent::LifeLoss {
            player: side(player),
            amount,
        },
        Event::CountersAdded {
            object,
            kind,
            amount,
        } => GameEvent::CountersAdded {
            slot: object.slot,
            counter: kind.into(),
            amount,
        },
        Event::CountersRemoved {
            object,
            kind,
            amount,
        } => GameEvent::CountersRemoved {
            slot: object.slot,
            counter: kind.into(),
            amount,
        },
        Event::Tap { object } => GameEvent::Tap { slot: object.slot },
        Event::Untap { object } => GameEvent::Untap { slot: object.slot },
        Event::TokensCreated { controller, tokens } => GameEvent::TokensCreated {
            controller: side(controller),
            tokens: tokens.iter().collect(),
        },
        Event::Cast { spell, controller } => GameEvent::Cast {
            slot: spell.slot,
            controller: side(controller),
        },
        Event::Attack { player, attackers } => GameEvent::Attack {
            player: side(player),
            attackers: attackers.iter().collect(),
        },
        Event::StepBegins { step, active } => GameEvent::StepBegins {
            step: step.into(),
            active: side(active),
        },
        Event::GainControl { object, player } => GameEvent::GainControl {
            slot: object.slot,
            player: side(player),
        },
        Event::CoinFlip { player, won } => GameEvent::CoinFlip {
            player: side(player),
            won,
        },
        Event::Lose { player } => GameEvent::Lose {
            player: side(player),
        },
        Event::Win { player } => GameEvent::Win {
            player: side(player),
        },
    }
}
