# Manual multiplayer

Beryl's manual table is a shared table for Magic: The Gathering. Beryl tracks
cards, zones, counters and life totals while players apply the rules. Players
can use cards from the card store regardless of whether Beryl implements their
rules. Use **Host a table** or **Join a table** to play with a friend, or **Solo
table** to play one seat. The phone apps offer solo play, joining and resuming
as a guest.

The host owns the table and sends each seat a **view** of what it can see and a
**script** of the animations that lead to that view. The **stage** draws both
manual and engine games from those values. Each player can enable **assists**,
which propose manual actions for the player to apply.

## Related information

- [Cockatrice inventory](manual-multiplayer-cockatrice.md): the appendix to this
  document. Every Cockatrice control that manipulates a game, with the gesture,
  menu row and key that reaches it in Beryl. Read it beside the
  [Interaction](#interaction) section.
- [Engine architecture](engine-architecture.md), decisions K22 to K26: the
  shared view and script contract between the session and the stage.
- [Engine architecture](engine-architecture.md), sections 13 and 19.2: hidden
  information and the session's protocol for engine games.
- [UI Conventions](ui-conventions.md): Beryl's design language. Beryl's
  interface follows MTG Arena's except where that page lists an exception. Arena
  has no manual mode, so the manual table adds entries to that page's "Not seen
  in Arena" list.
- [Cockatrice](https://github.com/Cockatrice/Cockatrice): the reference for the
  action set and the shortcuts.
- [iroh](https://docs.iroh.computer/) and its
  [relay documentation](https://docs.iroh.computer/concepts/relays): the
  peer-to-peer transport and public relay service.

## Decisions

| ID  | Decision                                                                                                                                |
| --- | --------------------------------------------------------------------------------------------------------------------------------------- |
| T1  | Players share a private invite. The host is trusted with the game state and random outcomes.                                            |
| T5  | The manual table owns its own state. Both game modes use the same stage protocol.                                                       |
| T6  | Beryl places permanents in automatic rows. A player can reorder cards or pin them to a row.                                             |
| T10 | Context menus contain command labels. Commands that need a number or text open a prompt.                                                |
| T14 | Either player can act on public cards and either life total. The host checks access to hidden zones.                                    |
| T19 | The handshake compares `WIRE_HASH`, which covers the Rust definitions for table actions, game and lobby updates, and the link envelope. |

## The table

This section defines what the app stores for a manual game. The **table** is
that state: every card and where it is, each player's life and counters, the
turn, and the log of what was done.

The table lives in `beryl_table`. It uses the card store for printed facts and
owns the actions and save format.

A card on the table is an object with the fields below. The example is a
creature with a counter and a note.

```rust
Object {
    id: 41,
    card: Card::Named("Tarmogoyf"),      // or Card::Custom { name, types, .. }
    owner: Seat(0), controller: Seat(0), // seats are numbered from 0
    zone: Zone::Battlefield,
    face: Face::Front,                   // Front, Back or Down
    tapped: true, token: false,
    counters: [("+1/+1", 1)],
    stats: Some((4, 5)),                 // shown as 5/6: stats, then counters
    damage: 0,
    note: Some("blocked this turn"),
    attached_to: None, doesnt_untap: false,
    row: Row::Auto, known_to: [Seat(0), Seat(1)],
}
```

The fields carry these rules.

- **`card`** is a name in the card store, or a custom token a player described.
  The table never interprets a name. Type line, printed power and toughness,
  keywords and rules text come from the store when something needs them.
- **`owner` and `controller`** differ when a player has taken control of the
  other's permanent.
- **`face`** is `Front`, `Back` for the second face of a two-faced card, or
  `Down`.
- **`counters`** maps a free-form name to a count from 0 to 999. The interface
  offers the common names (+1/+1, −1/−1, loyalty, charge, lore, stun) and
  accepts any text.
- **`stats`** holds a manual power and toughness that replaces the printed one.
  The card shows the manual value if set, otherwise the printed value, plus its
  +1/+1 counters and minus its −1/−1 counters. A printed `*` counts as zero
  until a manual value is set.
- **`damage`** is damage marked on a permanent this turn.
- **`row`** is `Auto`, or a row the player pinned the card to. See
  [Placement](#placement).
- **`known_to`** lists the seats that know which card this object is. See
  [Hidden information](#hidden-information).

Zones are each seat's library, hand, graveyard, exile and sideboard, plus one
battlefield and one stack. Every zone is ordered. The stack's newest entry is
its top. A stack entry is a card or an **ability marker**: a stand-in for an
activated or triggered ability, which names its source card and carries a line
of text.

The table also holds the following.

- **Each player's life total and counters.** Beryl's table already shows a mana
  pool of six symbols beside each life total. On a manual table each symbol is a
  counter the player sets by hand, and a player may add counters with any name:
  poison, energy, storm.
- **The turn:** whose it is, and the current phase. The phases are Cockatrice's
  eleven: untap, upkeep, draw, first main, beginning of combat, declare
  attackers, declare blockers, combat damage, end of combat, second main, end.
- **The arrows players have drawn,** each with the seat that drew it.
- **The match:** each player's deck and sideboard, the starting life and hand
  size, and the score.
- **The log.** See [The log, undo and rewind](#the-log-undo-and-rewind).

### Hidden information

Only the host's machine holds the whole table. Everything sent to a seat has had
what that seat should not know removed.

- **A seat sees a card's identity only if it is in that card's `known_to`.**
  Otherwise the object reaches that seat with no name, and the stage draws a
  card back.
- **A card in a public zone is known to every seat** unless it is face down. A
  face-down card keeps the `known_to` it had when it was turned down, plus any
  seat that peeks at it. It is drawn as a card back for everyone, and its
  preview shows its face only to the seats that know it.
- **A card entering a hand becomes known to that hand's owner only,** and is
  given a new id, so the other player cannot follow a card they once saw into a
  hand. Revealing a hand adds the seats it is revealed to.
- **Knowledge is removed in exactly two places:** a card entering a hand, and a
  shuffle. Nothing else ever removes a seat from `known_to`.
- **A card entering a library keeps its `known_to`.** A card I put on top of my
  library is still known to me, and to you if you had seen it. Its position is
  public.
- **A sideboard is known to its owner.**
- **A shuffle forgets.** It clears `known_to` for every card shuffled and gives
  those cards new ids.
- **Looking adds knowledge.** Looking at cards adds the looker to their
  `known_to`. Revealing adds the seats revealed to.
- **The host's random numbers decide shuffles, die rolls, coin flips and random
  reveals.** Each outcome is written to the host's log, so a replay or an undo
  never rolls again. A shuffle's outcome is never sent to a seat.

The redaction rule is exact and is tested as stated: nothing sent to a seat (a
view, a script, a log line or a proposal) may contain a card name for an object
whose `known_to` excludes that seat.

An example shows the rule. Seat 0 draws a card. The two seats receive the same
object with one difference.

```json
{ "to_seat": 0, "object": { "id": 77, "zone": "hand", "name": "Brainstorm" } }
{ "to_seat": 1, "object": { "id": 77, "zone": "hand", "name": null } }
```

### Placement

Permanents are placed by Beryl's existing layout, not by the player. Each side
of the battlefield has a creature row, a row for other permanents and a group of
lands. A permanent's row comes from its type line in the store.

- **Dropping a permanent between two cards of its row reorders it.** The order
  is part of the table and is the same for both players.
- **Dropping a permanent in another row on its controller's side pins it
  there,** by setting `row`. An animated land goes to the creature row this way.
  The pin lasts until the card leaves the battlefield.
- **Dropping a permanent on the other player's side gives them control of it.**
- **Dropping an Aura, Equipment or Fortification on a permanent attaches it.**
  Any other card can be attached through its menu. An attached card sits tucked
  under its host and moves with it. Dragging it off unattaches it.
- **A face-down card and a custom token with no type sit in the creature row.**

There are no free coordinates. A table can therefore never show overlapping or
off-screen cards, and the stage's layout checks apply unchanged.

### Actions

Every change to the table is an **action**: a small value, written as JSON,
naming what to do and to which objects. Gestures, menu rows, shortcuts, the
command list and assists all produce actions, and nothing else changes the
table.

The host log records object ids together with the zones they occupied when the
action was chosen. In the examples below, one action moves two cards from a hand
to a graveyard and the other adds a counter. Player requests use the same fields
with the viewer references described under
[Views and animation](#views-and-animation).

```json
{ "move": { "objects": [12, 15], "in": { "hand": 0 },
            "to": { "graveyard": 0 }, "face": "front" } }
{ "counter": { "objects": [41], "in": "battlefield",
               "name": "+1/+1", "add": 1 } }
```

The complete set follows. Each takes one object or many, so acting on a
selection is one action and one log line.

| Action                                    | What it does, and what it takes                                                                                                                               |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `move`                                    | Moves cards to a zone: a position (top, bottom, or nth), a face, and tapped or not. The zone may be `play` or `resolve`, which the host works out. See below. |
| `reorder`, `pin`, `give`                  | Changes a card's place in a row or a hand, pins it to a row, or gives control to a seat.                                                                      |
| `tap`, `face`, `flag`                     | Sets tapped; sets the face (front, back, down); sets "doesn't untap".                                                                                         |
| `peek`                                    | Adds the acting seat to a face-down card's `known_to`.                                                                                                        |
| `counter`, `stats`, `damage`, `note`      | Adds to or sets a named counter; sets or clears manual power and toughness, damage, a note.                                                                   |
| `attach`, `unattach`                      | Attaches cards to a host permanent, or detaches them.                                                                                                         |
| `clone`, `token`, `marker`                | Creates a copy of a card, a token (from the store or custom; optionally attached to a card), or an ability marker on the stack.                               |
| `remove`                                  | A token or marker ceases to exist.                                                                                                                            |
| `draw`, `mulligan`                        | Draws n cards; shuffles a hand into its library and draws n.                                                                                                  |
| `shuffle`                                 | Shuffles a whole library, or its top or bottom n cards.                                                                                                       |
| `look`                                    | Shows the acting seat the top or bottom n cards of a library, or all of it.                                                                                   |
| `close_look`                              | Closes the acting seat’s pile viewer and lowers the cards it lifted.                                                                                          |
| `reveal`                                  | Shows chosen cards, a whole zone, or n random cards of it, to named seats.                                                                                    |
| `lend`                                    | Lets another seat act on this hidden zone until it is next shuffled.                                                                                          |
| `always_reveal_top`, `always_look_at_top` | Keeps a library's top card revealed to all, or known to its owner.                                                                                            |
| `sort_hand`                               | Orders a hand by name, type or mana value.                                                                                                                    |
| `life`, `player_counter`                  | Adds to or sets a life total or a player's named counter.                                                                                                     |
| `phase`, `next_turn`                      | Sets the phase; passes the turn.                                                                                                                              |
| `roll`                                    | Rolls n dice of s sides, or flips coins.                                                                                                                      |
| `arrow`, `clear_arrows`                   | Draws an arrow from a card to a card or player; removes the acting seat's arrows.                                                                             |
| `load_deck`, `sideboard`, `ready`         | Sets a seat's deck; swaps cards between deck and sideboard; marks the seat ready.                                                                             |
| `start`, `next_game`, `concede`           | Starts a game; ends the game with a result and returns to the lobby; concedes.                                                                                |
| `set_assist`                              | Switches one of the acting seat's assists on or off.                                                                                                          |
| `flow`                                    | Opens a library flow for the acting seat. See [Library flows](#library-flows).                                                                                |
| `undo`, `rewind`                          | See [The log, undo and rewind](#the-log-undo-and-rewind).                                                                                                     |
| `batch`                                   | Applies several actions as one. See [Assists](#assists).                                                                                                      |

Two destinations of `move` are worked out by the host from the card store, so
that the guest's app computes nothing.

- **`play`** sends a land to the battlefield and anything else to the stack. A
  permanent whose rules text says it enters tapped arrives tapped.
- **`resolve`** sends a permanent to the battlefield and anything else to its
  owner's graveyard. An ability marker ceases to exist.

Applying an action produces a list of **changes**: reversible primitives such as
"this object moved from this position to that one" or "this field went from this
value to that one". Nothing alters the table except by a change. The log keeps
each action's changes, undo applies their inverses, and the adapter builds
animation from them.

A few changes happen as a consequence of an action. All but the last two are
Cockatrice's.

- **Leaving the battlefield clears a card's state:** tapped, counters, manual
  stats, damage, the doesn't-untap flag and attachments. Its note is cleared
  unless it moves to the stack.
- **A token that leaves the battlefield for anywhere but the stack ceases to
  exist.**
- **Arrows are removed** when a card they touch changes zone, and all arrows are
  removed when the turn passes.
- **"Untap all" skips cards flagged as not untapping.**
- **Passing the turn clears marked damage.**
- **A permanent with a printed loyalty or defense enters with that many
  counters.**

### Permissions

The host checks every action against decision T14 before applying it. An action
that fails is refused, the acting player sees the card shake, and nothing is
logged.

| The action touches                                       | Who may do it                                                           |
| -------------------------------------------------------- | ----------------------------------------------------------------------- |
| A card on the battlefield, the stack, a graveyard, exile | Either player.                                                          |
| A life total or a player's counters                      | Either player.                                                          |
| An arrow, the phase, the turn                            | Either player.                                                          |
| A hand, a library, a sideboard                           | Its owner. Another seat, for cards revealed to it or a zone lent to it. |
| A face-down card's identity (`peek`)                     | Its controller.                                                         |
| A deck, readiness, a concession                          | The seat they belong to.                                                |
| A rewind, or the undo of a batch                         | Either player.                                                          |

A revealed hand is the case this rule was written for. A discard spell that lets
me choose a card from your hand takes two actions (you reveal, I move the card
to your graveyard) where Cockatrice needs a third step in between: I tell you
which card, and you move it.

The host returns a stale refusal for a reference from an older view, from
another seat, or for a zone the object is absent from. In a batch, the host
resolves each member’s references against the state left by the previous
members. A reference can follow the same object to a public zone. If an action
replaces the object’s id, the old reference is stale.

### The log, undo and rewind

The log is the table's history: one entry per applied action, in order, each
with who did it and any random outcome. The host's copy is also the save file.
Each seat is sent the log with the same redaction as everything else.

```text
41  You      Cast Brainstorm.
42  You      Drew 3 cards.
43  You      Put 2 cards from your hand on top of your library.
44  Mara     Tapped Underground Sea.
45  You      Undid: Put 2 cards from your hand on top of your library.
```

Undo works in two ways: a key that undoes my own last action, and a rewind from
the log. Between them, anything can be undone.

- **The undo key undoes my own most recent action,** even if the other player
  has acted since. Each log entry records the objects and positions it touched.
  The undo is allowed when no later entry touched any of them, and is applied as
  a new entry, as line 45 shows.
- **When a later entry is in the way, the log opens with those entries
  highlighted.** The player rewinds, or the entries are undone first.
- **Any log line offers "Rewind to here",** which restores the whole table to
  the moment after that entry, for both players, with no confirmation from the
  other. The entries after it stay in the log, struck through. The rewind is
  itself an entry, and undoing it restores the table as it was before the
  rewind.
- **A batch is undone as one unit.** Its log line carries an undo control that
  either player may use, subject to the same rule about later entries.

Undo restores where things are, and never what people know.

- **Positions, order, state and ids are restored exactly.** Undoing a shuffle
  puts the library back in its earlier order with its earlier ids.
- **Knowledge is restored or kept, never reduced.** Undoing a shuffle gives back
  the `known_to` each card had before it. Undoing a draw returns the card to the
  library with the drawer still in its `known_to`, and the log says so: "Undid:
  Drew 3 cards (seen)".
- **A random result is removed, not reused.** Undoing a die roll removes the
  result, and rolling again draws a new number.

The host keeps a full copy of the table at the start of each turn and rebuilds
any other moment by replaying the log from the nearest copy. A rewind entry in
the log means "the table is now as it was after entry k", and replay honors it.
Replay is deterministic because random outcomes are in the log.

The save file carries a format version. Actions use the table's own vocabulary,
far more stable than Beryl's view types, and a newer build reads an older save
by migrating its actions, as Beryl's saved engine games already do.

## Views and animation

This section explains how the table reaches the screen. The stage draws any game
from two values: a view (the type `PlayerView`) and a script (the type
`Script`). The manual table produces the same two values, so the stage draws a
manual game with no knowledge that the rules engine is absent.

The piece that does this is an **adapter** between the table and those two
types. After each action it builds, for each seat, that seat's view and a script
of what that seat should watch.

The adapter uses the stage's shared animation verbs, such as `fly` for a card
traveling between zones and `tap` for a permanent turning.

- **Both seats get a script for every action.** When the other player looks at
  their top three cards, my script lifts three cards off their library. I see
  that it happened and not what the cards are.
- **The script must satisfy the session's existing rule:** replaying a script
  over the previous view gives the next view exactly. The adapter checks each
  transition and falls back to a snapshot if a release build cannot replay it.

Every action maps to verbs as follows.

| Action                                                            | Verb the stage plays                                                |
| ----------------------------------------------------------------- | ------------------------------------------------------------------- |
| `move`, `draw`, `give`, `mulligan`, `reorder`, `pin`, `sort_hand` | `fly`: the card travels to its new place.                           |
| `attach`, `unattach`                                              | `fly`: the card tucks under its host, or leaves it.                 |
| `tap`                                                             | `tap`.                                                              |
| `face`, `peek`                                                    | `flip`: the card turns over in place.                               |
| `counter`, `stats`, `damage`, `life`, `player_counter`            | `count`: a number changes.                                          |
| `token`, `clone`                                                  | `appear`.                                                           |
| `marker`                                                          | `emerge`: an ability lifts out of its source onto the stack.        |
| `remove`                                                          | `vanish`.                                                           |
| `reveal`                                                          | `showcase`: the card is held up for the seats it is shown to.       |
| `look`, `lend`, `close_look`                                      | `lift`: cards are lifted off their pile or lowered again.           |
| `arrow`, `clear_arrows`                                           | `point`: an arrow begins or ends.                                   |
| `roll`, `phase`, `next_turn`, `shuffle`                           | `announce`: a banner. A shuffle also plays `set` for the new order. |
| `flag`, `note`, `always_reveal_top`, `always_look_at_top`         | `set`: the table changes with nothing moving.                       |
| `undo`                                                            | The verbs of the action that reverses it.                           |
| `batch`                                                           | The verbs of its member actions, in one script.                     |
| `rewind`, `start`, `next_game`                                    | No motion. The stage takes the new view whole, with a banner.       |
| `load_deck`, `sideboard`, `ready`, `concede`                      | None. These change the lobby, not the table.                        |
| A pending proposal                                                | `mark`: a highlight on each card it would change.                   |

The shared protocol carries these fields for manual play.

- **A status meaning free play:** the game is live and no decision is pending.
  The existing statuses are deciding, thinking, playing, stopped and over, all
  of which describe an engine game.
- **A counter with a free-form name and a count up to 999,** beside the closed
  list of counter kinds the engine has, whose counts stop at 255. A name the
  engine's list has is sent as that kind. The stage uses its dedicated badge
  when it has one and a name badge for the remaining counters.
- **A note, a pinned row and the back face on an object.**
- **A sideboard zone.**
- **A field named `table`, empty in an engine game,** that holds what only a
  manual table shows: the log's lines, pending proposals and labels, the
  viewer's assists, the phase, the number of seats, hidden zones the viewer can
  use, and any prompt the host is holding for the viewer. Connection state
  arrives separately on `table-link`.

A view for the manual table sets the fields the engine fills from rules to
neutral values: no priority, no legal plays, no pending decision. The phase and
active player come from the table.

The view type names its two sides "you" and "opponent", as it does for engine
games, and the adapter translates seat numbers into those sides for each viewer.
A solo session has one seat. A hosted session reserves one guest seat, and the
view projects it as the opposing side.

### Object references for actions

`TableView.objectActions` lists the displayed cards and stack markers the viewer
can act on. Each entry pairs a stage subject with an opaque `reference` and its
expected `zone`. A card uses its displayed slot as the subject. A marker uses
the same ability number as its animation, so two markers from the same source
with the same text can still be chosen separately. A card back can have an
action reference, for example in a lent opposing pile.

The client looks up the entry for the displayed subject and copies its reference
and zone into the request. In a `TableAction`, a selection lists these
references in `objects` and supplies their common zone in `in`. An individual
target like `attach.host` carries a reference in `id` and its zone in `in`. The
same rule applies to explicit reveals and every member of a batch. Seat numbers
and zones use the existing action format. The generated protocol includes these
types, so `WIRE_HASH` covers them.

The host builds a `TableActionRefs` mapping with each seat's view and keeps it
alongside that seat's publication. It checks the request against that mapping
and the authenticated seat. References expire when the table revision changes,
even if the same slot still holds a card in the same zone. The log and save use
the host's object IDs.

## Networking

This section specifies how two apps share a table. The **host** is the player
whose app runs the table. The **guest** is the other player. The guest's app
runs no game logic at all: it shows the views it is sent and sends back the
actions its player takes. Assists are computed on the host too, for both
players.

The native desktop app hosts tables over the internet or on localhost. Desktop
and phone guests can join with an invite. The guest's app holds the link in Rust
and publishes updates under its own session ID. The table screen uses the same
commands and update types for solo, hosted and joined play.

### The link

A **link** is the connection between two apps: an ordered, reliable stream of
frames in each direction. TCP and iroh use length prefixes. Everything above the
link is unaware of how the frames travel.

```rust
trait Link: Send {
    fn send(&mut self, frame: &[u8]) -> Result<(), LinkError>;
    fn receive(&mut self) -> Result<Vec<u8>, LinkError>;   // blocks
    fn peer(&self) -> PeerInfo;        // name, route, round trip
    fn try_clone_sender(&self) -> Result<Box<dyn LinkSender>, LinkError>;
}
```

A `LinkSender` can send frames or close the connection from another thread while
the link waits in `receive`. Sending through either handle keeps frames whole.
Closing the connection wakes the receiver.

There are three implementations.

- **In memory:** two ends joined by channels in one process. Automated tests use
  it to run a host and a guest with no sockets.
- **Localhost TCP:** the host listens on a loopback port and the guest connects
  to it. Two apps on one machine use it, and it never touches the internet.
- **iroh:** for play between machines, described next.

### Shared iroh transport

The native app enables the shared session crate's iroh transport. The `iroh`
check family exercises the transport over loopback with the relay disabled.

iroh connects two endpoints by public key instead of by address. Each endpoint
stays connected to a **relay**, a server that passes encrypted traffic between
endpoints. Two endpoints first reach each other through a relay, then attempt a
direct connection through their home routers, and keep relaying only if that
fails.

- **Each app has a permanent identity,** a key pair created on first use and
  stored with the app's data.
- **An invite code names the host's key, its relay and a secret for this
  table.** The host copies it and sends it to the guest by any means. The guest
  pastes it. Nothing else is configured.
- **The relay is the public one run by n0, iroh's maker, by default.** It is
  free, rate limited and has no uptime guarantee. It cannot read the traffic,
  which is encrypted between the two endpoints. The relay address is a setting,
  so using a self-hosted relay (the server is open source) is a configuration
  change and not a code change.
- **The table secret stops strangers.** A connection that does not present the
  secret from the invite is closed before any table data is sent.

An invite code is text. The second form below is the localhost link's.

```text
beryl:k7fq3mxdpw2a91hc…        key, relay address and table secret, encoded
beryl-local:52114:3mxd         loopback port and table secret
```

The native desktop app creates an internet invite when you choose **Host a
table**, enter your name and select **Host**. Choose **Copy** beside the invite
in the lobby to share it. Check **Local only** in the host form to create a
localhost invite for a second Beryl instance on the same computer. Guests on
desktop, iOS and Android can paste an internet invite into **Join a table**.
Desktop hosts and guests on all platforms can reopen their saved tables from the
start screen.

### Messages

Frames carry JSON, compressed with DEFLATE. The `update` payload uses the same
JSON types as a local game.

The exchange opens with a handshake, shown here in order. The `table` field is
`new` or `resumed`.

```json
{ "hello": { "wire": "9c41e07ab2d35f18", "secret": "3mxd", "name": "Mara" } }
{ "welcome": { "seat": 1, "table": "resumed", "host": "Derek" } }
```

After it, the two sides exchange these messages.

- **Guest to host, `act`:** one action, with a number the guest chose.
- **Host to guest, `done` or `refused`:** the answer to an `act`, carrying its
  number. A refusal has a reason: `permission`, `stale` or `invalid`.
- **Host to guest, `update`:** the guest seat's next view and script. This is
  the same value a local game hands the interface.
- **Host to guest, `lobby`:** deck status, readiness and settings, before a game
  starts and between games.
- **Either way, `leave`:** the player left on purpose. The other player is told
  so, and no reconnection is attempted.
- **Either way, `ping` and `pong`:** send a numbered `ping` every five seconds.
  Echo the number in a `pong` to measure the round trip. Three unanswered pings
  count as a dropped connection.

```json
{ "act": { "n": 118, "action": { "tap": {
    "objects": [{ "view": "e61eb43cb57846e8b853fd0aa20c38bc", "handle": 3 }],
    "in": "battlefield", "tapped": true
} } } }
{ "done": { "n": 118 } }
{ "update": { "view": { "…": "…" }, "script": { "…": "…" } } }
```

The host is the only source of order. It applies actions one at a time in the
order they arrive, its own player's included, and sends each seat its update
before applying the next.

A wrong invite secret closes the link without a reply. After checking the
secret, the host reports a wire hash mismatch with a
`{"mismatch":{"reason":"Both players need the same version of Beryl."}}` frame
and closes the link.

Updates are small. Across 1,946 updates recorded from engine games, the median
is 12 KB of JSON and the 95th percentile 27 KB, which compress to 1.3 KB and 2.1
KB. A manual view carries the same objects. A busy turn of manual play is a few
actions a second, so the connection carries a few kilobytes a second.

### Responsiveness

Dragging and selection respond locally. A move changes the shared table when the
host accepts it and publishes the resulting view and script. A refused card
action can shake the card in place.

### Dropped connections and saving

The host keeps the authoritative save. The guest keeps its invite and seat name
for rejoining.

- **The host appends accepted actions and their random outcomes to the save.**
  Replay rebuilds the table and log from complete entries.
- **When the link drops, both tables freeze** under a banner saying the other
  player is reconnecting. Neither player can act.
- **The guest redials on its own,** every few seconds, with the same invite
  code. On success the handshake answers `"table": "resumed"` and the guest
  receives a whole view with no animation.
- **If the host's app quits, the host reopens the table from the start screen.**
  The original invite remains valid. Reopening replays the saved actions and
  restores the current guided opening prompt if one is pending.
- **If the guest quits, the guest can select Resume table on the start screen**
  to rejoin once the host is available. Leaving the table clears that option.
- **A guest never becomes the host.** If the host does not return, the game is
  over.

### Builds must match

The handshake compares `beryl_protocol::WIRE_HASH`. The build fingerprints the
Rust definitions for table actions, game and lobby updates, and the link
envelope, including their Serde attributes and custom serializers. It ignores
comments. It also includes other Rust tokens in those source files, so an edit
in a wire module can change the hash even if its JSON is the same.

When the fingerprints differ, both players need builds that agree. The host
reports this as “Both players need the same version of Beryl.”

### Two apps on one machine

Development and testing need a host and a guest on the same Mac.

- **A profile gives an app instance its own data folder,** and so its own saved
  tables, decks and settings. Two instances started with different profile names
  do not interfere.
- **The localhost link connects them,** so the pair runs with networking off.
- **One command starts the pair** at a shared local lobby. The visible run uses
  the `host` and `guest` profiles. The headless run uses temporary profiles and
  exits after both clients join.

```bash
just table-pair            # open the host and guest windows
just table-pair --headless # verify the localhost connection
```

## Cards and decks

The manual table can use any card Beryl can draw. The card store has a record
for every card legal in the Vintage format (34,205 of them) and 1,594 tokens.
Drawing a card needs nothing from the rules engine.

- **A deck is a list of names and counts,** with a sideboard. Nothing checks its
  size or legality.
- **Decks are imported as text,** pasted or opened from a file. Cockatrice's
  `.cod` files are also read.
- **A name the store does not have still loads.** The card is drawn as a plain
  frame with its name, and the lobby lists such names so a typo is noticed.
  Names are matched exactly, ignoring case. No near match is guessed.
- **Imported decks are kept** by name with the app's data and listed in the
  lobby.
- **A player's decklist is not sent to the other seat.** The lobby shows only
  its counts.

The text format is the one deck sites export. The parser accepts these forms.

```text
4 Brainstorm
4x Force of Will
1 Fable of the Mirror-Breaker // Reflection of Kiki-Jiki
3 Murktide Regent (MH2) 52

Sideboard
2 Surgical Extraction
SB: 1 Flusterstorm
```

- **A count, an optional `x`, and a name.** A set code in parentheses and a
  collector number after the name are ignored.
- **A two-faced card** is named by both faces with `//`, or by its front face
  alone.
- **The sideboard** begins at a blank line or a `Sideboard` heading, or is
  marked line by line with `SB:`.

Creating tokens and finding cards needs a search the store does not have yet.

- **Search by name** returns store records whose name contains the typed text,
  names starting with it first. The store is sorted by name, so a prefix search
  is a binary search, and a substring search is a scan of 36,000 short keys.
- **A custom token** takes a name, colors, a type line, power and toughness, and
  a note. It is drawn in Beryl's token frame.
- **The tokens a card makes** are linked from its store record, for every card
  that makes one. They are offered first in the token search and as rows in that
  card's context menu, as in Cockatrice.

## Interaction

The appendix lists the Cockatrice controls and their Beryl equivalents.

There are three ways to act at the table.

- **Direct manipulation** for what players do constantly: click, drag, drop.
- **The context menu** for what is done to a particular card or pile.
- **The command list** for everything, by typing a few letters.

Context menus, the command list and shortcuts use the registry in
`data/table-commands.json`. Each entry names the command, its target, the action
or UI operation it runs, any prompt, and any default key.

```json
{ "id": "library.look_top", "label": "Look at top cards…",
  "applies_to": "library", "asks": "number",
  "key": { "mac": "Cmd+W", "other": "Ctrl+W" } }
```

### Click

A plain click does the obvious thing to the card under it, as it does in an
engine game.

| Clicked                        | Effect                                                                                                   |
| ------------------------------ | -------------------------------------------------------------------------------------------------------- |
| A permanent                    | Taps or untaps it.                                                                                       |
| A card in hand                 | Plays it: a land to the battlefield, anything else to the stack.                                         |
| An entry on the stack          | Resolves it: a permanent to the battlefield, otherwise the graveyard. An ability marker ceases to exist. |
| A library                      | Draws a card.                                                                                            |
| A graveyard or exile pile      | Opens its viewer.                                                                                        |
| A card in a viewer             | Plays it, as from hand.                                                                                  |
| A life total                   | Opens the life prompt. See [Prompts for numbers and text](#prompts-for-numbers-and-text).                |
| A mana symbol or other counter | Adds one. A right-click removes one.                                                                     |
| A step on the step track       | Sets the phase. See [Turn and phase](#turn-and-phase).                                                   |

Two details complete "play".

- **Shift held while clicking a card in hand or in a viewer plays it face
  down,** to the battlefield.
- **A permanent whose rules text says it enters tapped is played tapped.**
  Cockatrice does the same from a flag in its card database.

The interface sends `move` to `play` or to `resolve`, and the host chooses the
zone.

### Select

Selection lets one action apply to many cards.

- **Cmd-click toggles a card** in the selection.
- **Dragging from empty table draws a rectangle** and selects what it touches.
- **Cmd+A selects every card in the zone under the pointer.** Cockatrice's keys
  for a row and a column are kept too.
- **Clicking a selected card acts on the whole selection.** If any selected
  permanent is untapped, all are tapped. Otherwise all are untapped.
- **A small summary sits by a selection of permanents:** the count, total power
  and total toughness.
- **Escape closes whatever is open on top** (the command list, a menu, a prompt,
  a viewer) and, when nothing is open, clears the selection.

### Drag

Dragging moves cards. A drag picks up the card under the pointer, or the whole
selection if that card is in it.

- **Every zone is a drop target:** each battlefield side and row, a hand the
  dragger may act on, the stack, and each library, graveyard and exile pile.
- **A library under a dragged card shows two halves,** top and bottom. Dropping
  on one puts the card there. Cockatrice reaches the bottom only by menu.
- **Dropping more than one card on a library opens the ordering tray,** the
  panel engine games already use for putting cards in an order.
- **Shift held on release drops the card face down.**
- **Dragging from a pile takes its top card.** Option held takes the bottom
  card.

[Placement](#placement) lists what a drop on the battlefield does.

### Arrows

Arrows show attacks, blocks and targets.

- **Dragging with the right button from a card draws an arrow** to the card or
  player it is released on. With a selection, each selected card gets an arrow.
- **An arrow drawn from a card in hand plays the card first.** The move and
  arrow are one log entry. If the arrow is refused, the card stays in hand.
- **The arrow's kind is worked out once, when it is drawn.** From a creature to
  the player who does not control it, or to that player's planeswalker, between
  the beginning of combat and combat damage phases: an attack. From a creature
  to a creature with an attack arrow: a block. Anything else: a target, colored
  as friendly or hostile by whether both ends have the same controller. Beryl
  already draws these four kinds in engine games.
- **Right-clicking an arrow removes it.** Cmd+R removes all the arrows the
  player drew.

### Hover

Hovering a card shows the enlarged preview engine games already show. It
replaces Cockatrice's card information panel and its middle-button preview. A
card the player may not know shows no preview.

### The context menu

A right-click on a card or pile opens a menu for it, or for the selection if the
card is selected. The menu is a list of commands and nothing else, by decision
T10. This is the menu for a creature token on the battlefield.

- **A header** names the card and whose it is.
- **Every row is a plain label,** with its key at the right edge. No row holds a
  button, a stepper, an icon or any other control.
- **For the player's own cards and piles, the first row is what a plain click
  does,** marked "click", so the menu teaches the faster route.
- **A "Move to" group** lists the zones the card can go to.
- **A row that is a switch shows a check mark when it is on,** as "Keep top card
  revealed" does in the library menu below.
- **A row whose label ends in "…" needs something more before it acts.**
  Choosing it closes the menu. A number or text is asked for with a prompt, and
  a target is picked on the table. Every other row acts at once.
- **"Every action…"** opens the command list, filtered to this card.

The first rows depend on what was clicked.

| Clicked                        | First rows                                                               |
| ------------------------------ | ------------------------------------------------------------------------ |
| My permanent                   | Tap; then Move to: Graveyard, Exile, Hand, Top and Bottom of library     |
| The other player's permanent   | Move to: Graveyard, Exile, Hand, Top and Bottom of library; Take control |
| A card in my hand              | Play, Play face down, Reveal; then Move to                               |
| A card on the stack            | Resolve; then Move to, with Graveyard first, for a countered spell       |
| A card in a graveyard or exile | Play; then Move to, with Battlefield and Hand first                      |
| My library                     | Draw, Draw cards…, Look at top cards…, Search, Shuffle                   |
| A life total                   | −1, +1, −5, +5, Set life…                                                |
| A two-faced card               | The rows for its zone, with Transform added                              |

The menu for a library has the same form. Rows that belong to an assist sit
under an "Assists" heading, and appear only when the player has switched that
assist on.

Three rules hold for every menu.

- **No submenu is deeper than one level.** "Bottom of library" is the deepest
  the menu goes.
- **The menu never asks a question the next step could take.** "Attach to…" and
  "Draw arrow…" open no dialog. Each closes the menu and starts an arrow from
  the card, and the click that ends the arrow picks the target.
- **A menu with fewer permissions is the same menu with fewer rows,** not a
  different one. The other player's hand offers its count. If the owner lends
  it, the menu also offers actions the viewer can take on that hand.

### Prompts for numbers and text

A row or shortcut that needs a number opens the stage's existing number prompt,
at the lower right where engine games ask for a value: a number with a minus and
a plus beside it, and a button that confirms. On a manual table the number can
also be typed. Engine games keep the two steps alone, as Arena has them.

- **The number starts at the likeliest value:** 1 for most commands, the current
  value for "Set life…".
- **Typing digits replaces the number.** Return confirms and Escape cancels.
- **The confirm button says what will happen,** for example "Draw 3".
- **A value typed with a leading + or − is applied to the current value,** where
  there is one. "+3" in the life prompt gains three life.
- **Text is asked for the same way,** with a text field in place of the number:
  a note, a counter's name, a custom token's name.

Drawing three cards is therefore Cmd+E, 3, Return, the same keystrokes as in
Cockatrice, with the prompt in the corner instead of a dialog over the table.

### The command list

Cmd+K opens a list of every command, filtered as the player types. It is where
rarely used commands live. The forty-odd library actions Cockatrice nests three
levels deep are each one search away.

- **It acts on the current selection,** or the pile or card under the pointer,
  and says which in the field.
- **Results are grouped by what they act on** and show their keys, so the list
  is also where shortcuts are learned.
- **A command that needs a number or text** opens the prompt after it is chosen.
- **Option+Return on a result binds a key to it.**

### Keyboard shortcuts

The defaults are Cockatrice's, so that a Cockatrice player's hands already know
them. The appendix lists every one. This section gives the rules that turn
Cockatrice's set into Beryl's.

- **Cmd stands for Ctrl on macOS, and Option for Alt,** as in Cockatrice's own
  macOS build. Other platforms use Ctrl and Alt.
- **A shortcut applies to the selection,** or to the card or pile under the
  pointer when nothing is selected.
- **Keys are matched by physical position.** On macOS, Option with a letter
  types an accented character, so shortcuts read the key pressed and not the
  character it produces.
- **Every shortcut can be rebound,** from the command list. Bindings are stored
  with the app's data.
- **Table shortcuts are live only at a table, and lobby shortcuts only in the
  lobby.** Cockatrice binds a few keys twice on this basis, and Beryl keeps both
  meanings. Elsewhere in the app, keys keep their usual meaning.

Where Cockatrice's defaults collide with the platform, they are resolved as
follows.

| Key       | At a manual table                                   | What gives way                                                                                           |
| --------- | --------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| Cmd+W     | Look at top cards of library                        | Close window                                                                                             |
| Cmd+M     | Mulligan                                            | Minimise window                                                                                          |
| Cmd+,     | Remove a −1/−1 counter                              | The settings window                                                                                      |
| Cmd+B     | Move card to bottom of library                      | Beryl's bug report, which is Cmd+Shift+B at a table and stays Cmd+B everywhere else, the lobby included. |
| Cmd+Q     | Quits Beryl after confirmation                      | Cockatrice's "Leave game", which it absorbs                                                              |
| Cmd+Space | Stays with macOS (Spotlight)                        | "Next phase", which keeps Tab and gains Space                                                            |
| Cmd+Z     | Undo, which Cockatrice does not have                | Nothing                                                                                                  |
| F2 to F12 | As in Cockatrice, where function keys are available | Nothing. See below.                                                                                      |

A Mac laptop's function keys are media keys unless Fn is held or the system is
set otherwise. Every command Cockatrice puts on a function key therefore has a
second route that needs none: the − and = keys change life, Space and Tab
advance the phase, a click on the step track sets it, and the piles open by a
click.

### Viewing piles

A viewer shows the cards of a pile in a grid: a graveyard, exile, a sideboard,
all or part of a library, or cards revealed to the player.

- **A viewer has a search field,** and can group by type and sort by name or
  mana value.
- **Cards in a viewer take the same clicks, drags and menus** as cards anywhere
  else.
- **Looking at a library is visible to the other player,** who sees the cards
  lifted and how many, and never which.
- **Closing a view of the whole library offers "Shuffle",** highlighted, with
  "Don't shuffle" beside it, since almost every search ends with a shuffle.

### Turn and phase

The table shows the phase on the step track engine games already have, and whose
turn it is on the life totals. Nothing is enforced: either player can set the
phase or pass the turn at any time.

- **Space or Tab moves to the next phase.** F5 to F10 jump to untap, draw, first
  main, combat, second main and end, as in Cockatrice.
- **Clicking a step on the track sets the phase to it.** When the active player
  clicks the untap step while it is already current, their permanents untap.
  When they click the draw step while it is already current, they draw a card.
  For the other player these clicks do nothing more.
- **Shift+Tab moves to the next phase and performs its action:** entering untap
  untaps the active player's permanents, and entering draw draws a card for
  them. If the player pressing it is not the active player, it only moves the
  phase.
- **Cmd+Return passes the turn.** The other player becomes active, the phase
  returns to untap, and arrows and marked damage are cleared. On a solo table
  the same player stays active.
- **The forward button at the lower right shows what Space will do,** as it does
  in engine games: "Next", with the phase it leads to beneath.

### The table's menu

The gear button at the top right of the table, which engine games already have,
opens Lobby, Next game, Concede, Assists and Leave table. On desktop it also
opens Shortcuts.

### Touch

Touch uses the same commands through taps, holds, the action sheet and the
command list.

| With a mouse and keyboard               | By touch                                                          |
| --------------------------------------- | ----------------------------------------------------------------- |
| Click                                   | Tap                                                               |
| Right-click                             | Long press. The menu opens as a sheet from the bottom edge.       |
| Hover preview                           | The sheet's header shows the full card.                           |
| Drag                                    | Drag, after a short hold so that scrolling a hand still works.    |
| Cmd-click, rectangle                    | "Select" in the sheet enters a mode where taps toggle cards.      |
| Shift (face down), Option (bottom card) | Rows in the sheet: "Play face down", "Bottom card".               |
| Right-drag arrow                        | "Draw arrow" in the sheet, then tap the destination.              |
| Command list (Cmd+K)                    | A search button in the corner, opening the same list full screen. |
| Shortcuts                               | None. Every command is in the sheet or the list.                  |

## Assists

An assist saves the players a run of manual actions that follow mechanically
from what is on the table. This is where the manual table goes beyond
Cockatrice.

All five assists share these rules.

- **An assist is off until a player switches it on.** Each player has their own
  five switches, in the lobby and in the table's menu. One player using an
  assist does not require the other to.
- **An assist never does anything a player could not do by hand.** Its result is
  a `batch` of the actions defined in [Actions](#actions).
- **Nothing changes without a click.**
- **The host computes every assist,** for both players, from the table and the
  card store: type line, printed power and toughness, the keyword list and rules
  text. No assist uses the rules engine, and the guest's app computes nothing.
- **An assist may be wrong, and shows its working.** Each change it would make
  is a line the player can switch off before applying.

An assist takes one of three forms.

- **A proposal** is a previewed batch that waits. Turn upkeep and combat raise
  proposals. A **proposal** is drawn as marks on the cards it would change and a
  panel listing each change as a line. Applying it applies every line still
  switched on, to both players' cards. The host recomputes a proposal after
  every action and withdraws it when its cause is gone.
- **A label** is a one-click suggestion on a single card or player. State nudges
  show labels.
- **A menu row** is a command that appears under "Assists" in a context menu
  while the assist is on. Library flows and ability menus add rows. Choosing the
  row is the click.

A proposal or label is shown only to a player who has that assist on. The combat
proposal is shown to each such player, and either may apply it. The turn upkeep
proposal is shown only to the player whose turn is beginning.

A batch is one log entry, and either player may undo it.

### Turn upkeep

When the turn passes to a player who has this assist on, they are offered a
proposal: untap their permanents, skipping those flagged as not untapping; draw
a card; and move to the first main phase. The draw is left out on the first turn
of the game for the player who went first.

### Combat

The combat assist proposes the result of combat damage from the arrows the
players have drawn. It appears once an attack arrow exists and updates as arrows
are added and removed, so it doubles as a preview of a block before the block is
committed to.

Its input is the set of attack and block arrows, and for each creature its power
and toughness as shown, the damage already marked on it, and the keywords in its
store record.

```text
rounds = [first strikers and double strikers], then [everything else and
         double strikers], or one round if nothing has first or double strike
for each round, for each creature dealing damage in it that is still alive:
    an unblocked attacker deals its power to the player or planeswalker attacked
    a blocked attacker assigns lethal damage to its blockers in the order their
        arrows were drawn (1 is lethal from a deathtouch source)
    any excess goes to the player or planeswalker attacked if the attacker has
        trample, or to its last living blocker otherwise
    a blocker deals its power to the attacker it blocks
after each round:
    a creature dies if its toughness is zero or less
    a creature without indestructible also dies if its damage is at least its
        toughness or a deathtouch source damaged it
    a lifelink source's controller gains the damage it dealt
```

An example: a 3/3 with first strike and trample attacks and is blocked by a 2/2
and a 1/1. In the first round it assigns 2 to the first blocker, 1 to the
second, and nothing tramples over. Both blockers die before the second round and
deal no damage. The proposal shows the two combat results below, followed by “Go
to end of combat.” Each card line ends with whose card it is.

```text
COMBAT
[x] Grizzly Bears dies                 theirs
[x] Llanowar Elves dies                theirs
```

- **It handles** first strike, double strike, deathtouch, trample, lifelink and
  indestructible.
- **It does not handle** damage prevention, protection, triggered abilities, or
  any effect that is not a keyword in the store record. The players switch off
  or work around the lines those change.
- **Applying it** marks damage on survivors, moves the dead to their owners'
  graveyards, changes life totals and loyalty, and moves the phase to end of
  combat.

### State nudges

With this assist on, the table marks a card or player that meets one of four
conditions with a small label on it. Clicking the label applies the fix.

- **A creature with toughness zero or less, or a creature without indestructible
  with damage at least its toughness:** move it to the graveyard.
- **A planeswalker with no loyalty counters:** move it to the graveyard.
- **Two legendary permanents with the same name under one controller:** choose
  one to move to the graveyard.
- **A player at zero life or less:** concede. This label is shown only to that
  player.

A nudge is a suggestion on the table and never applies itself.

### Library flows

A library flow turns a library effect that has a number into one guided step. It
uses the sorting tray engine games already show for the same effects.

| Flow          | What the player does                                            |
| ------------- | --------------------------------------------------------------- |
| Scry N        | Sees the top N cards and sorts them into "top" and "bottom".    |
| Surveil N     | Sees the top N cards and sorts them into "top" and "graveyard". |
| Look at top N | Sees the top N cards and puts them back in a chosen order.      |

A flow is offered in two places.

- **The library's context menu** has "Scry…" and "Surveil…" rows under
  "Assists". Each asks for N with the number prompt. With the assist on, "Look
  at top cards…" ends by asking for the order to put the cards back in.
- **A card whose rules text names one of these** has the row in its own menu
  with the number filled in. A spell on the stack whose text says "scry 2" has a
  "Scry 2" row. The table matches only the fixed wordings of these keyword
  actions: "scry N", "surveil N", and "look at the top N cards of your library".

No flow is written for one card's effect. A spell that draws three cards and
puts two back is "Draw cards…" with three, then a drag of two cards from hand
onto the library, which opens the ordering tray.

While a flow's tray is open, the host holds its state and the player's clicks in
it are sent as they are made, exactly as engine games handle a prompt. When the
player finishes, the result is applied as one batch and logged as what it was:
"Scried 2: 1 on top, 1 on the bottom". The other player sees the cards lifted
and where each one went. They see what a card is only if it went somewhere
public, as a surveilled card put into the graveyard does.

### Ability menus

This assist adds a card's own abilities to its context menu, read from its rules
text.

- **A planeswalker's loyalty abilities** are the paragraphs of its text that
  begin with a loyalty cost and a colon. Choosing one changes the loyalty
  counter by that amount and puts an ability marker on the stack. A cost of −X
  asks for X with the number prompt.
- **A permanent's activated abilities** are the paragraphs of its text with a
  cost before a colon, outside quotation marks and reminder text in parentheses.
  Choosing one puts an ability marker on the stack with that text. The table
  does not pay the cost. The player taps and pays by hand.
- **An ability marker resolves by a click,** like any stack entry, and then
  ceases to exist.

These lines of rules text show what is and is not read as an ability.

```text
+1: Draw a card, then discard a card.                  loyalty ability, +1
−X: Put X target cards from a graveyard on the bottom. loyalty ability, asks X
{T}: Add {G}.                                          activated ability
{2}, Sacrifice this artifact: Draw a card.             activated ability
Flying (This creature can't be blocked except by…)     not an ability row
```

The marker gives both players something on the stack to point at and respond to,
which Cockatrice has no way to show.

## Setup and match flow

The **lobby** is where players load decks, choose opening settings and prepare
for the next game. The start screen also offers **Resume table** when the
profile holds a saved hosted or joined table.

- **The host's lobby shows the invite code** and, once the guest connects, their
  name and the state of the connection.
- **Each player chooses a deck** and may open its sideboard to swap cards. The
  other player sees only that a deck is loaded, and its counts.
- **Each player sets their own assists.**
- **The host sets the game:** starting life (20), starting hand size (7), and
  who plays first (a roll, or a named player).
- **The guest marks themselves ready.** The host's Start button is available
  when both decks are loaded and the guest is ready.

Starting a game runs these steps in order, each visible on the table.

1. **It rolls for first,** if set to, showing both dice. The winner is asked
   whether to play or draw.
2. **It shuffles both libraries and deals each player a hand.**
3. **Each player keeps or mulligans** on the opening screen engine games already
   have. A mulligan shuffles the hand away and deals a new one. A player who
   keeps after mulligans is asked to put one card on the bottom for each
   mulligan taken, and may decline.
4. **The first player's turn begins** at the untap phase.

A game ends in one of two ways.

- **A player concedes.** The game is recorded as a win for the other player.
- **A player chooses "Next game"** from the table's menu, and picks the result:
  a win for either player, or no result. Both players see the choice.

Either way, both players return to the lobby.

- **The match score is shown** and updated.
- **Decks stay loaded, with sideboards open for changes.**
- **The loser of the last game chooses to play or draw,** unless the host
  changes the setting.
- **Nothing ends the match.** A fourth game, or a restart of the same game, is
  one click.

A solo table skips the invite and the second seat. One player loads a deck and
plays alone, to test a deck's draws or to try the interface. It is the same
table with one seat. Rules above that name "either player" or "the other player"
apply to the one player there is. The far side of the table is drawn empty, and
giving control and passing the turn to another player are unavailable.

## Engine and client boundary

- `beryl_table` owns state, actions, permissions, redaction, history, deck
  import and assists. `beryl_link` owns transport framing, the handshake and the
  host and guest loops. The optional `beryl_link_iroh` crate implements the
  internet transport.
- `beryl_protocol` owns the shared views, scripts and IPC definitions.
  `beryl_card_records` supplies printing and art records from the card store.
  `beryl_table_session` depends on these shared crates and projects each
  committed action into per-seat views and scripts. It uses neutral view
  defaults and puts manual-only fields in `PlayerView.table`. Engine views leave
  that field empty.
- `TableSessions` owns solo, hosted and joined sessions. `TableConnections` owns
  invites and profile resume records. A joined session sends intent to the host
  and receives its own redacted views. The host applies actions, completes
  prompts and saves the table. Both seats' publications are validated before the
  adapter commits a transition.
- The native client draws the table in `rust/beryl3d/crates/game_manual/`. It
  shares the scene, table placement and script playback with engine games. The
  table commands are defined in `beryl_protocol/src/ipc/table.rs`.

## Automated checks

- **Table state and adapter.**
  `just test-rust -p beryl_table -p beryl_link -p beryl_table_session` covers
  permissions, redaction, undo, saved-history replay and per-seat script replay.
  In-memory host and guest tests exercise deck loading, opening hands, combat,
  assist proposals, undo and reconnection. Save tests cover a guest resuming
  after quitting and the host recovering play-or-draw, mulligan and keep prompts
  after reopening, including undo and rewind.
- **Recorded views.** `just b3d test table-` checks native manual-table
  recordings from `data/beryl3d/fixtures.json` across the window matrix. Use
  `just b3d list table-` to find a board and `just b3d tree` with `--do` to
  inspect and drive it. The table crate checks the action templates in
  `data/table-commands.json`.
- **Native internet invites.** The Beryl 3D session test runs the native Host
  and Join commands over iroh loopback with the relay disabled.
- **Live startup.** `just table-pair --headless` joins a native host and guest
  over localhost. The `native-app`, `native-ios` and `native-android` Ignite
  families check desktop startup and build the phone packages.

## Manual reproduction

Use `just table-pair` to exercise the [table shortcuts](#keyboard-shortcuts) in
visible desktop windows. For a touch issue, use `just ios` or `just android` and
reproduce the action at a solo table. To exercise internet play, host an
internet table on one machine and join with its invite from another network.
Check that both players see each other in the lobby and can move cards on the
table. The automated checks are the landing gate.
