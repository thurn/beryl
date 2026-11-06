// Generated from rust/crates/beryl_session by ts-rs; do not edit. Regenerate with
// `BERYL_UPDATE_TS=1 just test-rust -p beryl_session --test typescript`.

export type AppInfo = { version: string, 
/**
 * Running under `just smoke`.
 */
smoke: boolean, };

export type Fixture = { messages: Array<FixtureMessage>, };

export type FixtureMessage = { "kind": "response", command: keyof Commands, value: unknown, } | { "kind": "event", event: keyof Events, payload: unknown, };

export type StartRequest = { agent: string, 
/**
 * The game's seed; none picks one from the clock.
 */
seed: number | null, };

export type ChooseRequest = { 
/**
 * The decision the UI received.
 */
decision: DecisionId, index: CandidateIndex, };

export type SetStopsRequest = { session: SessionId, stops: StopsSetting, };

export type CancelRequest = { 
/**
 * The open decision the UI received, part of the cast.
 */
decision: DecisionId, };

export type ConcedeRequest = { session: SessionId, };

export type SessionError = { "kind": "unknownAgent", name: string, } | { "kind": "unknownCard", name: string, } | { "kind": "deck", reason: string, } | { "kind": "staleSession", session: SessionId, } | { "kind": "notDeciding" } | { "kind": "staleDecision", decision: DecisionId, open: DecisionId, } | { "kind": "noSuchCandidate", index: CandidateIndex, candidates: number, } | { "kind": "notCancellable" } | { "kind": "ended" } | { "kind": "badReplay", reason: string, } | { "kind": "io", reason: string, };

export type GameUpdate = { 
/**
 * Session numbers only grow, so the UI can drop a view from a replaced game.
 */
session: SessionId, view: PlayerView, };

export type AgentView = { name: string, 
/**
 * What a human calls it.
 */
title: string, description: string, };

export type SessionId = number;

export type DecisionId = { session: SessionId, 
/**
 * Numbers the session's decisions of the human's from 1.
 */
seq: number, };

export type CandidateIndex = number;

export type PlayerView = { status: Status, turn: TurnView, players: Players, 
/**
 * Permanents, in the order they entered.
 */
battlefield: Array<number>, 
/**
 * Every object in a zone, by slot: the battlefield, the stack, and each player's library,
 * hand, graveyard and exile.
 */
objects: Array<ObjectView>, 
/**
 * Bottom first.
 */
stack: Array<StackItem>, 
/**
 * The viewer's priority stops.
 */
stops: StopsSetting, 
/**
 * What happened since the previous view, oldest first, for animation.
 */
events: Array<GameEvent>, };

export type Status = { "kind": "deciding", decision: DecisionView, } | { "kind": "thinking" } | { "kind": "playing" } | { "kind": "stopped" } | { "kind": "over", outcome: OutcomeView, };

export type Side = "you" | "opponent";

export type TurnView = { 
/**
 * 0 before the first turn.
 */
number: number, active: Side, step: Step, priority: Side, 
/**
 * Who takes the first turn (CR 103.1); none while the coin flip's winner chooses.
 */
starting: Side | null, 
/**
 * Who won the coin flip that chose who chooses, in a game begun by one: the session's to
 * say ([`crate::Game`]), as the position no longer holds it.
 */
flipWinner: Side | null, };

export type Players = { you: PlayerState, opponent: PlayerState, };

export type PlayerState = { life: number, 
/**
 * Unspent mana.
 */
mana: ManaPool, landsPlayed: number, mulligans: number, 
/**
 * Where they are in the mulligan (CR 103.5): kept, once the game begins.
 */
mulligan: Declaration, 
/**
 * Bottom first.
 */
library: Array<number>, 
/**
 * Bottom first.
 */
graveyard: Array<number>, 
/**
 * The emblems' names.
 */
emblems: Array<string>, };

export type Declaration = "undeclared" | "willMulligan" | "kept";

export type ManaPool = { white: number, blue: number, black: number, red: number, green: number, colorless: number, };

export type ObjectView = { slot: number, zone: ZoneName, owner: Side, controller: Side, 
/**
 * The name of the face up, which keys the card data in `data/cards.json`; none for a card
 * the viewer has not seen. A face-down permanent's controller may look at it (CR 708.5):
 * its card's name, for them.
 */
name: string | null, 
/**
 * Whether a permanent is face up or face down, part of its status (CR 110.5). A face-down
 * one is a 2/2 creature with no name or text, whatever its card (CR 708.2a).
 */
facing: Facing, token: boolean, tapped: boolean, 
/**
 * Can't attack or use its tap abilities yet (CR 302.6), if it is a creature.
 */
summoningSick: boolean, damage: number, counters: Array<CounterView>, 
/**
 * On the battlefield: the card types after continuous effects.
 */
types: Array<CardType>, 
/**
 * On the battlefield, for creatures: power and toughness after continuous effects.
 */
pt: PowerToughness | null, attachedTo: number | null, 
/**
 * The permanent that exiled it until it leaves (CR 610.3).
 */
exiledBy: number | null, combat: Combat | null, };

export type Facing = "up" | "down";

export type PowerToughness = { power: number, toughness: number, };

export type CounterView = { kind: Counter, count: number, };

export type Combat = { "kind": "attacking", planeswalker: number | null, blocked: boolean, } | { "kind": "blocking", attacker: number | null, };

export type StackItem = { "kind": "spell", slot: number, targets: Array<TargetRef>, 
/**
 * The announced value of X.
 */
x: number, 
/**
 * The alternative and additional costs it was cast with, in its card's order: what
 * they change, as an overload cost turns each "target" into "each", its printed text
 * does not say.
 */
paid: Array<PaidCost>, } | { "kind": "ability", source: number | null, ability: number, granted: boolean, name: string, 
/**
 * A triggered ability rather than an activated one.
 */
triggered: boolean, 
/**
 * How many abilities of its kind (triggered, or activated) come before it in its
 * list: the face's text says them in that order, so the UI finds its paragraph.
 */
nth: number, controller: Side, targets: Array<TargetRef>, 
/**
 * The announced value of X.
 */
x: number, };

export type PaidCost = "kicker" | "offspring" | "impending" | "overload" | "evoke" | "flashback";

export type TargetRef = { "kind": "player", player: Side, } | { "kind": "object", slot: number, };

export type DecisionView = { 
/**
 * What an answer names.
 */
id: DecisionId, kind: Kind, candidates: Array<CandidateView>, 
/**
 * The spell or ability whose resolution asks this, if one does. It has left the stack
 * list by then, as the first step of resolving it
 * ([CR 608.2](https://yawgatog.com/resources/magic-rules/#R6082)): "If the object that's
 * resolving is an instant spell, a sorcery spell, or an ability, its resolution may involve
 * several steps." A resolving spell's card is still among the objects, on the stack.
 */
resolving: StackItem | null, 
/**
 * What the decision asks, where its kind alone does not say.
 */
ask: AskView, 
/**
 * For a priority decision, where passing takes the viewer, for the pass button.
 */
passTo: PassTo | null, 
/**
 * The card the decision is for: the spell or ability resolving, or else the one on top of
 * the stack (which a target, an X or a payment is chosen for); the attacker whose combat
 * damage is divided; the land asking for life. None for priority and the other combat
 * declarations.
 */
source: number | null, 
/**
 * The session halts the viewer here: a priority where passing is all they can do,
 * which the engine would pass for them, offered under the smart stops at the start of
 * their own precombat main phase (an end-turn button), so their turn never ends unseen.
 */
halt: boolean, 
/**
 * The decision is part of a spell the viewer is casting or an ability they are
 * activating from their priority, whose costs are not yet paid: the `cancel` command
 * takes it back.
 */
cancellable: boolean, 
/**
 * At a priority, each card in the viewer's hand they may neither cast nor play as a land,
 * and why: what the UI says of one a click finds nothing to do with.
 */
refused: Array<Refused>, };

export type AskView = { "kind": "plain" } | { "kind": "scry", count: number, } | { "kind": "look", player: Side, zone: ZoneName, count: number, to: ZoneName, eligible: Array<number>, } | { "kind": "putBack", count: number, } | { "kind": "fateseal", player: Side, } | { "kind": "take", count: number, take: number, } | { "kind": "bottom", count: number, } | { "kind": "discardBasicLand", count: number, } | { "kind": "topOrBottom", card: number, } | { "kind": "explore" } | { "kind": "manifestDread" } | { "kind": "exileCards", zone: ZoneName, count: number, } | { "kind": "search", max: number, } | { "kind": "untap", max: number, } | { "kind": "crew", crew: number, left: number, } | { "kind": "waterbend", cost: string, } | { "kind": "unlessPays", cost: string, } | { "kind": "payLife", life: number, } | { "kind": "cleanup", count: number, };

export type PassTo = { "kind": "resolve" } | { "kind": "combatDamage" } | { "kind": "step", step: Step, } | { "kind": "nextTurn" };

export type Refused = { slot: number, reason: RefusalView, };

export type RefusalView = { "kind": "timing", by: number | null, } | { "kind": "landPlayed" } | { "kind": "mana" } | { "kind": "cost" } | { "kind": "target" };

export type CandidateView = { 
/**
 * A short English description, such as "Cast Mana Drain".
 */
label: string, option: Candidate, 
/**
 * For a candidate that casts a spell or activates an ability, or answers its X or its
 * payment plan: each way the cast can then be paid, with what it taps. None otherwise.
 */
payments: Array<Payment>, 
/**
 * For a candidate that declares a blocker: the attackers it may block, by slot, one of
 * each set of interchangeable attackers (architecture §7.2). None otherwise.
 */
blocks: Array<number>, 
/**
 * For a candidate that activates an ability of the object's own face: how many activated
 * abilities, mana abilities among them, its face lists before it, as
 * [`StackItem::Ability`]'s `nth` counts them, so the UI finds the ability's paragraph.
 */
nth: number | null, 
/**
 * For a candidate that announces one or more targets: what the first of them may be, one
 * of each set of interchangeable objects (§7.2), so the UI can take the count and the
 * target as one pick. None otherwise.
 */
targets: Array<TargetRef>, };

export type Payment = { x: number | null, 
/**
 * The index of the payment plan among those the cast offers (they differ in what they
 * consume or leave floating, architecture §10).
 */
plan: number | null, 
/**
 * Permanents, by slot.
 */
taps: Array<number>, };

export type Candidate = { "kind": "object", slot: number, } | { "kind": "pass" } | { "kind": "playLand", slot: number, } | { "kind": "play" } | { "kind": "draw" } | { "kind": "keep" } | { "kind": "mulligan" } | { "kind": "trigger", index: number, source: number | null, } | { "kind": "yes" } | { "kind": "no" } | { "kind": "done" } | { "kind": "attack", slot: number, } | { "kind": "defender", planeswalker: number | null, } | { "kind": "block", slot: number, } | { "kind": "lethal", slot: number, } | { "kind": "cast", slot: number, face: number, costs: Array<number>, } | { "kind": "activate", slot: number, ability: number, granted: boolean, } | { "kind": "mana", slot: number, color: Mana, } | { "kind": "pool", color: Mana, } | { "kind": "number", n: number, } | { "kind": "plan", index: number, } | { "kind": "player", player: Side, } | { "kind": "turnFaceUp", slot: number, };

export type OutcomeView = { "kind": "win", winner: Side, 
/**
 * Why the other player lost.
 */
reason: LossReason, } | { "kind": "draw" } | { "kind": "stalled", reason: Stall, };

export type Stall = "decisionsPerTurn" | "turns" | "stateBasedIterations" | "capacity";

export type LossReason = "life" | "library" | "effect" | "concede";

export type StopsSetting = "smart" | "exact";

export type GameEvent = { "kind": "zoneChange", slot: number, from: ZoneName, to: ZoneName, reason: Why, } | { "kind": "damage", source: number, to: TargetRef, amount: number, combat: boolean, } | { "kind": "draw", player: Side, cards: number, } | { "kind": "lifeGain", player: Side, amount: number, } | { "kind": "lifeLoss", player: Side, amount: number, } | { "kind": "countersAdded", slot: number, counter: Counter, amount: number, } | { "kind": "countersRemoved", slot: number, counter: Counter, amount: number, } | { "kind": "tap", slot: number, } | { "kind": "untap", slot: number, } | { "kind": "tokensCreated", controller: Side, tokens: Array<number>, } | { "kind": "cast", slot: number, controller: Side, } | { "kind": "attack", player: Side, attackers: Array<number>, } | { "kind": "stepBegins", step: Step, active: Side, } | { "kind": "gainControl", slot: number, player: Side, } | { "kind": "coinFlip", player: Side, won: boolean, 
/**
 * The permanent whose ability flipped it.
 */
source: number, } | { "kind": "lose", player: Side, } | { "kind": "win", player: Side, };

export type Kind = "priority" | "playOrDraw" | "keepOrMulligan" | "putOnBottom" | "discard" | "legendRule" | "triggerOrder" | "payLife" | "attacker" | "attackTarget" | "blocker" | "blockTarget" | "damageDivision" | "x" | "targetCount" | "target" | "paymentPlan" | "manaPayment" | "castOffer" | "yesNo" | "chooseObject" | "crew" | "chooseUpTo" | "waterbend" | "exileCost";

export type Step = "untap" | "upkeep" | "draw" | "precombatMain" | "beginningOfCombat" | "declareAttackers" | "declareBlockers" | "firstStrikeDamage" | "combatDamage" | "endOfCombat" | "postcombatMain" | "end" | "cleanup";

export type ZoneName = "library" | "hand" | "battlefield" | "graveyard" | "exile" | "stack" | "nowhere";

export type Why = "play" | "draw" | "discard" | "destroy" | "sacrifice" | "stateBased" | "exile" | "created" | "return" | "effect" | "cast" | "counter" | "resolve";

export type Counter = "plusOne" | "minusOne" | "loyalty" | "time" | "story" | "chorus";

export type Mana = "white" | "blue" | "black" | "red" | "green" | "colorless";

export type CardType = "artifact" | "creature" | "enchantment" | "instant" | "land" | "planeswalker" | "sorcery";

/** Each command: its arguments, its result, and the error it rejects with. */
export type Commands = {
  app_info: { args: Record<string, never>; result: AppInfo; error: never };
  ui_ready: { args: Record<string, never>; result: null; error: never };
  list_agents: { args: Record<string, never>; result: Array<AgentView>; error: never };
  start_game: { args: { request: StartRequest }; result: SessionId; error: SessionError };
  choose: { args: { request: ChooseRequest }; result: null; error: SessionError };
  set_stops: { args: { request: SetStopsRequest }; result: null; error: SessionError };
  cancel: { args: { request: CancelRequest }; result: null; error: SessionError };
  concede: { args: { request: ConcedeRequest }; result: null; error: SessionError };
  current_game: { args: Record<string, never>; result: GameUpdate | null; error: never };
};

/** Each event's payload. */
export type Events = {
  "game-update": GameUpdate;
};
