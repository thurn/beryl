# Manual multiplayer: Cockatrice inventory

*An appendix to [Manual multiplayer](manual-multiplayer.md). An inventory of
every control in Cockatrice that manipulates a game, read from its source at
commit `587c4b43c` on 2026-11-12, with the way each is reached on Beryl's manual
table.*

Cockatrice is an open-source app for playing Magic online with no rules
enforcement: a shared table where players move the cards by hand. Beryl's manual
table must let a player do everything Cockatrice does to a game. This document
is the checklist for that claim. It lists Cockatrice's menus, mouse gestures,
keyboard shortcuts, setup steps and automatic behaviors, and says for each one
how Beryl provides it or why it does not.

Three terms come from the design and are used throughout.

- **The context menu** is the menu a right-click opens on a card or pile: a list
  of plain labeled rows.
- **The command list** is a searchable list of every command, opened with Cmd+K.
- **An assist** is an optional helper, off by default, that works out a group of
  manual actions and applies them on a click.

## Related information

- [Manual multiplayer](manual-multiplayer.md): the design this appendix
  supports. Its Interaction section defines the gestures, the context menu and
  the command list named in the tables below.
- [Cockatrice](https://github.com/Cockatrice/Cockatrice): the source surveyed.
  The menus are built in its `game_graphics/player/menu` sources, the default
  shortcuts are in `shortcuts_settings.h`, and the game commands are the
  protobuf messages named in `game_commands.proto`.

## How to read the tables

Each table row is one Cockatrice control. The columns say what Cockatrice calls
it, its default key, and how Beryl reaches it.

- **Keys are written for macOS.** Cockatrice is a Qt application, so the
  defaults it writes as Ctrl are pressed as Cmd on a Mac, and Alt as Option.
  Beryl uses the same mapping. On other platforms read Cmd as Ctrl and Option as
  Alt.
- **A dash in the key column means Cockatrice binds no key by default.** Beryl
  binds none either, and the player can add one.
- **Every row that is a command is also in the command list,** unless its last
  column says it is not carried over. Rows that are gestures, such as a drag or
  a selection, are not commands. The "In Beryl" column names the fastest route,
  not the only one.
- **Cockatrice calls the battlefield "the table".** This document keeps that
  word inside Cockatrice's own labels and says "battlefield" everywhere else.

The "In Beryl" column uses these words.

- **Click, drag, right-drag:** direct manipulation on the table.
- **Menu:** a row in the context menu. A row quoted with "…" opens a prompt for
  a number or text.
- **Key:** the key in the key column does it.
- **List:** reached through the command list only. These are the rarely used
  actions.
- **Same:** Beryl behaves as Cockatrice does.
- **Changed:** Beryl provides the same result differently, as described.
- **Not carried over:** Beryl omits it, for the reason given.

## Cards on the battlefield

A right-click on a permanent opens this menu in Cockatrice. Every entry acts on
all selected cards.

| Cockatrice                  | Key          | In Beryl                                                                  |
| --------------------------- | ------------ | ------------------------------------------------------------------------- |
| Tap / Untap                 | —            | Click. Also the menu's first row.                                         |
| Skip untapping              | Option+U     | Menu, as the row "Doesn't untap", checked when on.                        |
| Turn Over                   | Option+F     | Menu. A two-faced card also has "Transform", for its other face.          |
| Peek at card face           | Option+L     | Menu, shown only for a face-down card the player controls.                |
| Clone                       | Cmd+J        | Menu.                                                                     |
| Move to (submenu)           |              | See [Moving a card](#moving-a-card).                                      |
| Attach to card…             | Cmd+Option+A | Drag the card onto its host. Menu entry starts an arrow to pick the host. |
| Unattach                    | Cmd+Option+U | Drag the card off its host. Menu, shown only when attached.               |
| Draw arrow…                 | Option+A     | Right-drag from the card. Menu, "Draw arrow…".                            |
| Power / toughness (submenu) |              | See [Power and toughness](#power-and-toughness).                          |
| Set annotation…             | Option+N     | Menu, as "Note…". Opens the text prompt.                                  |
| Reduce life by power        | Cmd+Shift+L  | List.                                                                     |
| Select All                  | Cmd+A        | Key. Selects every card in the zone under the pointer.                    |
| Select Row                  | Cmd+Shift+X  | Key. Selects the row under the pointer.                                   |
| Card counters (submenu)     |              | See [Counters on a card](#counters-on-a-card).                            |
| View related cards          | —            | Menu, one entry per related card, opening the enlarged preview.           |
| Token: (name)               | —            | Menu, one entry per token the card makes. See [Tokens](#tokens).          |

For a permanent the other player controls, Cockatrice shows only Draw arrow,
Clone, Reduce life by power and the selection entries. Beryl shows the full
menu, because either player may act on any public card (decision T14 in the
design). Its first rows differ: it has no "Tap" row first, and "Take control"
follows the "Move to" group.

## Cards in other zones

Cockatrice's card menu is shorter away from the battlefield. "Play" is the same
action a click performs.

| Zone                      | Cockatrice                         | Key         | In Beryl                                               |
| ------------------------- | ---------------------------------- | ----------- | ------------------------------------------------------ |
| Hand                      | Play                               | —           | Click. The menu's first row.                           |
| Hand                      | Play Face Down                     | —           | Shift+click, or Shift held when dropping. Menu.        |
| Hand                      | Reveal to… (all, or a player)      | —           | Menu, "Reveal". With two seats there is one recipient. |
| Hand                      | Clone, Move to, Attach, Draw arrow | as above    | As on the battlefield.                                 |
| Stack                     | Play                               | —           | Click, which resolves the entry. Menu, "Resolve".      |
| Stack                     | Play Face Down                     | —           | Menu.                                                  |
| Stack                     | Clone, Move to, Attach, Draw arrow | as above    | As on the battlefield.                                 |
| Graveyard, exile          | Play                               | —           | Click, in the pile's viewer. Menu.                     |
| Graveyard, exile          | Play Face Down                     | —           | Shift+click.                                           |
| Graveyard, exile          | Select Column                      | Cmd+Shift+C | Key, in a viewer.                                      |
| Library or sideboard view | Play, Play Face Down, Reveal to…   | —           | As for a card in hand.                                 |
| Revealed cards window     | Hide                               | Option+H    | Menu, in the viewer for cards revealed to the player.  |

## Moving a card

The "Move to" submenu sends the selected cards to a zone. In Beryl a move is
usually a drag, and the menu's "Move to" group lists the destinations.

| Cockatrice                        | Key        | In Beryl                                                                 |
| --------------------------------- | ---------- | ------------------------------------------------------------------------ |
| Top of library in random order    | —          | Drag onto the top half of the library. Menu, "Top of library".           |
| X cards from the top of library…  | —          | List, asking for the number.                                             |
| Bottom of library in random order | Cmd+B      | Drag onto the bottom half of the library. Key.                           |
| Table                             | —          | Drag to the battlefield. Menu, "Battlefield", from a graveyard or exile. |
| Hand                              | —          | Drag to the hand. Menu.                                                  |
| Graveyard                         | Cmd+Delete | Drag to the graveyard. Menu. Key.                                        |
| Exile                             | —          | Drag to exile. Menu.                                                     |

Cockatrice's piles accept a drop only on top. Beryl's library shows a top half
and a bottom half while a card is dragged over it.

Cockatrice's drag modifiers carry over, with one change.

| Cockatrice gesture                           | In Beryl                                                                                   |
| -------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Drag a card, with the selection following    | Same.                                                                                      |
| Shift held while dragging: face down         | Shift held on release.                                                                     |
| Drag from a pile: the top card               | Same.                                                                                      |
| Ctrl-drag from a pile: the bottom card       | Option-drag. Cmd-click is taken by selection.                                              |
| Drop on another player's table: give control | Drag to the other side of the battlefield.                                                 |
| Drop on a table cell, at grid coordinates    | Changed. Rows are automatic; a drop reorders within a row or pins the card to another row. |
| Drop onto a zone view window                 | Drop onto the viewer.                                                                      |

## Power and toughness

Cockatrice's submenu has ten entries. Beryl keeps every key, puts two rows in
the context menu, and leaves the rest to the keys and the command list. A change
by one is a key press; anything else is typed.

| Cockatrice                            | Key                  | In Beryl                                     |
| ------------------------------------- | -------------------- | -------------------------------------------- |
| Increase power                        | Cmd+= or Cmd++       | Key. List.                                   |
| Decrease power                        | Cmd+−                | Key. List.                                   |
| Increase toughness                    | Option+= or Option++ | Key. List.                                   |
| Decrease toughness                    | Option+−             | Key. List.                                   |
| Increase power and toughness          | Cmd+Option+=         | Key. List.                                   |
| Decrease power and toughness          | Cmd+Option+−         | Key. List.                                   |
| Increase power and decrease toughness | —                    | List.                                        |
| Decrease power and increase toughness | —                    | List.                                        |
| Set power and toughness…              | Cmd+P                | Menu, "Set power and toughness…". Key.       |
| Reset power and toughness             | Cmd+Option+0         | Key. Menu, shown when a manual value is set. |

As in Cockatrice, a value typed with a leading + or − is applied to the current
value, and a plain number replaces it. "+1/+1" raises both by one, and "3/3"
sets both.

## Counters on a card

Cockatrice gives each card six anonymous counters, lettered A to F and told
apart by color. Beryl's counters have names, so a +1/+1 counter and a loyalty
counter are different things, and +1/+1 and −1/−1 counters change the power and
toughness shown.

Cockatrice's keys for counters A, B and C are kept as three quick slots.

| Cockatrice                            | Key                            | In Beryl                                                                                                                                                                                     |
| ------------------------------------- | ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Add / remove / set counter A (red)    | Option+. / Option+, / Option+/ | Menu, "Add (name) counter" and "Remove (name) counter", for the card's first counter: +1/+1 on a creature, loyalty on a planeswalker, otherwise the counter its rules text names. Set: List. |
| Add / remove / set counter B (yellow) | Cmd+. / Cmd+, / Cmd+/          | −1/−1 on a creature, otherwise the card's second counter.                                                                                                                                    |
| Add / remove / set counter C (green)  | Cmd+> / Cmd+< / Cmd+?          | A generic counter.                                                                                                                                                                           |
| Add / remove / set counters D, E, F   | —                              | Changed. "Add counter…" in the menu takes any name.                                                                                                                                          |
| Increment all card counters           | Cmd+Shift+A                    | Key. List.                                                                                                                                                                                   |

Counter values run from 0 to 999, as in Cockatrice. A counter dialog in
Cockatrice accepts arithmetic on the old value; Beryl's number prompt accepts
the same.

## Tokens

Cockatrice creates tokens from the player menu and from a card's related
entries.

| Cockatrice                              | Key                  | In Beryl                                                                                                                  |
| --------------------------------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Create token…                           | Cmd+T                | Key. Opens a search of the store's tokens, with "Custom token" as an entry.                                               |
| Create another token                    | Cmd+G                | Key. Repeats the last token.                                                                                              |
| Create predefined token (from the deck) | Option+1 to Option+0 | Key. The tokens the deck's cards make, in the order first listed.                                                         |
| Token: (name), on a card                | —                    | Menu on that card.                                                                                                        |
| Token: Attach to (name)                 | —                    | Menu. The token is created attached to the card.                                                                          |
| Token: Transform into (name)            | —                    | Changed. A two-faced card has "Transform", which turns it to its other face. For any other card this is not carried over. |
| All tokens                              | Cmd+Shift+T          | Key. Menu, when a card makes more than one.                                                                               |

The custom token form has Cockatrice's fields: name, color, power and toughness,
and annotation. Cockatrice's two checkboxes carry over as follows.

- **"Destroy token when it leaves the table"** is always on in Beryl. A token
  that leaves the battlefield for anywhere but the stack ceases to exist.
- **"Create face-down"** is the Shift modifier, as for any card.

## The hand

These entries are in Cockatrice's player menu under Hand, and on a right-click
of the hand.

| Cockatrice                                 | Key                 | In Beryl                                                             |
| ------------------------------------------ | ------------------- | -------------------------------------------------------------------- |
| View hand                                  | —                   | Not carried over. The hand is always shown.                          |
| Sort hand by name / type / mana value      | — / Cmd+Shift+H / — | Key for type. List for the others. Dragging reorders a hand by hand. |
| Reveal hand to…                            | —                   | Menu on the hand.                                                    |
| Reveal random card to…                     | —                   | Menu on the hand.                                                    |
| Reveal a chosen number of random cards to… | —                   | List, asking for the number.                                         |
| Take mulligan (choose hand size)           | Cmd+M               | Key. Asks for the number.                                            |
| Take mulligan (same hand size)             | Cmd+Shift+M         | Key.                                                                 |
| Take mulligan (hand size − 1)              | Cmd+Shift+Option+M  | Key.                                                                 |
| Move hand to top / bottom of library       | —                   | List.                                                                |
| Move hand to graveyard / exile             | —                   | Select all (Cmd+A) and drag. List.                                   |

Cockatrice's mulligan moves the hand to the library, shuffles and draws. Beryl's
keys do the same. The London mulligan, which then puts cards on the bottom, is
part of the guided start in the design's "Setup and match flow" section.

## The library

Cockatrice's library menu is its deepest: about forty entries on three levels.
Beryl's library menu shows the common ones as plain rows, and the command list
holds the rest. A row that needs a number opens the number prompt.

| Cockatrice                    | Key         | In Beryl                                                             |
| ----------------------------- | ----------- | -------------------------------------------------------------------- |
| Draw card                     | Cmd+D       | Click the library. Menu. Key.                                        |
| Draw cards…                   | Cmd+E       | Menu, "Draw cards…". Key.                                            |
| Undo last draw                | Cmd+Shift+D | Key. Menu. General undo (Cmd+Z) also covers it.                      |
| Shuffle                       | Cmd+S       | Menu. Key.                                                           |
| View library                  | F3          | Menu, "Search". Key.                                                 |
| View top cards of library…    | Cmd+W       | Menu, "Look at top cards…". Key.                                     |
| View bottom cards of library… | Cmd+Shift+W | Key. Menu, under "Bottom of library".                                |
| Reveal library to…            | —           | List.                                                                |
| Lend library to…              | —           | List. Lets the other player act on the library until it is shuffled. |
| Reveal top cards to…          | —           | Menu, "Reveal top card", for one. List for more.                     |
| Always reveal top card        | Cmd+N       | Menu, as the row "Keep top card revealed", checked when on. Key.     |
| Always look at top card       | Cmd+Shift+N | Key. List.                                                           |
| Open deck in deck editor      | —           | Not carried over. Beryl has no deck editor.                          |

The "Top of library" submenu:

| Cockatrice                             | Key         | In Beryl                                                      |
| -------------------------------------- | ----------- | ------------------------------------------------------------- |
| Play top card                          | —           | Menu. Dragging from the library takes the top card.           |
| Play top card face down                | Cmd+Shift+E | Key. Shift-drag from the library.                             |
| Move top card to stack                 | Cmd+Y       | Key. Drag from the library to the stack.                      |
| Put top card on bottom                 | —           | List.                                                         |
| Move top card to graveyard             | Option+Y    | Menu, "Mill". Key.                                            |
| Move top cards to graveyard…           | Option+M    | Menu, "Mill cards…". Key.                                     |
| Move top cards to graveyard face down… | —           | List.                                                         |
| Move top card to exile                 | —           | Drag from the library to exile. List.                         |
| Move top cards to exile…               | —           | List.                                                         |
| Move top cards to exile face down…     | —           | List.                                                         |
| Put top cards on stack until…          | Cmd+Shift+Y | Key. List. Asks for a card name or type and a number of hits. |
| Shuffle top cards…                     | —           | List.                                                         |

The "Bottom of library" submenu is the one place Beryl's menu nests, one level
deep.

| Cockatrice                                | Key | In Beryl                            |
| ----------------------------------------- | --- | ----------------------------------- |
| Draw bottom card                          | —   | Menu, under "Bottom of library".    |
| Draw bottom cards…                        | —   | List.                               |
| Play bottom card                          | —   | Option-drag from the library. List. |
| Play bottom card face down                | —   | List.                               |
| Put bottom card on top                    | —   | List.                               |
| Move bottom card to graveyard             | —   | Menu, under "Bottom of library".    |
| Move bottom cards to graveyard…           | —   | List.                               |
| Move bottom cards to graveyard face down… | —   | List.                               |
| Move bottom card to exile                 | —   | List.                               |
| Move bottom cards to exile…               | —   | List.                               |
| Move bottom cards to exile face down…     | —   | List.                               |
| Shuffle bottom cards…                     | —   | List.                               |

The library viewer keeps Cockatrice's controls.

| Cockatrice                      | In Beryl                                                                                                               |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Search field                    | Same.                                                                                                                  |
| Group by, sort by               | Same: group by type, sort by name or mana value.                                                                       |
| Pile view checkbox              | Not carried over. The viewer has one layout, a grid.                                                                   |
| "Shuffle when closing" checkbox | Changed. Closing a view of the whole library offers "Shuffle" as the highlighted choice and "Don't shuffle" beside it. |
| Close most recent view          | Escape. Same key.                                                                                                      |

## Graveyard, exile and sideboard

| Cockatrice                                             | Key    | In Beryl                                             |
| ------------------------------------------------------ | ------ | ---------------------------------------------------- |
| View graveyard                                         | F4     | Click the pile. Key.                                 |
| Graveyard: reveal random card to…                      | —      | List.                                                |
| Move graveyard to top / bottom of library              | —      | List.                                                |
| Move graveyard to hand / exile                         | —      | Open the viewer, select all, drag. List.             |
| View exile                                             | —      | Click the pile.                                      |
| Move exile to top / bottom of library, hand, graveyard | —      | As for the graveyard.                                |
| View sideboard                                         | Cmd+F3 | Key. List.                                           |
| Move a card to or from the sideboard                   | —      | Drag to or from the sideboard viewer.                |
| View custom zone                                       | —      | Not carried over. Beryl has no player-defined zones. |

In Cockatrice the other player may only view a graveyard or exile. In Beryl
either player may act on the cards in them.

## Life and player counters

Cockatrice shows eight counters beside each player: life, one for each of the
five colors, colorless, and "other", which players use for storm. A left click
adds one, a right click subtracts one, and a middle click opens a menu with "Set
counter…" and steps from +10 to −10.

| Cockatrice                                 | Key                      | In Beryl                                                                        |
| ------------------------------------------ | ------------------------ | ------------------------------------------------------------------------------- |
| Add life                                   | F12                      | Key. Also the = key. Menu on the life total: +1 and +5.                         |
| Remove life                                | F11                      | Key. Also the − key. Menu: −1 and −5.                                           |
| Set life…                                  | Cmd+L                    | Key. Click the life total, which opens the life prompt.                         |
| Add / remove / set "other" counter         | Cmd+\] / Cmd+\[ / Cmd+\\ | Key. A storm counter beside the life total, shown once nonzero.                 |
| Add / remove / set a color counter         | —                        | Click a symbol in the mana pool beside the life total; right-click removes one. |
| Left click +1, right click −1 on a counter | —                        | Same, on any player counter.                                                    |
| +10 to −10 steps                           | —                        | Changed. The life menu has ±1, ±5 and "Set life…".                              |
| (none)                                     |                          | "Add counter…" on a player takes any name: poison, energy, experience.          |

The plain = and − keys are free for life because the manual table has no text
chat to type into. Function keys on a Mac laptop need the Fn key unless the
system is set otherwise, which is why life has a second pair.

## Dice, coins and other player actions

| Cockatrice                                    | Key            | In Beryl                                                                             |
| --------------------------------------------- | -------------- | ------------------------------------------------------------------------------------ |
| Untap all permanents                          | Cmd+U          | Key. Also done by "Next phase with action" entering untap.                           |
| Roll die…                                     | Cmd+I          | Key. Asks for sides (2 to 1,000,000) and count (1 to 100).                           |
| Flip coin                                     | Cmd+Shift+I    | Key.                                                                                 |
| Tally: subtypes, total power, total toughness | —              | Changed. A selection of permanents shows its count, total power and total toughness. |
| Say (message macros)                          | Cmd+1 to Cmd+0 | Not carried over. There is no chat.                                                  |

## Phases and turns

Cockatrice has a toolbar of eleven phase buttons and a next-turn button. Beryl
shows the phase on its step track.

| Cockatrice                    | Key               | In Beryl                                                      |
| ----------------------------- | ----------------- | ------------------------------------------------------------- |
| Untap step                    | F5                | Key. Click the step on the track.                             |
| Upkeep step                   | —                 | Click the step.                                               |
| Draw step                     | F6                | Key. Click the step.                                          |
| First main phase              | F7                | Key. Click the step.                                          |
| Beginning of combat step      | F8                | Key. Click the step.                                          |
| Declare attackers step        | —                 | Click the step.                                               |
| Declare blockers step         | —                 | Click the step.                                               |
| Combat damage step            | —                 | Click the step.                                               |
| End of combat step            | —                 | Click the step.                                               |
| Second main phase             | F9                | Key. Click the step.                                          |
| End of turn step              | F10               | Key. Click the step.                                          |
| Next phase                    | Tab, or Cmd+Space | Tab or Space. The forward button. Cmd+Space stays with macOS. |
| Next phase with action        | Shift+Tab         | Key. Entering untap untaps; entering draw draws.              |
| Next turn                     | Cmd+Return        | Key.                                                          |
| Click the active Untap button | —                 | Click the active untap step: untaps all.                      |
| Click the active Draw button  | —                 | Click the active draw step: draws a card.                     |
| Reverse turn order            | —                 | Not carried over. It has no meaning with two players.         |
| Set turn order…               | —                 | Changed. The lobby's "First" setting names who plays first.   |

Cockatrice lets only the active player set the phase and lets anyone pass the
turn. Beryl lets either player do both.

## Arrows

| Cockatrice                                                             | Key      | In Beryl                                                                                                           |
| ---------------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------ |
| Right-drag from a card to a card or a player                           | —        | Same.                                                                                                              |
| Draw arrow… (menu)                                                     | Option+A | Key. Menu. Then click the destination.                                                                             |
| Arrow color by modifier: red, yellow (Ctrl), blue (Alt), green (Shift) | —        | Changed. Beryl works out the arrow's kind (attack, block, friendly or hostile target) from its ends and the phase. |
| Selected cards each get an arrow                                       | —        | Same.                                                                                                              |
| An arrow from a card in hand plays the card first                      | —        | Same.                                                                                                              |
| Right-click an arrow to delete it                                      | —        | Same.                                                                                                              |
| Remove all local arrows                                                | Cmd+R    | Key. Removes the arrows the player drew.                                                                           |
| Arrows deleted when the turn passes                                    | —        | Same.                                                                                                              |
| Arrows deleted when a card they touch changes zone                     | —        | Same.                                                                                                              |
| Arrows kept through a block of phases, deleted after                   | —        | Changed. Arrows last until the turn passes or a player removes them.                                               |

## Selection and viewing

| Cockatrice                                              | Key | In Beryl                                             |
| ------------------------------------------------------- | --- | ---------------------------------------------------- |
| Click a card: select it                                 | —   | Changed. A click acts. Cmd-click selects.            |
| Double-click a card: tap, play or resolve               | —   | Changed. A single click.                             |
| Double-click the library: draw                          | —   | A single click.                                      |
| Double-click a graveyard or exile: open its view        | —   | A single click.                                      |
| Ctrl-click: toggle a card in the selection              | —   | Cmd-click.                                           |
| Drag on empty space: selection rectangle                | —   | Same.                                                |
| Hover a card: card info panel                           | —   | Hover shows Beryl's enlarged preview.                |
| Middle button held: large card popup                    | —   | The same hover preview.                              |
| Hover action panel (Info, Main, Move, Counters, Tokens) | —   | The context menu.                                    |
| "Selected cards" menu in the menu bar                   | —   | The context menu on a selected card.                 |
| Rotate view clockwise / counterclockwise                | —   | Not carried over. Beryl's table has one orientation. |
| Set playmat                                             | —   | Not carried over.                                    |

## Setup and match

Cockatrice's lobby is a deck view with buttons. Beryl's is the lobby panel the
design describes.

| Cockatrice                                         | Key                       | In Beryl                                                                                                                                                       |
| -------------------------------------------------- | ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Host game                                          | Cmd+N                     | "Host a table" on the start screen. Key there.                                                                                                                 |
| Game settings: players, starting life              | Cmd+Shift+N               | The lobby's game row. Two seats only.                                                                                                                          |
| Password, buddies only, registered only, game type | —                         | Not carried over. An invite code is the only way in.                                                                                                           |
| Spectator settings; join as spectator              | —                         | Not carried over. There are no spectators.                                                                                                                     |
| Create or join as judge                            | —                         | Not carried over. Either player may already act on any public card.                                                                                            |
| Open decklists in lobby                            | —                         | Not carried over. A decklist is never shown to the other player.                                                                                               |
| Load deck…                                         | Cmd+O                     | Key. "Change" in the lobby: open a file or pick a saved deck.                                                                                                  |
| Load deck from clipboard…                          | Cmd+Shift+V               | Key. Paste a decklist.                                                                                                                                         |
| Load remote deck…; Load from website…              | Cmd+Option+O; Cmd+Shift+O | Not carried over. Beryl has no deck server and fetches no decklists.                                                                                           |
| Unload deck                                        | Cmd+Option+U              | Key. "Change" in the lobby.                                                                                                                                    |
| Sideboard lock; drag cards between main and side   | Cmd+Shift+B               | Changed. In the lobby the key opens the sideboard swap view. There is no lock: changes are open until Start. Beryl's bug report uses this key only at a table. |
| Ready to start                                     | Cmd+Shift+S               | Key. The guest's "Ready".                                                                                                                                      |
| Force start                                        | —                         | The host's Start, available once both decks are loaded.                                                                                                        |
| (first seat plays first)                           |                           | Changed. Start rolls for first, or uses the lobby's setting.                                                                                                   |
| (no opening hand is dealt)                         |                           | Changed. Start deals each hand and offers keep or mulligan.                                                                                                    |
| Concede / Unconcede                                | F2                        | Key. Asks first. There is no unconcede: the game is over and both players are in the lobby.                                                                    |
| (rematch: everyone readies again)                  |                           | "Next game" returns to the lobby with the score and sideboards open.                                                                                           |
| Game information                                   | —                         | The lobby panel, reachable from the table's menu.                                                                                                              |
| Copy game link; Invite to game…                    | —                         | "Copy" beside the invite code.                                                                                                                                 |
| Leave game                                         | Cmd+Q                     | The table's menu. Cmd+Q quits Beryl after asking to leave the table.                                                                                           |
| Kick from game                                     | —                         | Not carried over. The host leaves, and hosts again for a new invite.                                                                                           |
| Reconnect to a game in progress                    | —                         | Automatic, with the same invite code.                                                                                                                          |

Cockatrice binds Cmd+N, Cmd+Shift+N and Cmd+Option+U twice: once for one of the
lobby actions listed just above, and once for a game action listed in an earlier
section. Beryl keeps both meanings: the lobby meaning in the lobby, and the game
meaning at the table.

## Chat and the log

Cockatrice's chat does not carry over, by decision T13 of the design.

| Cockatrice                                               | Key          | In Beryl                         |
| -------------------------------------------------------- | ------------ | -------------------------------- |
| Chat input; Focus chat                                   | Shift+Return | Not carried over.                |
| `[[Card name]]` links; Option+click a card to insert one | —            | Not carried over.                |
| `@name` mentions                                         | —            | Not carried over.                |
| Clear chat                                               | F12          | Not carried over. F12 adds life. |

Cockatrice writes a log line for every action, and Beryl's log does the same in
its existing log panel. The kinds of line Cockatrice has, all of which Beryl's
log covers:

- **Lifecycle:** game started, player joined, left, lost connection, restored
  connection.
- **Setup:** deck loaded (with sideboard size), ready, conceded, unconceded.
- **Turn flow:** whose turn, each phase.
- **Movement:** a card played, put into play, moved to a hand, graveyard, exile,
  sideboard or library (top, bottom, or a number of cards from the top), each
  with where it came from and whether face down; control given.
- **Draws and shuffles:** cards drawn, a draw from an empty library, a mulligan,
  a shuffle of a whole library or part of one, a draw undone.
- **Looking and revealing:** a player looking at a pile or at its top or bottom
  cards, a reveal (to whom, and whether random), a library lent, a face-down
  card peeked at, a top card kept revealed.
- **Dice:** a die roll, several dice, a coin flip.
- **Card state:** tapped, untapped, all permanents untapped, set not to untap,
  annotated, power and toughness changed, turned face up or down, token created,
  token destroyed, attached, unattached, counters placed or removed.
- **Players and arrows:** a player counter set, an arrow drawn from one thing to
  another.

Beryl's log adds lines Cockatrice has no equivalent for: an action undone, the
table rewound, and an assist's batch applied.

## Automatic behaviors

Cockatrice's server changes the game in a few ways no player asks for. Beryl
keeps each, as a rule of the table or as an assist a player switches on.

| Cockatrice behavior                                                                                      | In Beryl                                                                                                              |
| -------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| A card leaving the table loses its counters, tapped state, power and toughness, and "doesn't untap" flag | Same. A rule of the table.                                                                                            |
| A card's annotation is cleared on leaving the table, except to the stack                                 | Same.                                                                                                                 |
| A token leaving the table for anywhere but the stack is destroyed                                        | Same.                                                                                                                 |
| A card moved face down or to another controller gets a new id                                            | Changed. A card gets a new id when shuffled and when it enters a hand, the two places it could otherwise be followed. |
| A card given to another player is annotated "Owner: name"                                                | Changed. The card shows its owner as Beryl's engine games do.                                                         |
| A shuffle forgets which top cards the owner had looked at                                                | Same.                                                                                                                 |
| A shuffle ends a lent library's write access                                                             | Same.                                                                                                                 |
| "Untap all" skips cards set not to untap                                                                 | Same.                                                                                                                 |
| Undo draw works only while the drawn card is still in hand                                               | Changed. Any action can be undone unless a later action touched the same cards.                                       |
| Playing from hand sends non-lands to the stack first (setting, on by default)                            | Same. This is what "play" means in Beryl.                                                                             |
| Playing from the stack sends an instant or sorcery to the graveyard, anything else to the table          | Same.                                                                                                                 |
| A card whose database entry says it enters tapped is played tapped                                       | Same, read from the rules text.                                                                                       |
| A played card's table row comes from its type                                                            | Same. Rows are always automatic.                                                                                      |
| Same-named cards pile up in threes on the table                                                          | Changed. Beryl's layout groups lands as its engine games do.                                                          |
| An attached card cannot be moved separately on the table                                                 | Same. It sits under its host until unattached.                                                                        |
| Dice, shuffles and random reveals use the server's random numbers                                        | The host's, recorded in the log.                                                                                      |
| Nothing is dealt, untapped or drawn automatically                                                        | Same, unless the "Turn upkeep" assist is on.                                                                          |

## Other Cockatrice features

These are parts of Cockatrice around a game that are not ways of manipulating
it. They are listed so that nothing is left unaccounted for.

| Cockatrice                                    | In Beryl                                                                                                                       |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Replays: recorded games, with a timeline      | Changed. The host's saved log reopens a table, and "Rewind to here" walks back through it. There is no separate replay viewer. |
| Sounds for phases and die rolls               | Beryl's existing sounds for its animations.                                                                                    |
| Card info dock                                | The hover preview.                                                                                                             |
| Player list with ping                         | The lobby shows the other player's name, and whether the connection is direct or relayed with its round trip time.             |
| Deck editor                                   | Not carried over. Decks are imported as text.                                                                                  |
| Game history and message docks, layout reset  | Not carried over. The log panel is the only panel.                                                                             |
| Moderator tools: ignore, report, private chat | Not carried over. There is no server and no chat.                                                                              |

## Protocol coverage

Cockatrice's wire protocol has 37 game commands. This table is the cross-check
that each has an action on Beryl's table, using the action names defined in the
design.

| Cockatrice command                                                             | Beryl action                                                                                  |
| ------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- |
| MoveCard                                                                       | `move`, `reorder`, `pin`, `give`                                                              |
| FlipCard                                                                       | `face`                                                                                        |
| AttachCard                                                                     | `attach`, `unattach`                                                                          |
| CreateToken                                                                    | `token` (optionally attached to a card), `clone`                                              |
| CreateArrow, DeleteArrow                                                       | `arrow`, `clear_arrows`                                                                       |
| SetCardAttr (tapped, doesn't untap, power and toughness, annotation)           | `tap`, `flag`, `stats`, `note`                                                                |
| SetCardCounter, IncCardCounter                                                 | `counter`                                                                                     |
| IncCounter, SetCounter, CreateCounter, DelCounter                              | `life`, `player_counter`                                                                      |
| DrawCards, UndoDraw                                                            | `draw`, `undo`                                                                                |
| Shuffle                                                                        | `shuffle`                                                                                     |
| Mulligan                                                                       | `mulligan`                                                                                    |
| DumpZone                                                                       | `look`                                                                                        |
| RevealCards, RevealRandomCards                                                 | `reveal`, `peek`, `lend`                                                                      |
| ChangeZoneProperties                                                           | `always_reveal_top`, `always_look_at_top`                                                     |
| RollDie                                                                        | `roll`                                                                                        |
| NextTurn, SetActivePhase                                                       | `next_turn`, `phase`                                                                          |
| DeckSelect, SetSideboardPlan                                                   | `load_deck`, `sideboard`                                                                      |
| ReadyStart                                                                     | `ready`, `start`                                                                              |
| Concede                                                                        | `concede`                                                                                     |
| Unconcede                                                                      | None. Conceding asks first.                                                                   |
| LeaveGame                                                                      | Closing the connection.                                                                       |
| GameSay                                                                        | None. There is no chat.                                                                       |
| KickFromGame, Judge, ReverseTurn, SetPlayerOrder, SetPlaymat, SetSideboardLock | None. See the tables above.                                                                   |
| (none)                                                                         | `marker`, `remove`, `damage`, `sort_hand`, `next_game`, `rewind`, `batch`: Beryl's additions. |

## Manual QA

This appendix is verified by walking it against the finished table. Start a host
and a guest with `just table-pair`, as the design's Manual QA section describes.

- **Check every row of every table.** For each row whose last column is not "Not
  carried over", perform the Cockatrice action in Beryl by the route the row
  names, and confirm the result in both windows.
- **Check every key.** Press each key in the key column at a table and confirm
  it does what its row says. Confirm that the keys shared with macOS (Cmd+W,
  Cmd+M, Cmd+comma, Cmd+R, Cmd+P, Cmd+=, Cmd+−) act on the table and not on the
  window.
- **Check the command list is complete.** Open it with nothing typed and confirm
  every row that is a command, and is not marked "Not carried over", appears in
  it under some name.
- **Check the "Not carried over" rows.** Confirm none of them is reachable, and
  that nothing in the interface refers to chat, spectators or judges.
- **Compare with Cockatrice directly.** Open the same two decks in a Cockatrice
  local game, play three turns there and the same three turns in Beryl, and
  confirm no step needed in Cockatrice was impossible in Beryl.
