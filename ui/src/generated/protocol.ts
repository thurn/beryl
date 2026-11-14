// Generated from rust/crates/beryl_session by ts-rs; do not edit. Regenerate with
// `BERYL_UPDATE_TS=1 just test-rust -p beryl_session --test typescript`.

export type AppInfo = { version: string, 
/**
 * Running under `just smoke`.
 */
smoke: boolean, platform: Platform, 
/**
 * Running in the iOS Simulator or Android Emulator.
 */
simulator: boolean, };

export type Platform = "macos" | "windows" | "linux" | "ios" | "android";

export type Fixture = { messages: Array<FixtureMessage>, };

export type FixtureMessage = { "kind": "response", command: keyof Commands, 
/**
 * The request it answered, where a replay holds the UI to it: a click on a prompt,
 * whose reply is the state after that click and no other.
 */
request?: unknown, value: unknown, } | { "kind": "event", event: keyof Events, payload: unknown, };

export type StartRequest = { agent: string, 
/**
 * The game's seed; none picks one from the clock.
 */
seed: number | null, };

export type ChooseRequest = { 
/**
 * The decision the UI received.
 */
decision: DecisionId, index: CandidateIndex, 
/**
 * Joins a follow-up answer to the preceding choice for Undo.
 */
continuation?: boolean, };

export type SetStopsRequest = { session: SessionId, stops: StopsSetting, };

export type CancelRequest = { 
/**
 * The open decision the UI received, part of the cast.
 */
decision: DecisionId, };

export type UndoRequest = { 
/**
 * The open decision the UI received.
 */
decision: DecisionId, };

export type PassOnRequest = { 
/**
 * The open decision the UI received.
 */
decision: DecisionId, };

export type ConcedeRequest = { session: SessionId, };

export type ShareBugRequest = { id: string, };

export type BugReport = { 
/**
 * What the player says went wrong.
 */
description: string, 
/**
 * The player reports a questionable play of the AI's, for incident analysis: the report
 * carries the agent's latest searches.
 */
ai: boolean, 
/**
 * The app's snapshot of the window as the player froze it (`capture_window`), which the
 * app turns into the report's images ([`Images`]); none in a browser.
 */
snapshot: number | null, 
/**
 * Where the region lies in the window, in CSS pixels.
 */
region: Region | null, page: PageInfo, 
/**
 * The UI's log ([`ui/src/diagnostics.ts`]): commands, events, errors, console.
 */
uiLog: unknown, 
/**
 * The latest game updates the UI received, oldest first.
 */
updates: unknown[], };

export type Region = { x: number, y: number, width: number, height: number, };

export type PageInfo = { url: string, 
/**
 * The window's size in CSS pixels, and device pixels to one.
 */
width: number, height: number, scale: number, };

export type BugSaved = { id: string, 
/**
 * Its folder.
 */
path: string, };

export type CardRecordsRequest = { 
/**
 * Card, face, token and emblem names, as the engine gives them.
 */
names: Array<string>, };

export type CardRecords = { 
/**
 * The image of Magic's card back.
 */
card_back: string, 
/**
 * The art a token with no name is drawn with ([`NAMELESS_TOKEN_ID`]).
 */
nameless_token: string, 
/**
 * The cards named so, or with a face so named.
 */
cards: { [key in string]: CardEntry }, 
/**
 * The tokens and emblems named so: of each, the printing the card that makes it links.
 */
tokens: { [key in string]: CardEntry }, };

export type CardEntry = { name: string, scryfall_id: string, set: string, collector_number: string, layout: string, artist: string, faces: Array<CardFace>, };

export type CardFace = { name: string, mana_cost?: string, type_line: string, oracle_text?: string, power?: string, toughness?: string, loyalty?: string, colors: Array<string>, images: CardImages, };

export type CardImages = { art_crop: string, normal: string, large: string, };

export type UiLogRequest = { entries: Array<UiLogEntry>, };

export type UiLogEntry = { level: Level, message: string, data: unknown, };

export type Level = "debug" | "info" | "warn" | "error";

export type SessionError = { "kind": "unknownAgent", name: string, } | { "kind": "unknownCard", name: string, } | { "kind": "deck", reason: string, } | { "kind": "staleSession", session: SessionId, } | { "kind": "notDeciding" } | { "kind": "staleDecision", decision: DecisionId, open: DecisionId, } | { "kind": "noSuchCandidate", index: CandidateIndex, candidates: number, } | { "kind": "notCancellable" } | { "kind": "nothingToUndo" } | { "kind": "noPassOn" } | { "kind": "promptNotHeld" } | { "kind": "stalePrompt", revision: number, open: number, } | { "kind": "ended" } | { "kind": "badReplay", reason: string, } | { "kind": "io", reason: string, };

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

export type PlayerView = { status: Status, 
/**
 * Public scope of the opponent's card handling while their decision is open.
 */
handling?: CardHandling, 
/**
 * Public attackers whose destination is still being chosen.
 */
pendingAttackers?: Array<ObjectId>, turn: TurnView, players: Players, 
/**
 * Permanents, in the order they entered.
 */
battlefield: Array<ObjectId>, 
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
events: Array<GameEvent>, 
/**
 * The viewer's latest answer was taken back (`undo`): what happened since the decision
 * asked again never did.
 */
undone: boolean, 
/**
 * What the viewer's screen does between the view before and this one
 * ([`crate::script`]): every view a game in progress sends has one. A view built on its
 * own, and one recorded before scripts were, has none.
 */
script?: Script, };

export type Status = { "kind": "deciding", decision: DecisionView, } | { "kind": "thinking" } | { "kind": "playing" } | { "kind": "stopped" } | { "kind": "over", outcome: OutcomeView, };

export type CardHandling = { "zone": "library", side: Side, count: number | null, scry?: boolean, } | { "zone": "hand", side: Side, };

export type Side = "you" | "opponent";

export type TurnView = { 
/**
 * The game's turns so far, both players' together: 0 before the first turn. Orders turns.
 */
number: number, 
/**
 * The active player's own turn count, which is what players call "turn N": the starting
 * player's first turn and the other player's first turn are both 1. See [`own_number`].
 */
ownNumber: number, active: Side, step: Step, priority: Side, 
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
library: Array<ObjectId>, 
/**
 * Bottom first.
 */
graveyard: Array<ObjectId>, 
/**
 * The emblems' names.
 */
emblems: Array<string>, };

export type Declaration = "undeclared" | "willMulligan" | "kept";

export type ManaPool = { white: number, blue: number, black: number, red: number, green: number, colorless: number, };

export type ObjectView = { slot: ObjectId, zone: ZoneName, owner: Side, controller: Side, 
/**
 * The name of the face up, which keys its record in the card store
 * ([`crate::card_records`]); none for a card the viewer has not seen. A face-down
 * permanent's controller may look at it (CR 708.5): its card's name, for them.
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
pt: PowerToughness | null, 
/**
 * On the battlefield: the keyword abilities it has after continuous effects, printed and
 * gained alike ([CR 613.1f](https://yawgatog.com/resources/magic-rules/#R6131f): "Layer
 * 6: Ability-adding effects, keyword counters, ability-removing effects, and effects that say an object
 * can't have an ability are applied"), so that one gained until end of turn shows.
 */
keywords: Array<Keyword>, attachedTo: ObjectId | null, 
/**
 * The permanent that exiled it until it leaves (CR 610.3).
 */
exiledBy: ObjectId | null, 
/**
 * The object whose linked ability exiled it, if that object's other abilities may still
 * find it among the cards "exiled with" it
 * ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)).
 */
exiledWith: ObjectId | null, combat: Combat | null, };

export type Facing = "up" | "down";

export type PowerToughness = { power: number, toughness: number, };

export type CounterView = { kind: Counter, count: number, };

export type Combat = { "kind": "attacking", planeswalker: ObjectId | null, targetDeparted?: boolean, blocked: boolean, } | { "kind": "blocking", attacker: ObjectId | null, };

export type StackItem = { "kind": "spell", slot: ObjectId, targets: Array<TargetRef>, 
/**
 * The announced value of X.
 */
x: number, 
/**
 * The alternative and additional costs it was cast with, in its card's order: what
 * they change, as an overload cost turns each "target" into "each", its printed text
 * does not say.
 */
paid: Array<PaidCost>, } | { "kind": "ability", source: ObjectId | null, ability: number, granted: boolean, name: string, 
/**
 * A triggered ability rather than an activated one.
 */
triggered: boolean, 
/**
 * Its cost lets permanents be tapped in place of mana, each a pick of its
 * controller's: the taps are seen before the ability is.
 */
tapsToPay?: boolean, 
/**
 * How many paragraphs of its face's text say abilities of its kind (triggered, or
 * activated) before the one that says it, so the UI finds its paragraph.
 */
nth: number, 
/**
 * Its rules text as its definition says it, when the printed text of its face has no
 * paragraph that says it: an ability another object grants, or one past the printed
 * paragraphs of its kind, as a token's or an emblem's with no text in the card store
 * is. None where the UI finds the `nth` paragraph.
 */
text?: string, controller: Side, targets: Array<TargetRef>, 
/**
 * The announced value of X.
 */
x: number, };

export type PaidCost = "kicker" | "offspring" | "impending" | "overload" | "evoke" | "flashback";

export type TargetRef = { "kind": "player", player: Side, } | { "kind": "object", slot: ObjectId, };

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
 * The prompt the decision's pick belongs to: its shape and purpose, the record's
 * parameters and its words.
 */
prompt: PromptView, 
/**
 * For a priority decision, where passing takes the viewer, for the pass button.
 */
passTo: PassTo | null, 
/**
 * For a priority where passing stops the viewer again in this step (the top of the stack
 * resolving, or combat damage), where the `pass_on` command takes them instead: on to
 * their next stop in a later step, unless the opponent does something first.
 */
passOn: PassTo | null, 
/**
 * The card the decision is for: the spell or ability resolving, or else the one on top of
 * the stack (which a target, an X or a payment is chosen for); the attacker whose combat
 * damage is divided; the land asking for life. None for priority and the other combat
 * declarations.
 */
source: ObjectId | null, 
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
 * How many of the viewer's answers the `undo` command can take back, latest first, across
 * the opponent's decisions and turns. Automatic cast follow-ups go back with the preceding
 * answer. Zero with nothing to take back.
 */
undo: number, 
/**
 * How many of the viewer's answers stand before this decision, counting those too old
 * to take back: its place among them, the same whenever an undo comes back to it, which
 * the UI marks its log by.
 */
undoDepth: number, 
/**
 * At a priority, each card in the viewer's hand they may neither cast nor play as a land,
 * and why: what the UI says of one a click finds nothing to do with.
 */
refused: Array<Refused>, };

export type PassTo = { "kind": "resolve" } | { "kind": "combatDamage" } | { "kind": "step", step: Step, } | { "kind": "nextTurn" };

export type Refused = { slot: ObjectId, reason: RefusalView, };

export type RefusalView = { "kind": "timing", by: ObjectId | null, } | { "kind": "landPlayed" } | { "kind": "mana" } | { "kind": "cost" } | { "kind": "target" };

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
blocks: Array<ObjectId>, 
/**
 * For a candidate that activates an ability of the object's own face, or puts a waiting
 * trigger on the stack: how many paragraphs of its face's text say abilities of its kind
 * (activated, mana abilities among them, or triggered) before the one that says it, as
 * [`StackItem::Ability`]'s `nth` counts them, so the UI finds the ability's paragraph.
 */
nth: number | null, 
/**
 * For a candidate that activates an ability or puts a waiting trigger on the stack: the
 * ability's rules text, when the printed text of its face has no paragraph for it, as
 * [`StackItem::Ability`]'s `text` says. None otherwise.
 */
text?: string, 
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
taps: Array<ObjectId>, 
/**
 * For a payment at a value of X: the total mana cost that value makes, as Oracle text
 * writes costs, so the UI can say what each X costs.
 */
cost: string | null, };

export type Candidate = { "kind": "object", slot: ObjectId, } | { "kind": "pass" } | { "kind": "playLand", slot: ObjectId, } | { "kind": "play" } | { "kind": "draw" } | { "kind": "keep" } | { "kind": "mulligan" } | { "kind": "trigger", index: number, source: ObjectId | null, ability: number, } | { "kind": "yes" } | { "kind": "no" } | { "kind": "done" } | { "kind": "attack", slot: ObjectId, } | { "kind": "defender", planeswalker: ObjectId | null, } | { "kind": "block", slot: ObjectId, } | { "kind": "lethal", slot: ObjectId, } | { "kind": "cast", slot: ObjectId, face: number, costs: Array<number>, } | { "kind": "activate", slot: ObjectId, ability: number, granted: boolean, } | { "kind": "mana", slot: ObjectId, color: Mana, } | { "kind": "pool", color: Mana, } | { "kind": "number", n: number, } | { "kind": "plan", index: number, } | { "kind": "player", player: Side, } | { "kind": "turnFaceUp", slot: ObjectId, };

export type OutcomeView = { "kind": "win", winner: Side, 
/**
 * Why the other player lost.
 */
reason: LossReason, 
/**
 * The card that made them lose, when one did and the session saw it.
 */
by: Culprit | null, } | { "kind": "draw" } | { "kind": "stalled", reason: Stall, };

export type Stall = "decisionsPerTurn" | "turns" | "stateBasedIterations" | "capacity";

export type LossReason = "life" | "library" | "effect" | "concede";

export type Culprit = { 
/**
 * The name of its face.
 */
name: string, 
/**
 * Which of its abilities did it, as its card's text orders them; none for a spell, or
 * for damage.
 */
ability: AbilityRef | null, 
/**
 * The ability's rules text, when the printed text of its face has no paragraph that
 * says it ([`StackItem::Ability`](super::StackItem::Ability)'s `text`).
 */
text?: string, };

export type AbilityRef = { kind: AbilityClass, nth: number, };

export type AbilityClass = "triggered" | "activated" | "static";

export type StopsSetting = { 
/**
 * Full control: every priority the viewer receives, even where passing is all they can
 * do. Otherwise the smart stops: wherever the viewer can act, an opponent's doings on the
 * stack and the steps below; and their own precombat main phase in any case, while it is
 * one of them.
 */
fullControl: boolean, 
/**
 * The steps of the viewer's own turns where they stop with the stack empty, as Arena's
 * phase stops: by default their main phases, and combat once creatures attack.
 */
yours: Array<Step>, 
/**
 * The steps of the opponent's turns where the viewer stops with the stack empty: by
 * default combat once creatures attack, and the end step.
 */
theirs: Array<Step>, };

export type GameEvent = { "kind": "zoneChange", slot: ObjectId, from: ZoneName, to: ZoneName, reason: Why, 
/**
 * Whether a spell went to the top or the bottom of its owner's library.
 */
libraryTop?: boolean, } | { "kind": "damage", source: ObjectId, to: TargetRef, amount: number, combat: boolean, } | { "kind": "draw", player: Side, cards: number, 
/**
 * The public slots drawn, top first. Older recordings carry only the count.
 */
slots?: Array<ObjectId>, } | { "kind": "scry", player: Side, top: number, bottom: number, } | { "kind": "topOrBottom", player: Side, top: boolean, } | { "kind": "libraryShuffled", player: Side, } | { "kind": "reveal", slot: ObjectId, } | { "kind": "lifeGain", player: Side, amount: number, } | { "kind": "lifeLoss", player: Side, amount: number, } | { "kind": "countersAdded", slot: ObjectId, counter: Counter, amount: number, } | { "kind": "countersRemoved", slot: ObjectId, counter: Counter, amount: number, } | { "kind": "tap", slot: ObjectId, } | { "kind": "untap", slot: ObjectId, } | { "kind": "mana", player: Side, 
/**
 * The mana ability's permanent, or the spell or ability that added it, in the slot
 * it had then: a source sacrificed for mana has left it by the time the view shows.
 */
source: ObjectId | null, color: Mana, amount: number, } | { "kind": "resolutionPayment", player: Side, item: StackItem, } | { "kind": "abilityFinished" } | { "kind": "tokensCreated", controller: Side, tokens: Array<ObjectId>, } | { "kind": "cast", slot: ObjectId, controller: Side, 
/**
 * The spell as it went on the stack: an opponent's spell that resolves unanswered
 * is gone from the stack by the next view, which shows its cast and resolution
 * together.
 */
item: StackItem, } | { "kind": "ability", item: StackItem, 
/**
 * A loyalty ability's cost: the loyalty counters it added or removed, which the
 * activation says rather than the counters
 * ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)): "The cost to activate
 * a loyalty ability of a permanent is to put on or remove from that permanent a certain
 * number of loyalty counters, as shown by the loyalty symbol in the ability's cost."
 */
loyalty: number | null, } | { "kind": "attack", player: Side, attackers: Array<ObjectId>, } | { "kind": "block", player: Side, blocker: ObjectId, attacker: ObjectId, } | { "kind": "stepBegins", step: Step, active: Side, } | { "kind": "gainControl", slot: ObjectId, player: Side, } | { "kind": "coinFlip", player: Side, won: boolean, 
/**
 * The permanent whose ability flipped it.
 */
source: ObjectId, } | { "kind": "lose", player: Side, } | { "kind": "win", player: Side, };

export type Kind = "priority" | "playOrDraw" | "keepOrMulligan" | "triggerOrder" | "attacker" | "attackTarget" | "blocker" | "blockTarget" | "damageDivision" | "x" | "targetCount" | "target" | "paymentPlan" | "manaPayment" | "castOffer" | "yesNo" | "chooseObject" | "chooseUpTo";

export type Step = "untap" | "upkeep" | "draw" | "precombatMain" | "beginningOfCombat" | "declareAttackers" | "declareBlockers" | "firstStrikeDamage" | "combatDamage" | "endOfCombat" | "postcombatMain" | "end" | "cleanup";

export type ZoneName = "library" | "hand" | "battlefield" | "graveyard" | "exile" | "stack" | "nowhere";

export type Why = "play" | "draw" | "discard" | "destroy" | "sacrifice" | "stateBased" | "exile" | "created" | "return" | "effect" | "cast" | "counter" | "resolve";

export type Counter = "plusZeroPlusOne" | "plusOnePlusZero" | "plusOne" | "plusOnePlusTwo" | "plusTwoPlusTwo" | "minusZeroMinusOne" | "minusZeroMinusTwo" | "minusOneMinusZero" | "minusOne" | "minusTwoMinusOne" | "minusTwoMinusTwo" | "acorn" | "aegis" | "age" | "aim" | "arrow" | "arrowhead" | "awakening" | "bait" | "blaze" | "blessing" | "blight" | "blood" | "bloodline" | "bloodstain" | "book" | "bore" | "bounty" | "brain" | "bribery" | "brick" | "burden" | "cage" | "carrion" | "cell" | "charge" | "chorus" | "coin" | "collection" | "component" | "conqueror" | "contested" | "corpse" | "corruption" | "credit" | "croak" | "crystal" | "cube" | "currency" | "death" | "deathtouch" | "decayed" | "defense" | "delay" | "depletion" | "descent" | "despair" | "devotion" | "discovery" | "divinity" | "doom" | "doubleStrike" | "dread" | "dream" | "duty" | "echo" | "egg" | "elixir" | "energy" | "enlightened" | "eon" | "everything" | "exalted" | "experience" | "eyeball" | "fade" | "fate" | "feather" | "feeding" | "fellowship" | "fetch" | "filibuster" | "film" | "finality" | "fire" | "firstStrike" | "flame" | "flood" | "flying" | "foreshadow" | "fungus" | "fury" | "fuse" | "gem" | "ghostform" | "glyph" | "gold" | "growth" | "harmony" | "haste" | "hatching" | "hatchling" | "healing" | "hexproof" | "hit" | "hone" | "hoofprint" | "hope" | "hour" | "hourglass" | "hunger" | "ice" | "impostor" | "incarnation" | "incubation" | "indestructible" | "infection" | "influence" | "ingenuity" | "ingredient" | "intel" | "intervention" | "invasion" | "invitation" | "isolation" | "javelin" | "judgment" | "ki" | "kick" | "knowledge" | "landmark" | "level" | "lifelink" | "loot" | "lore" | "loyalty" | "luck" | "magnet" | "manifestation" | "mannequin" | "matrix" | "memory" | "menace" | "mine" | "mining" | "mire" | "music" | "muster" | "necrodermis" | "nest" | "net" | "night" | "oil" | "omen" | "ore" | "page" | "pain" | "palliation" | "paralyzation" | "peace" | "petal" | "petrification" | "phylactery" | "phyresis" | "pin" | "plague" | "plan" | "plot" | "point" | "poison" | "polyp" | "possession" | "pressure" | "prey" | "promotion" | "pupa" | "quest" | "rad" | "rally" | "reach" | "refine" | "rejection" | "reprieve" | "rev" | "revival" | "ribbon" | "ritual" | "rope" | "rust" | "saurian" | "scream" | "shadow" | "shell" | "shield" | "shred" | "silver" | "skewer" | "sleep" | "sleight" | "slime" | "slumber" | "soot" | "soul" | "spite" | "spore" | "stash" | "storage" | "story" | "strife" | "study" | "stun" | "supply" | "suspect" | "takeover" | "task" | "theft" | "tide" | "time" | "tower" | "training" | "trample" | "trap" | "treasure" | "unity" | "unlock" | "valor" | "velocity" | "verse" | "vigilance" | "vitality" | "void" | "vortex" | "vow" | "voyage" | "wage" | "winch" | "wind" | "wish";

export type Mana = "white" | "blue" | "black" | "red" | "green" | "colorless";

export type CardType = "artifact" | "creature" | "enchantment" | "instant" | "land" | "planeswalker" | "sorcery";

export type Keyword = "firstStrike" | "flying" | "reach" | "vigilance";

export type Script = { 
/**
 * The stage starts over from the view's table, with no motion: a seat's first view, and
 * an undone one.
 */
reset?: Reset, 
/**
 * In order; the steps of one beat happen together.
 */
beats: Array<Array<ScriptStep>>, };

export type Reset = { 
/**
 * The numbers of the abilities on the view's stack, bottom first.
 */
abilities: Array<number>, };

export type ScriptStep = { act: Act, 
/**
 * The spell, ability, permanent or player responsible; none for the rules of the game.
 */
cause?: Subject, };

export type Subject = { "kind": "object", slot: ObjectId, } | { "kind": "ability", id: number, } | { "kind": "player", player: Side, };

export type Act = { "verb": "fly", from: ZoneName, why: Why, object: ObjectView, 
/**
 * Where in its zone's order it goes, counted from the bottom of a pile or the first
 * permanent: none is the end, in a new zone, and where it was, in the same one.
 */
at?: number, 
/**
 * The spell it becomes, when it goes to the stack.
 */
item?: StackItem, } | { "verb": "appear", from: ZoneName, why: Why, object: ObjectView, at?: number, item?: StackItem, } | { "verb": "vanish", subject: Subject, from: ZoneName, why: Why, object?: ObjectView, at?: number, } | { "verb": "emerge", id: number, item: StackItem, } | { "verb": "flip", object: ObjectView, } | { "verb": "tap", object: ObjectView, } | { "verb": "strike", source: ObjectId, to: Subject, amount: number, object?: ObjectView, } | { "verb": "streak", 
/**
 * A player, for mana out of their pool. None when the source has no place on the
 * table.
 */
from?: Subject, to: Subject, stream: Stream, amount: number, object?: ObjectView, } | { "verb": "count", tally: Tally, } | { "verb": "set", fact: Fact, } | { "verb": "showcase", object: ObjectView, } | { "verb": "dwell", subject: Subject, } | { "verb": "announce", notice: Notice, } | { "verb": "point", span: Span, arrow: Arrow, from: Subject, to: Subject, } | { "verb": "mark", span: Span, mark: Mark, on: Subject, } | { "verb": "lift", span: Span, handling: CardHandling, };

export type Stream = { "kind": "mana", color: Mana, } | { "kind": "damage" };

export type Tally = { "kind": "life", player: Side, life: number, } | { "kind": "pool", player: Side, mana: ManaPool, } | { "kind": "counters", counter: Counter, object: ObjectView, } | { "kind": "stats", object: ObjectView, };

export type Fact = { "kind": "turn", number: number, ownNumber: number, active: Side, step: Step, } | { "kind": "object", object: ObjectView, } | { "kind": "library", player: Side, order: Array<ObjectId>, } | { "kind": "item", subject: Subject, item: StackItem, };

export type Notice = { "kind": "turn", active: Side, } | { "kind": "scry", player: Side, top: number, bottom: number, } | { "kind": "topOrBottom", player: Side, top: boolean, } | { "kind": "coin", player: Side, won: boolean, source: ObjectId, };

export type Span = "begin" | "end";

export type Verb = "fly" | "appear" | "vanish" | "emerge" | "flip" | "tap" | "strike" | "streak" | "count" | "set" | "showcase" | "dwell" | "announce" | "point" | "mark" | "lift";

export type Mark = "eligible" | "selected" | "suggested" | "paying" | "friendly-target" | "hostile-target";

export type Arrow = "friendly-target" | "hostile-target" | "source" | "attack" | "block";

export type PromptView = { 
/**
 * Counts this prompt's states from 0. A click names the state it was made on.
 */
revision: number, 
/**
 * What the prompt is for. Its [`Shape`] is the purpose's ([`Purpose::shape`]). The wire
 * writes it as its name, and TypeScript has the closed list of names.
 */
purpose: Purpose, 
/**
 * Which part of the shape the open decision's pick answers: a prompt is a run of picks.
 */
part: PartName, 
/**
 * What the prompt asks, in words, from the wording table.
 */
title: string, 
/**
 * The words of the buttons a UI that holds the prompt's state itself puts on it.
 */
words: Words, 
/**
 * How much the prompt is about as a whole: the cards looked at on top of a library, a
 * crew cost's power, the life to pay. None where its purpose has no such number.
 */
count: number | null, 
/**
 * The objects the question is about, in candidate order: what a yes then picks among,
 * the creatures being aimed, the cards a yes or no or a mode asks about.
 */
about: Array<ObjectId>, 
/**
 * Whose cards the prompt is among and the zone they lie in: those of the first of
 * `about`, or else of the first object the decision offers. None where it has neither.
 */
from: Lies | null, 
/**
 * The mark on each card, ability and player that has one, at most one each, in no
 * order.
 */
marks: Array<Marked>, 
/**
 * The buttons, in the order they are laid out.
 */
buttons: Array<PromptButton>, 
/**
 * What the player has put together so far, by the prompt's shape; or that the session
 * holds no state for the prompt, which the UI then answers by candidate.
 */
body: ShapeBody, };

export type Purpose = "priority" | "target" | "choose" | "discard" | "cleanup" | "put-on-bottom" | "legend-rule" | "look" | "search" | "untap" | "manifest-dread" | "exile-cost" | "combat-damage" | "trigger-order" | "scry" | "arrange" | "put-back" | "may" | "may-search" | "move" | "fateseal" | "top-or-bottom" | "explore" | "unless-pays" | "pay-life" | "x" | "play-or-draw" | "mulligan" | "cast-offer" | "act" | "color" | "creature-type" | "card-name" | "divide-damage" | "distribute-counters" | "mana" | "crew" | "waterbend" | "attack" | "block";

export type ObjectId = number;

export type PartName = "pick" | "whether" | "instead" | "count" | "first" | "second" | "aim" | "plan";

export type Lies = { player: Side, zone: ZoneName, };

export type Words = { 
/**
 * The button that answers with what is picked.
 */
confirm: string, 
/**
 * That button where the prompt takes several and counts them.
 */
counted: string, 
/**
 * The button that answers with nothing.
 */
decline: string, 
/**
 * The button that answers yes, and the one that answers no.
 */
yes: string, no: string, 
/**
 * The button that answers as the session suggests: its payment, every attacker.
 */
auto: string, 
/**
 * The button that takes the prompt back, where it can be.
 */
cancel: string, 
/**
 * What each of `places` is called over its pile; or of an order, what its two ends are
 * called, the last and then the first.
 */
piles: [string, string], };

export type Marked = { on: Subject, mark: Mark, };

export type PromptButton = { id: ButtonId, label: string, 
/**
 * What it says under its label, where it says more: where a pass of priority leads.
 * Empty otherwise.
 */
caption: string, 
/**
 * Whether a click does anything now.
 */
enabled: boolean, 
/**
 * The one the prompt puts forward: at most one button of a prompt.
 */
primary: boolean, };

export type ButtonId = { "kind": "confirm" } | { "kind": "decline" } | { "kind": "auto" } | { "kind": "passOn" } | { "kind": "option", index: number, } | { "kind": "ability", nth: number, };

export type ShapeBody = { "shape": "pick", 
/**
 * The cards laid out to pick from when they are not on the table as it lies: the
 * top of a library, a hand, a graveyard. Empty when they are picked where they lie.
 */
tray: Array<ObjectId>, 
/**
 * How few and how many may be picked.
 */
least: number, most: number, 
/**
 * Those picked, in the order they were.
 */
picked: Array<Subject>, 
/**
 * Those the prompt asks about before it answers with them: the viewer's own
 * permanents a removal would target (UI Conventions, exception 7). A click on one
 * picks it and puts the question; the confirm button then answers.
 */
warned: Array<Subject>, } | { "shape": "order", items: Array<Subject>, } | { "shape": "split", piles: Array<Pile>, } | { "shape": "yesNo", 
/**
 * The cards the question is about.
 */
about: Array<ObjectId>, 
/**
 * Where the card the question is about goes on a yes, and on a no; none where the
 * question sends no card anywhere.
 */
places: [PlaceName, PlaceName] | null, } | { "shape": "number", values: Array<number>, value: number, 
/**
 * What the value means, where it means more than itself: the total cost an X makes.
 */
detail: string | null, } | { "shape": "mode", 
/**
 * The cards the choice is about.
 */
about: Array<ObjectId>, } | { "shape": "catalog", entries: Array<string>, 
/**
 * The entry chosen, by index.
 */
chosen: number | null, } | { "shape": "divide", total: number, 
/**
 * How much of it is not yet given out.
 */
left: number, shares: Array<Share>, } | { "shape": "pay", 
/**
 * The whole cost, and what of it is still to pay, each as the screen writes it: a
 * mana cost as Oracle text does, a crew cost as its power.
 */
cost: string, left: string, } | { "shape": "declare", 
/**
 * The line drawn from each object to what it is aimed at.
 */
arrow: Arrow, pairs: Array<Pair>, 
/**
 * Whether those lines are drawn: a block's are, and an attack's only where more
 * than one thing can be attacked, as the script draws them once it is declared.
 */
lined: boolean, 
/**
 * The objects picked whose aim the next click gives.
 */
aiming: Array<ObjectId>, } | { "shape": "candidates" };

export type Pile = { 
/**
 * Where its cards go; none where they stay where they lie, as the cards a put-back
 * leaves in the hand do.
 */
place: PlaceName | null, label: string, 
/**
 * The cards in it: where the place keeps an order, as they will lie, the top first.
 */
cards: Array<ObjectId>, 
/**
 * How many it must hold for the prompt to confirm, if a number.
 */
exactly: number | null, 
/**
 * It is settled: no card can be dropped into it or taken out.
 */
closed: boolean, };

export type Share = { on: Subject, amount: number, };

export type Pair = { from: ObjectId, to: Subject, };

export type PlaceName = "top" | "onTop" | "bottom" | "hand" | "graveyard" | "exile" | "battlefield";

export type Click = { "kind": "on", on: Subject, } | { "kind": "button", button: ButtonId, } | { "kind": "drop", on: Subject, pile: number, at: number, } | { "kind": "number", n: number, on: Subject | null, } | { "kind": "entry", index: number, };

export type ClickRequest = { 
/**
 * The open decision the UI received.
 */
decision: DecisionId, 
/**
 * The state of its prompt the click was made on ([`PromptView::revision`]): a click on
 * an older state is refused, as an answer to an older decision is.
 */
revision: number, click: Click, };

export type ClickReply = { "kind": "open", prompt: PromptView, } | { "kind": "answered" };

/** Each command: its arguments, its result, and the error it rejects with. */
export type Commands = {
  app_info: { args: Record<string, never>; result: AppInfo; error: never };
  ui_ready: { args: Record<string, never>; result: null; error: never };
  list_agents: { args: Record<string, never>; result: Array<AgentView>; error: never };
  start_game: { args: { request: StartRequest }; result: SessionId; error: SessionError };
  click: { args: { request: ClickRequest }; result: ClickReply; error: SessionError };
  choose: { args: { request: ChooseRequest }; result: null; error: SessionError };
  set_stops: { args: { request: SetStopsRequest }; result: null; error: SessionError };
  cancel: { args: { request: CancelRequest }; result: null; error: SessionError };
  undo: { args: { request: UndoRequest }; result: null; error: SessionError };
  pass_on: { args: { request: PassOnRequest }; result: null; error: SessionError };
  concede: { args: { request: ConcedeRequest }; result: null; error: SessionError };
  current_game: { args: Record<string, never>; result: GameUpdate | null; error: never };
  resume_game: { args: Record<string, never>; result: SessionId | null; error: never };
  capture_window: { args: Record<string, never>; result: number; error: SessionError };
  report_bug: { args: { request: BugReport }; result: BugSaved; error: SessionError };
  export_bugs: { args: Record<string, never>; result: number | null; error: SessionError };
  share_bug: { args: { request: ShareBugRequest }; result: null; error: SessionError };
  ui_log: { args: { request: UiLogRequest }; result: null; error: never };
  card_records: { args: { request: CardRecordsRequest }; result: CardRecords; error: never };
};

/** Each event's payload. */
export type Events = {
  "game-update": GameUpdate;
};
