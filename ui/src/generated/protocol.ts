// Generated from rust/crates/beryl_session by ts-rs; do not edit. Regenerate with
// `BERYL_UPDATE_TS=1 just test-rust -p beryl_session --test typescript`.

export type GameUpdate = { session: number, view: PlayerView, };

export type AgentView = { name: string, description: string, };

export type PlayerView = { 
/**
 * Numbers the viewer's decisions; `choose` names the one it answers.
 */
seq: number, viewer: number, turn: TurnView, 
/**
 * By seat.
 */
players: Array<PlayerState>, 
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
 * The viewer's open decision; none once the game is over.
 */
decision: DecisionView | null, outcome: OutcomeView | null, 
/**
 * The viewer's priority stops.
 */
stops: StopsSetting, 
/**
 * What happened since the previous view, oldest first, for animation.
 */
events: Array<GameEvent>, };

export type TurnView = { 
/**
 * 0 before the first turn.
 */
number: number, active: number, step: Step, priority: number, };

export type PlayerState = { life: number, 
/**
 * Unspent mana: white, blue, black, red, green, colorless.
 */
mana: Array<number>, landsPlayed: number, mulligans: number, 
/**
 * Bottom first.
 */
library: Array<number>, hand: Array<number>, 
/**
 * Bottom first.
 */
graveyard: Array<number>, exile: Array<number>, 
/**
 * The emblems' names.
 */
emblems: Array<string>, };

export type ObjectView = { slot: number, zone: ZoneName, owner: number, controller: number, 
/**
 * The name of the face up, which keys the card data in `data/cards.json`; none for a card
 * the viewer has not seen.
 */
name: string | null, token: boolean, tapped: boolean, 
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
power: number | null, toughness: number | null, attachedTo: number | null, 
/**
 * The permanent that exiled it until it leaves (CR 610.3).
 */
exiledBy: number | null, combat: Combat | null, };

export type CounterView = { kind: Counter, count: number, };

export type Combat = { "kind": "attacking", planeswalker: number | null, blocked: boolean, } | { "kind": "blocking", attacker: number | null, };

export type StackItem = { 
/**
 * The spell, or the ability's source (which may have left its zone since).
 */
slot: number, controller: number, 
/**
 * The spell's face name, or the name of the card with the ability.
 */
name: string, 
/**
 * The index of the ability on that face; none for a spell.
 */
ability: number | null, targets: Array<TargetRef>, 
/**
 * The announced value of X.
 */
x: number, };

export type TargetRef = { "kind": "player", player: number, } | { "kind": "object", slot: number, };

export type DecisionView = { player: number, kind: Kind, candidates: Array<CandidateView>, };

export type CandidateView = { 
/**
 * A short English description, such as "Cast Mana Drain".
 */
label: string, option: Candidate, };

export type Candidate = { "kind": "object", slot: number, } | { "kind": "pass" } | { "kind": "playLand", slot: number, } | { "kind": "play" } | { "kind": "draw" } | { "kind": "keep" } | { "kind": "mulligan" } | { "kind": "trigger", index: number, source: number | null, } | { "kind": "yes" } | { "kind": "no" } | { "kind": "done" } | { "kind": "attack", slot: number, } | { "kind": "defender", planeswalker: number | null, } | { "kind": "block", slot: number, } | { "kind": "lethal", slot: number, } | { "kind": "cast", slot: number, face: number, costs: Array<number>, } | { "kind": "activate", slot: number, ability: number, } | { "kind": "mana", slot: number, color: Mana, } | { "kind": "pool", color: Mana, } | { "kind": "number", n: number, } | { "kind": "plan", index: number, } | { "kind": "player", player: number, };

export type OutcomeView = { "kind": "win", player: number, } | { "kind": "draw" } | { "kind": "stalled", reason: string, };

export type StopsSetting = "smart" | "exact";

export type GameEvent = { "kind": "zoneChange", slot: number, from: ZoneName, to: ZoneName, reason: Why, } | { "kind": "damage", source: number, to: TargetRef, amount: number, combat: boolean, } | { "kind": "draw", player: number, cards: number, } | { "kind": "lifeGain", player: number, amount: number, } | { "kind": "lifeLoss", player: number, amount: number, } | { "kind": "countersAdded", slot: number, counter: Counter, amount: number, } | { "kind": "countersRemoved", slot: number, counter: Counter, amount: number, } | { "kind": "tap", slot: number, } | { "kind": "untap", slot: number, } | { "kind": "tokensCreated", controller: number, tokens: Array<number>, } | { "kind": "cast", slot: number, controller: number, } | { "kind": "attack", player: number, attackers: Array<number>, } | { "kind": "stepBegins", step: Step, active: number, } | { "kind": "gainControl", slot: number, player: number, } | { "kind": "coinFlip", player: number, won: boolean, } | { "kind": "lose", player: number, } | { "kind": "win", player: number, };

export type Kind = "priority" | "playOrDraw" | "keepOrMulligan" | "putOnBottom" | "discard" | "legendRule" | "triggerOrder" | "payLife" | "attacker" | "attackTarget" | "blocker" | "blockTarget" | "damageDivision" | "x" | "targetCount" | "target" | "paymentPlan" | "manaPayment" | "castOffer" | "yesNo" | "chooseObject" | "crew" | "chooseUpTo" | "waterbend";

export type Step = "untap" | "upkeep" | "draw" | "precombatMain" | "beginningOfCombat" | "declareAttackers" | "declareBlockers" | "firstStrikeDamage" | "combatDamage" | "endOfCombat" | "postcombatMain" | "end" | "cleanup";

export type ZoneName = "library" | "hand" | "battlefield" | "graveyard" | "exile" | "stack" | "nowhere";

export type Why = "play" | "draw" | "discard" | "destroy" | "sacrifice" | "stateBased" | "exile" | "created" | "return" | "effect" | "cast" | "counter" | "resolve";

export type Counter = "plusOne" | "minusOne" | "loyalty" | "time" | "story" | "chorus";

export type Mana = "white" | "blue" | "black" | "red" | "green" | "colorless";

export type CardType = "artifact" | "creature" | "enchantment" | "instant" | "land" | "planeswalker" | "sorcery";
