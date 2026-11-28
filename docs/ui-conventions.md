# UI Conventions

MTG Arena is the reference for Beryl's flow, the order and timing of events,
table layout and wording (AGENTS.md, UI;
[Engine architecture](engine-architecture.md), K22). This page lists only what
Arena does not settle: where Beryl differs on purpose, what Arena has no
equivalent for, and what the recordings do not show.

The rulings were made on 2026-11-10 from an audit of 180 bug reports against the
recordings in `~/arena`. Each Arena finding cites the recording moments that
show it, as `<recording id> <time>`, for `just video film`.

This page is generated from `data/ui-conventions.json` (K28) by
`just conventions`: edit the data, not the page. The repo lane fails when the
page is stale.

## Beryl exceptions

Beryl does these and Arena does not. A bug report from the user adds, changes or
removes an entry; nothing else does.

1. **Phase icons.** Twelve icons, two rows of three on each side of the life
   total, filled where a stop is set, the same rows for both seats (by-2o5,
   by-42v, by-sjp, by-cio). Arena shows one row above the action button.
2. **Full cards that tap.** A permanent is its whole card and turns when it
   taps. Each row stands its permanents apart with a gap, and a crowded row
   shrinks them, to no less than three fifths of their size, before it lays one
   over another. Arena shows a crop of the art, on cards small enough that a row
   of them stands apart with a gap even on a crowded board (IACoyoys7w8 38:40
   and 42:45).
3. **No question before a choice.** Never ask a yes or no whose answer the next
   screen could take: open the search, chooser or payment with a way to decline
   (by-kac). Arena asks "Search your library…?" first.
4. **Target arrows by relationship.** Red at something an opponent controls,
   green at your own. This is skin, listed so nobody removes it (by-pnm, by-sm5,
   by-vrv). Arena draws every target arrow cyan, whoever controls either end;
   its one red line is an opponent's declared attack, with a red chevron under
   the attacker (UrGalw_f35w 40:58, UrGalw_f35w 48:57, IEXcjBn3BXk 34:04,
   UrGalw_f35w 48:33.5).
5. **Lands unstacked until seven.** Each land is separate until its controller
   has seven or more; then same-named lands stack (by-p9e, by-goui). Arena piles
   same-named lands from the second copy, a tapped pile apart from an untapped
   one, and a pile of four shows "x4" (UrGalw_f35w 17:00, UrGalw_f35w 48:44,
   iRr-GeSF0lI 49:10).
6. **Linked exile under its host.** A card exiled "until this leaves the
   battlefield" sits tucked under the permanent that exiled it (by-8g5). Arena
   shows no link on the table.
7. **Own-target warning.** Confirm before a counter or removal spell targets the
   player's own spell or permanent (by-cp2y, by-jmx9).
8. **Cards put back in one screen.** Cards a player puts from their hand on top
   of their library (Brainstorm, Jace, the Mind Sculptor's 0) are chosen and
   ordered in one screen over the shaded table: the library face down at the
   left, the cards put on it fanned down its face with the top card lowest and
   in front, and the hand fanned beside it. A click moves a card between the
   hand and the library, and a drag puts it at a height on the library
   (by-rm60). Arena has the cards picked in the hand itself, "Put two cards into
   your library. (ordering will be done in next step)" with Submit, then orders
   them in an "Order Cards" screen between Bottom and Top (AhTcvQ-lQQQ 4:52,
   IT6nwh6HtYk 10:35).
9. **AI thinking indicator.** While the AI opponent is deciding, a spinner shows
   beneath the opponent's life orb on desktop and phone: a faint ring with a
   bright arc that turns once every 0.8 seconds. It hides as soon as the
   opponent acts. It does not show during a combat step banner, during the
   opening, while the player is deciding, or at a manual table. Approved by the
   user on 2026-11-26. Arena has no AI opponent and shows only a disabled
   "Opponent's Turn" button.

## Settled as Arena

Arena's way stands where an earlier report asked for something else, so do not
restore the old behavior.

- **A batch exile.** Cards exiled together leave one at a time, about 0.3 s
  apart (by-5pj). Delve's cards gather on the graveyard, then peel off its top
  one by one into a vortex. Delve is the only batch exile in the recordings, all
  in one recording (5UHzpVvUkfI 26:06.7, 5UHzpVvUkfI 47:36.2).
- **A card a look puts in the graveyard.** It appears on the graveyard at pile
  size once the chooser closes; it is not shown large first (by-2ma). A
  surveilled card lands on the graveyard as the overlay closes; kept cards close
  with the overlay (iRr-GeSF0lI 21:23.9, SN9QFe2kM4Y 13:10).
- **A card you may cast from your graveyard.** While you may cast a card from
  your graveyard (flashback, such as Snapcaster Mage gives, escape, or a
  permanent that lets you), it lies at the right end of your hand's fan, after
  the cards really in your hand, ringed in a soft violet halo that breathes
  slowly, apart from the cyan outline of a card you may play. It flies from the
  graveyard into the fan in about half a second as it becomes castable, and back
  onto the graveyard when it no longer is. Cast, it flies from its place in the
  fan to the stack. The opponent's castable cards stay on their graveyard. When
  Past in Flames resolves, the cards in the graveyard that gained flashback
  leave the graveyard together and fan into the hand within about a quarter of a
  second, to the right of the cards already there, each wrapped in drifting
  white and violet smoke over the cyan outline of a card that may be played. One
  cast leaves the fan for the stack (tovCPHl-gOM 9:07.75, tovCPHl-gOM 15:02.5).
- **A countered spell.** It leaves before the spell that countered it (by-kxyq).
  The countered spell crumbles in place under the counterspell, which stays
  until the debris is gone and then fades, 0.5 to 0.9 s later (I3Te0m1bBFw
  39:50.0, IEXcjBn3BXk 51:35.0).
- **A finished ability.** It is removed at once, with no fade (by-c8b).
- **A line from an ability to its source.** It is always shown while the ability
  is on the stack, thin and faint as Arena draws it, and hidden during a prompt.
  A heavy arrow is not that line (by-wkr, by-afh, by-110h).
- **Additional costs.** A chooser that lowers the mana to pay (Delve) opens
  before mana taps; a sacrifice cost is chosen after the lands tap (by-jqd).
  Treasure Cruise's Delve chooser opens before any land taps; Natural Order's
  lands send their mana, then "Sacrifice a green creature." opens (5UHzpVvUkfI
  47:31.0, RwKoGGUmw2k 39:26.0, RwKoGGUmw2k 1:07:02.25).
- **An arrow to the pointer.** While a target is chosen, an arrow runs from the
  spell or ability that asks to the mouse, thin and pale. On something that may
  be chosen it becomes the arrow the choice will draw, in that arrow's color and
  along its way, and the click draws the stage's own. It follows the pointer
  itself, with no lag: only the swing onto a target takes time. Nothing is drawn
  over a target already chosen, and a touch pulls none (by-w8gv.21.67). As the
  prompt opens, a cyan arrow runs from the ability that asks to the creature
  under the cursor, and what may be chosen is outlined (IEXcjBn3BXk 23:13.17).
- **An opponent's spell returned to hand.** It breaks up in place on the stack
  over 0.5 to 1 s; it does not fly or flip toward their hand (by-e0l). Seen for
  Remand on the opponent's spells (3sEoP_s4DQE 45:32.0, 3sEoP_s4DQE 56:19.5,
  TbrYIs2juCw 36:04.5).
- **Attacking a planeswalker.** No prompt: an attacker defaults to the player,
  and the player pulls an arrow to the planeswalker (by-p00u). Attackers that
  can attack get a cyan outline; a clicked attacker turns gold, the opponent's
  life total and the planeswalker glow, and an arrow follows the cursor to the
  planeswalker (SswBO8aOOgc 33:30.5).
- **Button order.** It follows Arena prompt by prompt; there is no blanket
  "default on the right" (by-ceo3).
- **Cards put on the bottom after a mulligan.** A card clicked in the hand
  glides down into its slot under the hand in a quarter of a second, slowing as
  it arrives, and the cards left in the hand close up in about a sixth of a
  second. A card clicked in its slot glides back up to its place in the hand the
  same way. Nothing jumps from one place to the other (by-uo1r). Arena's card is
  dragged to a Library box at the left of the hand, then settles into the
  library in about 0.2 s, slowing as it arrives, and slips behind its top card;
  the hand closes up as the card leaves it. Beryl keeps its slots under the
  hand, where a card clicked again comes back (2VW6ZzmS8U4 8:05.4, AhTcvQ-lQQQ
  14:16.3, 6aai5sMxV1o 11:38.5).
- **Cards the opponent looks at.** They come off the top of their library face
  down at library size and lie beside it; a top-bound card slides back onto the
  library and a bottom-bound one slides away from it, to either side. A pill
  above their avatar reads "Scry: 1 top, 1 bottom." for about 3 s (by-pv6p,
  by-yfb2, by-nva). No card is enlarged (IHPV6yRaFlk 52:54.6, IHPV6yRaFlk
  53:31.2, qF4GF-vfc8Y 52:59.9, IEXcjBn3BXk 33:32.3).
- **Choosing from a list.** The entries stand in a column over the shaded table.
  A press lights an entry and "Submit 1" answers with it, where Arena takes a
  color at the press: no card asks for an entry yet, and the prompt is the one a
  longer list needs, a creature type or a card name. Mother of Runes' color:
  Choose One over Choose Color, and White, Blue, Black, Red and Green in a
  column, each taken at a click (UrGalw_f35w 45:18.5).
- **Choosing how to cast.** A card or permanent with more than one way to cast
  or use it opens a modal, "Choose One", showing the card once per option, with
  Cancel: an adventure, an alternative cost, a land's abilities. A permanent
  with two or more activated abilities that are not mana abilities, as a
  planeswalker, opens it on every click, with each of those abilities in the
  card's order: one it cannot activate now, as a loyalty ability its loyalty
  cannot pay, is shown dimmed and takes no press (by-lzl, by-9fb). Brazen
  Borrower, Bonecrusher Giant and Force of Will open it from hand, Wasteland on
  the battlefield. Liliana of the Veil at 3 loyalty opens it with all three
  loyalty abilities, the +1 and −2 outlined and the −6 without the outline,
  dimmed (SN9QFe2kM4Y 42:44.2, 3sEoP_s4DQE 50:44.3, 3sEoP_s4DQE 46:50.2,
  MnhyzykajXw 35:43, 2VW6ZzmS8U4 6:24.5, TbrYIs2juCw 1:10:20.5). The cards a
  `mode` prompt is about are the one thing on screen twice: the duplicate audit
  allows them while it is open.
- **Choosing X.** The value of X is set by a step down and a step up beside it,
  over the button that answers, which says the value under its label: "Pay" over
  "X = 1". Beside the value stands the whole cost it makes, and the permanents
  that would pay it are outlined as the value changes. Beryl has the single
  steps alone. Mind Twist's X stands under the spell between -5, - and +, +5, on
  a button that reads Pay over X = 0, with Cancel (0VsYGs88NXA 55:32.0,
  3sEoP_s4DQE 25:01.0).
- **Dividing among targets.** Each target wears its share on its card, between a
  step that raises it and a step that lowers it, under the arrow to it; "Submit"
  answers once everything is given out. The prompt says after what it asks how
  much is still to give, as a cost paid with permanents says what is left. Fire
  Covenant's damage: an arrow from the spell to each target, a count on each
  between an arrow up and an arrow down, Submit and Cancel (3sEoP_s4DQE 28:32.5,
  bJQDi6NIrZA 21:42.5).
- **Hover inspection.** A hovered card in hand rises upright and grows in place,
  with no inspector beside it: about 0.3 of the table's height wide, its foot on
  the bottom edge, less where it would cover a card a prompt asks you to pick.
  It may cover your own life orb. It eases up in a tenth of a second and back
  down into the fan in about a sixth when the pointer leaves. A hovered
  permanent gets a separate copy about three times its size, away from the
  cursor, with its keywords explained. A card on the stack gets none. A card a
  prompt holds up in its tray, such as a card put back from the hand, is already
  shown large: hovered, it only turns upright and lifts a little in its place,
  and dragged, it keeps its size and faces the camera (by-jxo1, by-q2hn). The
  raised hand card is 0.29 to 0.31 of the table's height wide (1.8 times a card
  at rest in its hand), its top 0.58 of the way down, its foot just inside the
  bottom edge, over the avatar where it lies there, and no preview shows beside
  it; the permanent's copy is estimated from stills. The hand card's rise (70 to
  130 ms) and drop (130 to 200 ms) were filmed at 30 frames a second; each
  covers about half its way in the first frame and slows as it arrives. In the
  "Order Cards" screen a hovered card turns upright and rises about 0.015 of the
  table's height within a frame, about its size at rest, and a dragged card
  stays upright at about its size there while its neighbors make room
  (jD7VqaT8VC8 45:26.77, SswBO8aOOgc 21:02.75, IEXcjBn3BXk 18:06.62, jD7VqaT8VC8
  45:27.5, SswBO8aOOgc 21:02.0, pE4XSNi-nZc 34:21.0, jD7VqaT8VC8 17:34.0,
  IT6nwh6HtYk 9:17.93, IT6nwh6HtYk 9:18.2).
- **Mana into what it pays for.** Each tapped land bursts in the color of its
  mana ("mana-birth") and sends one streak of that color straight to the spell
  on the stack, which flares as they land ("mana-hit"), in about 0.3 s. Mana
  does not stop at a pool (by-p1j). Seen for casts paid with lands (TbrYIs2juCw
  33:32.45, 3sEoP_s4DQE 45:30.85).
- **Ordering cards on a library.** Cards a player puts back on their library in
  an order of their choice (Halimar Depths, and the cards a look leaves on top
  or on the bottom) lie in one row under "Order Cards", the library's top card
  at the right: "Bottom" is written under the row's left end and "Top" under its
  right. A card dragged along the row goes where it is let go. A scry keeps its
  two piles, and cards put back from the hand keep their own screen (exception
  8). Ponder's "Order Cards", over "Drag cards to order them on top of your
  library.", has the three cards in a row with Bottom under its left end and Top
  under its right; Brainstorm's has the same ends (Qrd11-wXZrk 8:04.0,
  AhTcvQ-lQQQ 4:59.0).
- **Passing to the end step.** "End Turn" passes until the end step, or the
  player's next stop where the end step is not one, through every spell and
  ability the opponent puts on the stack meanwhile. It stops sooner only for a
  decision other than priority, such as declaring blockers; "Resolve" beside it
  is the way to stop at the opponent's next spell (by-sqo, by-xuku). The button
  reads "End Turn" over "To End", and goes blank while it passes (SswBO8aOOgc
  21:00, WQiYBv23buw 37:52, jD7VqaT8VC8 45:29).
- **Permanents that leave.** A permanent that is not a token dissolves where it
  is: destroyed, it breaks into ash at its slot; exiled, it becomes a vortex
  there. Nothing flies to the pile. What it leaves room for closes up only once
  it has gone (by-1ug). A destroyed creature glows and falls to ash over about 1
  s; the spell then goes to its graveyard. The row closes up after the permanent
  is gone (RwKoGGUmw2k 27:36.4, bJQDi6NIrZA 51:46, TbrYIs2juCw 30:37,
  5UHzpVvUkfI 58:17.0, 3sEoP_s4DQE 24:49.6).
- **Targeting a spell.** With one spell on the stack, a spell that targets a
  spell takes it with no prompt and no Confirm. With more, a "Choose One"
  browser of the stack, "Target a spell.", opens with nothing preselected,
  Submit and Cancel (by-2lwc, by-69ps). Counterspell and Mana Leak at a lone
  spell go straight onto the stack; Spell Pierce over two spells opens the
  browser at "Submit 0" (vPdoSA5H5xU 21:36.5, qF4GF-vfc8Y 23:58.5, jD7VqaT8VC8
  17:54.5, SN9QFe2kM4Y 26:54.0, pE4XSNi-nZc 40:55.0).
- **The hand's fan.** Your hand's cards lie along one circle over the near edge,
  about 14 card widths across, on the desktop and the phone. A card's turn and
  how far it sinks follow its distance from the fan's middle, so a crowded hand
  lies nearly flat and a spread one curves, and no card turns or drops further
  than its neighbors. the client shows the printed cost on the card face.
  Inspecting a card reveals its printed cost when a neighbor covers it. Measured
  from whole-table frames: a card's turn grows by about 0.0275° for each pixel
  it lies from the middle, with cards 180 pixels wide on a table 1920 wide, at
  5, 6, 7, 8, 10 and 18 cards. Its outer cards turn about 11° at 8 cards and 14°
  to 15° at 10 and 18. Cards sit about one card's width apart until the hand
  reaches the turn button, then close up. Each card's mana cost floats above its
  top right corner, over the card beside it, so that every cost shows at 10
  cards (nBj2ikeYCtk 1:07.0, sybcO8S7x6A 24:27.0, FGsR_FGChOU 2:25.0,
  kvVLJ_cS_KE 6:24.0, uzg_P976qhc 17:20.0, \_IBqPx0gcag 12:00.0, sybcO8S7x6A
  51:04.0).
- **The kept opening hand.** When the opening closes, the screen over the table
  goes at once, and the hand it showed drops from there into the hand at the
  near edge in about an eighth of a second, slowing as it lands. On the phone
  the cards drop from the opening's rows. Nothing appears in the hand from
  nowhere (by-qmbe). Arena's opening screen vanishes in one frame, and the hand,
  still fanned in the middle of the screen, falls into the hand at the bottom,
  most of the way in two frames of 30 and at rest after four (3SEgIYXJgdQ
  13:33.95, 2VW6ZzmS8U4 6:56.25, 2VW6ZzmS8U4 14:07.75).
- **The local player's scry.** The table is shaded and "Scry" shows the cards
  face up over it, BOTTOM on the left and TOP on the right, and Done; surveil's
  has Graveyard and Library boxes instead. The cards are the library's own: they
  come up from it as the prompt opens, and on Done the shade lifts at once and
  they go back to it at its size, none shown large on its way to the top or the
  bottom. A pill reads "Scry: 1 top, 1 bottom." (by-8qq, by-2wn7, by-kyig,
  by-1mrh). Seen for a scry of two and for surveil; no card changes face
  (SN9QFe2kM4Y 14:50.5, SN9QFe2kM4Y 14:52.7, SN9QFe2kM4Y 13:08.5, iRr-GeSF0lI
  21:16.5).
- **The opponent's turn.** Arena's "Opponent's Turn" label shows in the action
  button's place (exception 9 adds the thinking indicator beneath the opponent's
  orb while the AI is deciding) (by-58n, by-7ih, by-oyx).
- **Tokens.** They appear at their slot; they do not fly out of their source
  (by-kgc, by-81h). Arena's appears whole with a short glow, about 0.1 s after
  its ability leaves the stack; a batch appears as one group (UrGalw_f35w
  17:38.4, WQiYBv23buw 23:57.0, iRr-GeSF0lI 21:58.3).
- **Two items on the stack.** The top one still gets its target arrow. Each item
  beneath peeks out to the left of the one over it by a quarter of its width (a
  fifth past five items, as far as the room left of the stack allows), so a
  spell leaving from under another is seen as it goes (by-i88). A cyan arc runs
  from the top item to its target, even when that is the one card under it; the
  top card sits a little right of and below the one beneath, which shows about a
  third of its width beside it, and a countered spell crumbles in that strip
  (I3Te0m1bBFw 39:49.4, IEXcjBn3BXk 51:34.5, IEXcjBn3BXk 51:35.3, MnhyzykajXw
  36:19.5, 5UHzpVvUkfI 58:15).
- **Wording.** Arena's exactly, case and punctuation too: "All Attack", "Choose
  attackers." Every prompt's title and buttons are one table in the session
  (`engine/crates/beryl_session/src/prompt/wording.rs`), each entry beside the
  recording moments that show it. A browser's title is the line that asks: its
  sentence of rules text, or its heading where the line under it only says how
  to drag. A pick is confirmed by "Submit" and the number picked, an arrangement
  by "Done", a "may" by "Take Action" or "Decline" (by-u4g, by-a09o).

## No Arena equivalent

- **A whole table leaving.** When a subgame begins or ends, or anything else
  takes cards from several zones at once with no event to say why, the cards
  leave zone by zone, and a new game's cards come to their zones the same way.
  The cards of one zone move together, and the next zone's begin 0.3 s later.
- **Ability menus on a manual table.** With Ability menus on, a permanent's
  context menu lists its ability costs and the start of each ability under
  Assists. −X opens the number prompt. Choosing an ability places a marker on
  the stack. The marker shows the source card and the ability text on hover and
  resolves with a click.
- **Assist label.** State nudges put clickable tags on the affected cards and
  life total. Click a tag to apply its suggested action.
- **Chrome.** Round icon buttons; the log toggle top left; the bug button above
  undo with no gap kept for a missing button (by-kib, by-8g4).
- **Command list.** The manual table's command list shows what it acts on and
  filters commands by label as you type. Results are grouped by what they act on
  and show their shortcuts. Up and Down move between results, Return runs the
  selected command, Option+Return captures a new shortcut. The list reports when
  another command loses the key. Escape closes the list, and Shortcuts in the
  settings menu offers Reset shortcuts. On a phone, the search button opens the
  list full screen with a Close button and without key hints. Typed table
  prompts open at the top so the keyboard leaves the field visible.
- **Context menu.** The manual table's context menu names the card or pile and
  whose it is. It lists commands as plain labeled rows with key hints, check
  marks for active switches and at most one submenu level. A command that needs
  a number opens a prompt when chosen. On a phone, a long press opens a bottom
  sheet with the full card above the actions. A short hold before moving starts
  a drag. Dropping on the lower half of a library puts the card on the bottom.
  Draw arrow waits for a tap on the destination. Submenus replace the rows and
  offer Back. Select opens a bar with the count, Actions and Done while taps
  toggle cards. Play face down is available on the card, and Bottom card makes
  the next drag from that pile take its bottom card.
- **Creating tokens on a manual table.** The token chooser uses the command list
  layout and offers the selected card's tokens first. Typing searches the card
  store by name. Custom token opens a form for the token's name, type line,
  colors, power, toughness and note. Shift while choosing a token creates it
  face down.
- **Guided manual table start.** The step banner shows each player's die in the
  roll for first and shows another round on a tie. The winner, the named player
  or the previous game's loser gets Play or Draw on the opening screen. Both
  seats see their own opening hands and can Keep or Mulligan at the same time.
  They can confirm bottom cards or choose Done with none selected. The table
  moves to the first untap after both seats finish.
- **Library drop halves.** While a dragged card is over a library, the pile
  shows Top and Bottom. The half under the pointer lights up. Dropping multiple
  cards opens the ordering tray before the cards are moved.
- **Lobby.** Host a table, Join a table and Solo table open a manual table
  lobby. Phones can join or play solo. Each player chooses a deck, can change
  its sideboard and sets their own assists. The host shares an invite code. Both
  players see the connection, deck counts and match score. The host sets
  starting life, starting hand size and who plays first, and can start when both
  decks are loaded and the guest is ready. The gear menu opens the lobby as
  read-only information and lets either player record a result or confirm a
  concession. Both players return to the lobby with their decks ready for
  sideboarding.
- **Manual counters and untap flags.** Named counters use the existing counter
  badges with shortened names. Add counter offers six common names above the
  text field. Player counters appear beside the mana pool. A permanent marked as
  skipping untap shows a lock in its corner.
- **Manual table announcements.** The stage uses its existing step banner for a
  manual shuffle, roll, phase, turn, rewind or game start. It reads Shuffled,
  Rolled 4 (d6), Heads, the phase name, the active player's name followed by 's
  turn, Rewound to 41, or Game 2. The banner fades on the stage's clock.
- **Manual table connection.** A dropped connection freezes input on both seats
  under the existing step banner, which names the other player. The guest
  redials automatically and the returning view appears at once. A table that the
  other player left offers Lobby. The start screen offers Resume table for saved
  hosted tables and for the last table the guest joined. Quitting keeps the
  guest’s join available, and leaving the table clears it. Join failures appear
  on the join form, and a version mismatch appears for both players.
- **Manual table library flows.** When Library flows is on, the player's library
  menu offers Scry and Surveil under Assists. These and Look at top cards ask
  for a count, then open the shared sorting tray. A known card's Assists menu
  offers the library instructions on its visible face with the printed count.
  The other player sees the lifted cards and their destinations. A hidden card's
  identity stays private unless the other player already knew it or the card
  goes to a public zone.
- **Manual table log.** The log shows numbered entries with their actors.
  Rewinding strikes the entries after the chosen line. Cmd+Z and the undo button
  undo the viewer's latest action. Cmd+Shift+D undoes the viewer's latest draw.
  When later entries block an undo, the log opens and highlights them. Either
  player can undo a batch from its line or rewind from any line.
- **Manual table pile viewers.** The existing pile browser can be searched,
  grouped by card type, and sorted by name or mana value. The other player sees
  how many cards are lifted from a library during a look. Closing a full library
  search offers Shuffle and Don't shuffle, with Shuffle highlighted.
- **Manual table prompts.** Numbers, text and power/toughness are entered in the
  lower-right action dock. The field starts selected, and numeric entries have a
  step down and a step up. A sign changes the current value where one exists.
  The button says what it will do, such as Draw 3. Return confirms and Escape
  cancels.
- **Manual table prompts.** A guided choice on the manual table uses the same
  sorting tray or list as an engine game. The seat answering it sees the cards
  or choices. Across the table a library sort lifts only a packet of cards,
  which returns when the tray closes.
- **Manual table turn controls.** The step track lights the current step beside
  the active player's life total. Clicking an icon sets the phase. Clicking the
  lit Untap or Draw icon on your turn also performs that action. Cleanup selects
  End step. The Next button names the following phase and advances to it,
  wrapping to Untap. The Next turn command passes the turn.
- **Manual table: a click acts.** Click to tap or untap a permanent, play a
  card, resolve a stack entry, draw from the library, or open a graveyard or
  exile viewer. Shift-click plays face down. Click a life total to edit it in
  the number prompt. Click a mana symbol or a player counter to add one, or
  right-click to remove one. On phones, the mana total beside the life orb opens
  a sheet. Tap a symbol or counter to add one, or its minus button to remove
  one. The press shows immediately. A refused action shakes what was clicked.
- **Manual table: drawing and removing arrows.** Right-drag from a card to a
  card or life total to draw an arrow. A selected source draws an arrow from
  each selected card. A hand card plays first. A command that asks for a target
  starts the same aim and a click chooses the target. Escape cancels the aim.
  Right-click a drawn arrow to remove it, or use Cmd+R to remove all the arrows
  you drew.
- **Nothing overlaps.** The hand, the stack and their indicators never cover a
  control or each other. No layer of a library, graveyard or exile pile covers a
  card in play, for either seat, however long a row grows. The opponent's piles
  keep the band above their back row to themselves (by-biv, by-krxg).
- **Proposal panel.** Turn upkeep and combat show the suggested changes with one
  checkbox per line and a mark on each affected card. Apply commits the checked
  lines as one undoable batch. The panel shares the log column on desktop and
  has its own strip on the phone.
- **Quitting from a manual table.** On the desktop, the Quit shortcut asks
  whether to leave the table and quit Beryl. Cancel keeps the table open.
- **Selection rectangle.** On a manual table, dragging from empty felt selects
  the cards the rectangle touches. Selected cards wear the gold selection mark.
  A summary beside the selected permanents gives their count, total power and
  total toughness.
- **Suggestions are sensible.** The suggested untap targets and the auto-pay
  sources are the ones a player would pick (by-svq, by-ufmq).
- **The opponent's choice of top or bottom.** When the opponent puts a card on
  the top or the bottom of their library by their own choice (Subtlety's "their
  choice of the top or bottom"), a pill beside that library reads "Top" or
  "Bottom" as the card goes there, and fades as a scry's pill does. Your own
  choice gets none. Arena shows no words: the card only goes to the library
  (iRr-GeSF0lI 1:01:07.5) (by-6dw5).
- **Undo.** Every answer can be undone, across turns and prompts, the cast and
  its payment together (by-2ml, by-96t, by-pwe).
- **Why a click is refused.** An illegal click shows its reason at the permanent
  that forbids it (by-l2ez).

## Not seen in Arena

Built from motions and shapes Beryl already has. Arena has these, but the
recordings do not show them.

- **A battlefield row that scrolls.** A row with more cards than fit with a
  strip of each uncovered shows as many as fit, scrolls one group for each notch
  of the wheel or 48 points of a sideways drag, and shows the number of cards
  off each end, as "+12".
- **A card picked face down.** While the other player still has to pick, as for
  Show and Tell, a card a player has picked from their hand lies apart from that
  hand where the cards a player handles lie: face up for its player, face down
  for the other. On the phone it lies beside its owner's library, toward the
  table, as the cards the opponent looks at do. Arena's recordings show no such
  pick.
- **A card turning face up.** A face-down permanent turning face up enlarges in
  place and flips; one leaving reveals its face during its flight, and one that
  leaves for a hand or a library, where no face shows, joins the Revealed Cards
  as it goes. Nothing turns face up in the recordings, and one face-down card is
  seen leaving (by-5hcb, by-thod). Killed, Arena's face-down card dissolves as a
  card back and a "Revealed Cards" panel then shows its face at the top left
  (I3Te0m1bBFw 29:42.9).
- **A copy on the stack.** It appears on the stack with no flight. None is seen.
- **A cost paid as a spell resolves.** Mana streaks from its sources into the
  resolving spell or ability, as into a spell being cast. No such payment is
  seen.
- **A creature that enters attacking.** It gets an explicit choice of what it
  attacks, and its line stays drawn throughout (by-l26, by-y3sp).
- **A free mulligan.** In a shared-deck game, when a player's opening hand
  qualifies before a regular mulligan, the mode mulligan prompt shows "Free
  Mulligan" in place of "Mulligan" above the primary "Keep 7" button. Taking it
  reveals the hand to the opponent in the Revealed Cards pile (showcase) before
  the hand goes back into the shared library with the mulligan's motion and a
  new seven-card hand is dealt. While the player who took it waits for the other
  player to declare, the other player's opening shows "Your opponent has taken a
  free mulligan" under the title, where Arena's opening says "Your opponent has
  taken 1 mulligan and may take another", so the revealed hand reads as the
  mulligan's reveal and not as hidden information shown by mistake.
- **A hand in two rows.** In the client, a hand with more cards than one fan
  across the window holds with a strip of each uncovered, such as 60 cards, is
  laid in two or more fans, one behind and above the other, in hand order from
  the back fan to the front. Each card keeps a strip to press, and raises under
  the pointer as in one fan. Arena's recordings show no hand that large.
- **A linked exile returning.** The exiled card returns as its source departs,
  moving with it. No exiling permanent is seen leaving with its card returning
  (by-esr2).
- **A mulligan's hand.** It goes from sight where it lies, every card of it, as
  a permanent that leaves does; then the next hand is dealt as an opening hand
  is. No mulligan is filmed.
- **A permanent changing sides.** It flies across the battlefield to its new
  controller's row. Seen only before and after its move (0VsYGs88NXA 27:39.5,
  gIhJjyoJ0_M 53:21.5).
- **A permanent's chosen value.** A value chosen as a permanent entered (a
  color, a creature type, a card name) shows on the permanent on one line, as a
  manual card's note does. Its inspection panel says "Chosen:" and the value.
- **A spell that ends the turn.** When a resolving spell ends the turn, play
  goes on to the cleanup step with no banner of its own: the turn is cut short
  as any turn ends, and the spell goes to its owner's pile (a shared-deck game's
  shared exile pile) in the move it would make at any other time. Arena's
  recordings show no spell that ends the turn.
- **A target that moves.** An arrow or line ends before either of its ends
  moves, and an arrow is not drawn again at a card that changed zones while its
  spell or ability stays on the stack. No target is seen leaving in response
  (by-yfw, by-ad8, by-8uw, by-2wsl).
- **A token that leaves.** It dissolves in place like any permanent. No token
  leaving the battlefield is seen (by-1ug).
- **An ability with no source on the battlefield.** It emerges from the pile its
  source lies in, and appears on the stack when it has no source. No trigger
  from a graveyard is seen.
- **An Aura that takes control.** No Aura that gains control of what it enchants
  is in the recordings. Control Magic resolves as any Aura does: it flies from
  the stack to the creature and settles behind it in the creature's row. Then
  the creature slides, with the Aura, straight to its new controller's creature
  row, as a permanent changing sides does (Coveted Jewel, 0VsYGs88NXA 27:39.5).
  When the Aura leaves, the creature slides back the same way, after the spell
  that removed the Aura has gone.
- **Arranging a melded or merged permanent's cards.** When a melded or merged
  permanent is put into its owner's graveyard or library, the owner picks its
  cards one at a time, the bottom one first, in the ordinary pick prompt:
  "Choose the bottom card." The last card left goes on top. The recordings show
  no such move.
- **Banding.** When an attacker can start or join a band, the choice uses the
  existing option buttons. Attacking bands carry no extra mark on the table.
  Assign Damage uses the number control to assign a share to each creature in
  turn.
- **Cards a look returns.** Cards a look puts back into a library fly straight
  to it. The opponent's look at the top of their library that is not a scry
  lifts nothing: while they choose, their library lies as it was; each card it
  takes drops from the library into the table, deepest half way, and rises into
  their hand; the rest are never seen to move (Arena's Stock Up, 3sEoP_s4DQE
  46:04.0, 0VsYGs88NXA 1:13:06.3, WCknTZNbraE 1:22:28.2). Your own look's cards
  put back are not filmed (by-ubgt, by-o5fn).
- **Cards leaving a graveyard.** A card returned to the battlefield or to your
  hand flies there from its pile; one returned to the opponent's hand swings up
  to a readable apex beside the stack before gliding to their hand (jD7VqaT8VC8
  46:23.2). Returning to the battlefield or to your own hand is not seen.
- **Cards moving to and from the shared piles.** A card drawn, milled, put back
  or looked at in a shared-deck game uses the `fly` and `lift` motions a
  per-player game uses, starting or ending at the shared pile. A draw rises off
  the shared library and comes down to where the drawing player's draw ends: the
  viewer's beside the stack, the opponent's into their hand. A milled card
  crosses the table from the shared library to the shared graveyard on the same
  rising path as a per-player mill. The cards a player looks at lie beside the
  shared library, toward the middle of the table and half a card toward that
  player.
- **Cards under a merged permanent.** The other cards of a merged permanent
  (mutate) peek out from under it, as cards a permanent holds in exile do.
  Clicking one opens them all to read.
- **Casting while searching a library.** When a library search offers a spell to
  cast, the candidate buttons offer the cast alongside the cards the search can
  find and Done. After the cast, the search continues.
- **Choosing a companion before the game.** Before the opening hands are drawn,
  a player whose deck fulfills a sideboard card's companion condition gets the
  sideboard catalog titled Choose Companion, listing each card they may reveal.
  Done reveals none. Arena reveals a companion without asking.
- **Choosing a replacement effect.** When a draw could be replaced by dredge,
  the player picks the card to dredge, or declines and draws, with the pick
  prompt Beryl uses for choosing up to a number of cards. The title reads Choose
  an effect to apply, and its Decline button lets the draw happen. No card on
  Arena has dredge (by-bajq.3.2).
- **Choosing a sideboard card during a game.** The catalog shows eligible card
  names to the player making the choice. Done leaves every card in the
  sideboard. The chosen card is revealed and appears in its owner's hand.
- **Choosing the deck.** The New Game screen lists the decks a game can be
  played with, "WU Control" and "Dandân", as radio rows above the opponents, and
  its subtitle names the chosen one. On a phone the two stand side by side in
  the row of the sound button. The Dandân deck is one library both players draw
  from, so Fair deal, which compares two players' opening hands, is hidden while
  it is chosen. Arena's play screen has no equivalent.
- **Controlling another player.** While the viewer controls the opponent's turn,
  each decision they make for the opponent says "For opponent" before what it
  asks; a priority says only that.
- **Copies.** A permanent that is a copy shows the copied card's face with a
  "Copy" label where a token shows "Token" (the phone, which shows neither
  label, shows the copied card's scan). Its inspection panel names the card it
  really is.
- **Dealing a fair game.** With Fair deal ticked, Start leaves the start screen
  up while the session judges candidate deals, which takes a few seconds. The
  start button is disabled and reads "Dealing…" until the new game's first view
  arrives, and then the game screen opens. Arena has no fair deal.
- **Equipment.** It flies to the creature it becomes attached to. The equip
  ability leaves the stack with a flash at the creature; the equipment's own
  move is not seen (krskVfn2v5Q 46:10.45).
- **Evoke and cycling.** A card with evoke, or a land with cycling, opens the
  same "Choose One" modal. No evoke is seen; Triomes cycle straight to the
  stack, but whether a land could still be played is not shown (by-lzl, by-9fb).
  Arena shows no chooser before these cycles (RwKoGGUmw2k 19:23.5, WQiYBv23buw
  45:29.5).
- **FPS monitor.** Settings can show a read-only panel at the top left of the
  native client. It reports current FPS, the lowest frame rate implied by a
  frame gap in the last minute, the app’s target, the display link’s rate when
  available, and the graphics tier being drawn, Enhanced or Baseline, in the
  settings sheet’s words.
- **Hand cards put on the bottom.** The split prompt is headed "Put cards on the
  bottom." The player orders any cards they put on the bottom and can press Done
  with all their cards in hand. The opponent resolves the spell and draws cards.
  Their choice is hidden (kvVLJ_cS_KE 21:54.5).
- **Inspecting a face-down manual card.** The table shows a card back. Pointing
  at the card opens a face preview only if the viewer knows the card's identity.
- **Known cards in a library.** A known top card shows face up on its library.
  Clicking a library opens its browser, headed "Library" (or "Opponent's
  Library") and "N Cards": every card in library order, top first, each known
  card face up at its place and the rest as backs. No known card in the
  opponent's library is seen (by-nva, by-9es, by-dxx4). The player's own library
  stays a card back after their tutor puts a card on top, and after a look. Its
  "Library" browser ("9 Cards") lists the cards top first in a row, three
  unknown ones as backs and then the known ones face up (SN9QFe2kM4Y 38:16.5,
  gIhJjyoJ0_M 29:48, SswBO8aOOgc 20:15).
- **Mana spent from a pool.** It streaks from its player's pool into the spell
  or ability it pays for, as from a land. No floating mana is seen.
- **Milled cards.** They fly from the library to the graveyard face up, one
  after another as draws do. None is held up large first. No mill is seen.
- **Mutating over or under.** As a mutating creature spell resolves, its
  controller is asked as a top-or-bottom choice is, under the spell's name:
  "Over or Under?" with OVER and UNDER. The recordings show only the opponent
  mutating, never the choice. A card under a merged permanent is in the merged
  zone and is not drawn on the table.
- **Notes on manual cards.** A card shows its table note on one line, with an
  ellipsis when it runs out of room. Its inspection panel shows the full note.
- **Other numbers.** A changed power and toughness, and mana left in a pool,
  count as a life total does. Neither is seen.
- **Paying with permanents.** Waterbend, convoke, crew. Start with no payment
  highlights. Hover over Auto Pay on desktop or long-press it on mobile to see
  which permanents Beryl would use. Click or tap the button to accept. To pay
  manually, click or tap each permanent you want to contribute. For a
  three-creature Convoke, pick A, B and D directly even if Auto Pay would use A,
  B and C. After each manual pick, recalculate the proposal for the remaining
  cost and keep the player's picks. Let the player finish manually or accept
  Auto Pay for the remainder, and stop taking picks once the cost is covered. A
  question whose yes pays a cost at once (pay {1}, or the spell is countered)
  shows what that button would tap the same way, on hover or a long press, and
  nothing before. Arena documents the preview and acceptance gestures in its
  [May 23, 2019 patch notes](https://devtrackers.gg/magic-arena/p/3e5941f1-may-23-0-15-00-00-patch-notes).
  A selected permanent turns and carries a tap mark, and an outline means one
  thing at a time: at rest every permanent that can pay wears the outline of
  what may be picked, and while Auto Pay shows its proposal those it would tap
  alone show, in their ring. Once a pick has paid part of the cost, the prompt
  says after what it asks how much is still to pay (by-1y4, by-uld, by-4hjm,
  by-z973, by-szrn, by-uq8, by-x45, by-hmfq, by-tyf).
- **Permanents entering together.** When permanents a player controls enter the
  battlefield at the same time and the order of their timestamps could change
  what their abilities do, the player picks them one at a time, earliest first,
  in the ordinary pick prompt: "Choose which enters first." Arena orders them
  without asking, as far as the recordings show.
- **Phased-out permanents.** A phased-out permanent stays where it lay, faded.
  It is never offered as a candidate, and its inspection panel says "Phased
  out".
- **Shared piles.** A shared-deck table draws one shared library, one shared
  graveyard and one shared exile pile with the same card look, tilt and scale as
  per-player piles, placed from each profile's `shared_piles` data in one column
  at the table's left edge on every layout: the library on the seam between the
  two players' creature rows, the graveyard above it and the exile pile below
  it. The battlefield rows beside the column start right of it, and the phone's
  prompt pill and phase icon keep clear of it. The per-player pile places stay
  empty.
- **Text-changed cards.** A spell or permanent whose text or type line was
  changed by a text-changing effect shows a "Text changed" tag on the table,
  renders Beryl's card frame with its new type line and rules text on the table
  and in the zoom view, and renders any ability it puts on the stack with its
  changed text on the ability plate.
- **The opponent's draws and discards.** A card the opponent draws flies face
  down from their library to their hand, as long as any card flies and never
  spinning round on the way. One they discard flies to their graveyard and turns
  face up on the way, as their spells do. Only the player's own are seen.
- **The phone.** It plays what the desktop plays; its layout and gestures are
  Beryl's own. Every motion is one of the desktop's verbs between the phone's
  places. A card flies between your hand's fan, the stack in the middle band and
  its row; the opponent's leaves and joins the card backs at the top edge. A
  draw leaves the library's pile in its player's strip, and a card put into a
  graveyard lands on that pile. An ability lifts out of its source to the stack,
  and a token appears in its row. The rows make room as the middle band grows
  for the stack, so a source glides aside while its ability lifts. An exiled
  card rests unseen under its owner's graveyard and is listed in that
  graveyard's sheet under exile's name, while one held by a permanent remains
  tucked under it. An emblem stands after its controller's other permanents, and
  its ability lifts from that card. Crowded rows overlap rather than shrinking
  creature cards below 56px high. A hand too large for one row takes further
  rows above it, to the right of its player's piles, which stay in view. Its
  newest cards stand in those rows. The phone centers a board at most 430px wide
  and scales a 375px board to fit narrower screens. Sounds follow the same
  steps. What the desktop shows around its table is composed for the phone from
  the same pieces. On the phone's start screen, the opponent list scrolls
  between the sound and credits controls at the top and the seed, the Fair deal
  checkbox and the game buttons at the bottom. The game result shows over the
  board with its reason and any culprit card centered, and the full-width result
  buttons stay above the bottom safe area when the result is shown or put aside.
  The opening shows the coin above two centered rows of cards, with bottom picks
  in slots beneath the hand and full-width actions above the bottom safe area.
  The menu button opens the settings with the log and the bug report in them.
  Undo and full control's mark stand at the end of the middle band, across from
  the phase. Unspent mana lies beside its player's life total, and the combat
  damage on its way on the total's rim. A touch on a graveyard, or on a library
  with a known card, lists its cards; a touch on a card shows it whole, and a
  touch beside it puts it away. The turn's banner crosses the middle band, a
  combat step's lies under the opponent's strip, and a scry's pill beside its
  player's lands, past the end of their row; a coin is tossed just above the
  middle band. A prompt picked on the table is the desktop's: the button it puts
  forward stands in the player's strip where the turn button is, with the one
  that passes on beside it; what it asks is a pill on the middle band's line, as
  Arena writes it across the middle of the table, or at the nearest height clear
  of every card where the stack lies on that line, and over the strip while the
  table is shaded under cards the prompt holds up; its other buttons are stacked
  over the turn button. A touch on a card the prompt can use uses it, and one it
  has no use for shows the card whole; a card in your hand is read by a touch
  and played by carrying it out of the hand. The Revealed Cards lie in the
  opponent's strip beside their graveyard, at a pile's size with no pill, and a
  touch opens them as it opens a pile. The bug reporter freezes the whole page
  and saves both that frame and the region the player selects. On a phone, the
  reporter opens from the menu, a touch selects the whole screen or a drag
  selects a region, the description stays above the keyboard, and a saved report
  offers Share and Done. Android's Back closes the open sheet or bug reporter
  and otherwise leaves a game in place. The phone keeps dark system chrome in
  either system appearance. The bottom actions clear the gesture bar, and the
  seed and game buttons rise above the numeric keyboard.
- **The Revealed Cards between reveals.** The pile holds what one spell or
  ability revealed, and what another reveals takes its place; a card revealed
  and then put where its face shows (a graveyard, exile, the battlefield, the
  stack) is not held up, since its flight shows it. The pile opens into the
  viewer every pile opens into. The recordings show the pile with one effect's
  cards at a time, dismissed before the next. One spell's four revealed cards
  lie in the pile together; the next reveal seen comes after the pile was
  dismissed (I3Te0m1bBFw 22:08.8, I3Te0m1bBFw 29:45.5).
- **Why a creature can't attack.** During Choose attackers, clicking or tapping
  a creature you control that cannot attack plays the refusal sound and opens
  the inspector with the reason under its face: "Tapped", "Summoning sickness",
  "Defender needs <subtype>" (reflecting text-changing effects), or "Can't
  attack".
- **Words: prompts Beryl folds.** Where Arena asks before a choice, Beryl opens
  the choice (exception 3) and words it from both screens. One discard in place
  of several: "Discard a basic land card or {most:words} cards." A look that may
  take nothing is its sentence as a question, with "Decline": "Put {most:words}
  of them into your hand?", "Exile {most:words} of them?", "Put {most:words} of
  them on top of your library?", "Put {most:words} of them on the bottom of your
  library?", "Put {most:words} of them into your graveyard?", "Put {most:words}
  of them onto the battlefield?". The other way about, Arena opens a payment
  that may be declined and Beryl asks first: "Pay {cost}." is answered "Pay
  {cost}" or "Decline". A card offered to cast is cast by its button, "Cast".
  Arena asks Discard a basic land card? and then Discard two cards.; a look that
  may take nothing is headed Choose One over a question, with Submit and
  Decline; a payment that may be declined has Decline beside Auto Pay
  (UrGalw_f35w 21:13.5, 5UHzpVvUkfI 27:36.5, IHPV6yRaFlk 28:19.5, 0VsYGs88NXA
  56:48.5).
- **Words: prompts the recordings lack.** Each is composed from the nearest
  prompt Arena shows. A discard to hand size: "Discard {most} cards." A mulligan
  of more than one card: "Return {most} Cards". Manifest dread: "Manifest one of
  them." Cards put back from a hand: "Put a card on top.", "Put {most:words}
  cards on top." A fateseal: "Top of Library", answered "BOTTOM" or "TOP" as a
  scry's piles are named. A card that goes on top or bottom with no asker to
  name: "Top or Bottom?". Looked-at cards sent to a graveyard or the
  battlefield: "Put {most:words} of them into your graveyard.", "Put
  {most:words} of them onto the battlefield." One card a filter names, looked at
  and sent somewhere Arena's recordings do not show: "Put {what} into your
  hand.", "Put {what} into your hand?", "Exile {what}.", "Put {what} on top of
  your library.", "Put {what} on the bottom of your library.", "Put {what} on
  top of your library?", "Put {what} on the bottom of your library?", "Put
  {what} into your graveyard.", "Put {what} into your graveyard?", "Put {what}
  onto the battlefield." An optional move of a card anywhere but to exile: "Put
  that card into your hand{from}?", "Put that card into your graveyard{from}?",
  "Put that card onto the battlefield{from}?", "Put that card on top of your
  library{from}?", "Put that card on the bottom of your library{from}?", "Move
  that card{from}?". A cost of no mana: "Pay {0}." The opening choices: "Play or
  draw?", answered Play or Draw, and "Keep or mulligan?" over Arena's own
  buttons. Catalogs: "Choose Creature Type", "Choose Word to Replace", "Choose
  New Word". Waterbend: "Tap permanents to Waterbend". Convoke: "Tap creatures
  to Convoke". Improvise: "Tap artifacts to Improvise". What a declared creature
  is aimed at: "Choose attack target", "Choose what to block". Piles of a split:
  "Hand", "Exile", "Battlefield". The pass button's captions for where Arena
  shows none: "To Untap", "To Upkeep", "To Main 1", "To Main 2", "To Cleanup",
  "Nothing to play" under a halt and "To Opponent's Turn" under the player's own
  End Turn. The own-target warning (exception 7) asks Arena's question less its
  end, "Do you want to target {name}?", and a spell picked on the stack reads
  "Target {name}", or the warning's question where it is the player's own. Only
  the opponent's manifest dread, fateseal and put-back are seen, and games begin
  with the first player chosen (pE4XSNi-nZc 28:43.5, pE4XSNi-nZc 34:55.0,
  pE4XSNi-nZc 27:18.0, 0VsYGs88NXA 16:58.0).
- **Words: what a card's text would say.** Arena words many prompts from the
  text of the card that asks, which a prompt's record does not carry. A look, a
  search or an optional search whose cards a filter of the asking spell or
  ability chose names one of them as the card does, as Arena does: "Search for
  {what}.", "Search your library for {what}?", where {what} reads, for example,
  an artifact card with mana value 1 or less. Otherwise Beryl says what Arena
  says of any card, or the same sentence less the card's part: "Choose a
  target.", "Search for a card.", "Search your library?", "Exile a card.",
  "Exile {most:words} cards.", "Distribute damage.", "Distribute counters.",
  "Choose One" over a card chosen for an effect, and an optional effect under
  its asker's name ("{source}", or "Take Action?" with none). Braces are the
  table's holes. Arena names the card's own: Target a creature., Search for a
  basic land card., Exile a blue card., Draw a card? (0VsYGs88NXA 25:33.5,
  I3Te0m1bBFw 36:34.0, TbrYIs2juCw 32:00.5, XYkigNjhINU 1:17:11.0).
- **Your own spell returned to hand.** It glides from the stack to the hand,
  turning as it goes. Only the opponent's spells are seen returned (by-e0l).

## Verbs

Every change on the table is shown by one of these, applied to a card, an
ability or a player by a step of the script the session sends
(`engine/crates/beryl_protocol/src/script`, K23). The list is closed: what Arena
does not show is composed from these. A verb's duration, delay, stagger and
curve, and each case's, are the values below. The session reads them to time
each step of the script (`beryl_protocol/src/script/timing.rs`). the client
plays that timing. `just client check` checks the motion in a recording, and
`data/arena-motion.json` holds Arena's measured instances (K27).

- **fly.** A card travels from where it is to its new place, and flashes as it
  lands on the battlefield, unless it is a land played from a hand: a land
  played, a spell cast, a permanent spell resolving, a draw, a discard, an aura
  onto its permanent. One whose face changes turns over on the way, top over
  bottom and seen in depth: a spell begins to tip away as it clears the hand, is
  edge-on about two fifths of the way along, leaning a little off level, lies
  flat with its face up about a sixth of a second after it began to tip, and
  flies the rest of its way face up, a drawn card turns over as it leaves the
  library and flies face up, and one that lay the other way up the table never
  spins round besides. A card you draw flies from the library across the table
  to a place beside the stack, where one drawn alone is held up large to be
  read, sinking a little, and then drops into the right end of the hand; each of
  several drawn together sweeps past that place without stopping and comes down
  into the hand, which has made room for them all as the first left. A card that
  comes to a hand joins its right end, and keeps its place there. The cards that
  stay in a hand as one joins or leaves it move to their new places in about a
  sixth of a second, slowing into them, as the change happens: as a drawn card
  leaves the library, and as a card cast or played leaves the hand. A card that
  lands face down the other way up the table, as the opponent's draw does,
  leaves the way up it will lie. A spell cast leaves the straight line in a
  shallow arc that bows toward its controller's edge of the table. A permanent
  resolving, either player's, is thrown up off the stack: it is smaller in its
  first frame, rises up the screen at once, crosses most of the way to its place
  at that height, half way across after a quarter of its time, and then drops
  onto its place, shrinking as it comes down. The player's own land glides
  straight to its place. The opponent's land leaves its slot face up, leans as
  it swings down and right past their life badge, then levels out and glides
  back into the land row. Its shadow separates during the swing and fades as the
  card settles. A permanent waits on the stack until the flare of the mana that
  paid for it has gone, and then leaves. A played land glides from where it is
  released to its slot in about 0.3 s, gathering speed, and lands with no flash.
  The opponent's land leaves their hand face up, leans as it moves down and
  right, and reaches about two thirds of the table's width and three tenths of
  its height after a quarter second. It levels out, and glides left and up into
  its slot about 0.6 s after it left, with no flash. The opponent's spell leaves
  their hand as a card back and is face up when it reaches the stack, about 0.4
  s; the player's own flies from where it is released in about 0.35 s. A
  resolving permanent, either player's, is the smaller battlefield card one
  frame after it leaves the stack, rises up the screen by about a fifth of the
  table's height within 0.1 s, crosses to above its slot at that height and
  drops onto it, about 0.5 s in all, and the slot flashes. A drawn card leaves
  the library at the lower left leaning a little, turns face up within about 0.2
  s at the size it had there, flies right across the table to a place beside the
  stack growing as it comes, holds there about 0.2 s and drops into the right
  end of the hand, about 1.1 s in all; three drawn by one spell leave a quarter
  second apart and fly the same way with no hold, about 0.7 s each. A discard
  drops to the graveyard at hand size in about 0.2 s (UrGalw_f35w 16:14.8,
  UrGalw_f35w 16:35.37, UrGalw_f35w 16:46.1, pE4XSNi-nZc 20:13.2, AhTcvQ-lQQQ
  23:51.50, UrGalw_f35w 16:46.3, UrGalw_f35w 16:47.15, pE4XSNi-nZc 22:00.0,
  pE4XSNi-nZc 23:23.5, UrGalw_f35w 17:01.3, UrGalw_f35w 16:15.8, WQiYBv23buw
  29:14.3, vPdoSA5H5xU 17:46.2, 3sEoP_s4DQE 39:04.3, 5UHzpVvUkfI 1:01:50.3,
  UrGalw_f35w 17:01.03). Beryl: 475 ms, 326 ms apart, ease-out. Effects: settle
  as it ends (into the battlefield) (but not land played, opponent land played).
- **appear.** A card comes to be at its place with no flight: a token, the
  opening hand, a card a look put in the graveyard, a permanent put into a
  library. Its row makes room in its delay, so it shows at a place already open.
  A token appears whole with a short glow, a batch as one group. The opening
  hand and a mulligan's new hand are there at once. A permanent put into a
  library is on the library face up within a frame, with nothing left where it
  was, and slips in 70 to 130 ms later (UrGalw_f35w 17:38.4, WQiYBv23buw
  23:57.0, UrGalw_f35w 15:48, 0VsYGs88NXA 54:16.6, iRr-GeSF0lI 21:23.9,
  TbrYIs2juCw 1:09:04.55, jD7VqaT8VC8 23:05.50). Beryl: 66 ms, 135 ms after its
  cause, 33 ms apart, in place. Effects: glow as it begins (token).
- **vanish.** A card or an ability goes from where it is with no flight, and a
  card is at its new place as it is gone: destroyed, exiled, sacrificed,
  countered, returned to a hand, or done resolving. Its step is its going alone;
  what it left room for closes up after, as one move even where the going is a
  cut, and where nothing does the next thing follows at once. A destroyed
  permanent falls to ash in about 1 s, an exiled one becomes a vortex, a
  sacrificed one flashes, and a returned one glows and is gone; none flies to a
  pile or a hand. A countered spell crumbles on the stack. A resolved instant
  darkens and dissolves there, or shrinks into its graveyard. A finished ability
  is gone between two frames (RwKoGGUmw2k 27:36.4, IEXcjBn3BXk 22:25.4,
  5UHzpVvUkfI 26:06.7, UrGalw_f35w 19:28.5, 3sEoP_s4DQE 24:49.4, I3Te0m1bBFw
  39:50.0, WQiYBv23buw 29:15.67, pE4XSNi-nZc 22:01.3, IHPV6yRaFlk 39:46.9).
  Beryl: 395 ms, 260 ms after its cause, in place. Effects: dissolve as it
  begins, dim as it begins (resolved instant or sorcery), blanch as it begins
  (exiled), glow as it begins (bounced).
- **emerge.** An ability lifts out of its source and goes on the stack: a small
  copy appears in the middle of its source and travels all but straight to the
  stack, sagging a little toward the middle of the table, and grows as it goes.
  The source stays as it is. A triggered ability shoots a beam of light toward
  the middle of the table about 0.1 s after it comes to rest, and the line to
  its source appears whole once the beam has gone. A small copy of the card
  appears in the middle of the source, about a quarter of its size on the stack,
  and travels to the stack, growing evenly with the way it has come, in 0.25 to
  0.35 s; an activated ability does the same. Filmed whole, the way is all but
  straight: it sags at most 0.025 of its length, toward the middle of the table,
  from either seat. A triggered ability flares white at the stack about 0.1 s
  after it comes to rest, for about 0.27 s, while gold sparks hang along the way
  it came. Two triggers of one event leave 0.3 s apart. A dies trigger leaves
  before its source breaks up (IEXcjBn3BXk 20:39.3, 5UHzpVvUkfI 19:48.5,
  5UHzpVvUkfI 19:58.25, krskVfn2v5Q 46:07.2, WQiYBv23buw 49:08.0, I3Te0m1bBFw
  26:56.75, 0VsYGs88NXA 19:39.5, 2VW6ZzmS8U4 32:15.3). Beryl: 300 ms, 190 ms
  after its cause, 300 ms apart, ease-in. Effects: beam as it ends (triggered
  ability) 100 ms later.
- **flip.** A card turns over where it lies: a double-faced card transforms, a
  face-down permanent turns face up (not seen: built as "A card turning face up"
  says). A transforming card is covered by an effect (a lava slab, a cocoon) for
  about 1.1 s and shows its new face as the effect clears; it neither turns nor
  grows (WCknTZNbraE 35:49.2, WCknTZNbraE 1:23:36.8, UrGalw_f35w 49:06.5).
  Beryl: 1130 ms, in place. Effects: cover as it begins.
- **tap.** A permanent turns a quarter as it taps or untaps, its whole card
  about its own middle where it lies. Its row then closes up or makes room, the
  tapped apart from the untapped. Arena tilts a tapped permanent's art crop 7 to
  8 degrees in about 0.2 s and dims it; an untap restores it in about 0.1 s
  (IEXcjBn3BXk 19:35.3, UrGalw_f35w 19:49.1, 0VsYGs88NXA 31:13.15). Exempt:
  exception 2.
- **strike.** A creature rears back, grows a little toward the eye and leans
  toward the side of what it deals combat damage to, hangs there a moment, then
  dashes onto it in a few frames; what it strikes flashes and the whole table
  shakes as the blow lands; it stays on it a moment and drifts back, leaning
  less as it goes. A player's life total changes on that blow. Two creatures
  that strike each other meet half way, at one time. A strike that would pass
  over a creature of an earlier strike still on its way out or back waits until
  that one is home. On something much smaller than itself it lands whole,
  shrinking as it goes. Each attacker rears back about half a card and leans
  about 15 degrees in about 0.1 s, hangs there about 0.3 s, dashes onto the
  player's avatar or its blocker in about 0.07 s, the table shakes for about
  0.17 s, and it stays a moment and drifts back, 1.2 to 1.4 s in all. A blocked
  attacker strikes about 0.4 s before an unblocked one. First strike is a strike
  of its own, seconds earlier (IEXcjBn3BXk 22:24.9, IEXcjBn3BXk 22:25.4,
  IEXcjBn3BXk 25:55.6, IEXcjBn3BXk 35:35.5, sMpAJ0HKDdg 54:54.0, sMpAJ0HKDdg
  54:57.5). Beryl: 1250 ms, 285 ms apart. Effects: impact as its blow lands (on
  what it strikes), shake as its blow lands (on what it strikes), trail as it
  dashes (on what it leaves).
- **streak.** A streak of light leaves one thing and lands on another, which
  flashes: mana from its source into the spell or ability it pays for, and
  damage from a spell or ability to what it hits. It rises out of its source's
  row toward the middle of the table on a low arc, a bright head with a tail
  behind it, a hot core down its middle and sparks shed behind its head,
  gathering as it leaves and quickening all the way, and its tail is drawn in as
  it lands. Mana leaves as its source begins to turn, several into one spell
  together, and has left the row before the row closes up; the mana of a pile of
  lands fans out to either side. A source with no place left on the table sends
  none: what its mana pays for only flashes. Each tapped land sends one streak
  of its color to the spell on the stack in about 0.3 s. A burn spell or ability
  glows, and a streak of fire crosses to its target in about 0.25 s, with a
  burst where it lands; the spell leaves after (TbrYIs2juCw 33:32.45,
  3sEoP_s4DQE 45:30.85, pE4XSNi-nZc 22:00.6, IEXcjBn3BXk 20:40.88, IEXcjBn3BXk
  38:36.7, I3Te0m1bBFw 31:37.8). Beryl: 250 ms, 100 ms after its cause, ease-in.
  Effects: mana-birth as it begins (mana) (on what it leaves), mana-hit as it
  ends (mana) (on what it strikes), glow as it begins (damage) (on what it
  leaves), impact as it ends (damage) (on what it strikes).
- **count.** A number changes and shows that it did: a life total, a kind of
  counter, a power and toughness, a mana pool. A life total jumps to its new
  value in a burst of about 0.4 s; a loss's red number drifts down from the
  opponent's avatar, and a gain's green one rises for about 0.5 s. A counter's
  badge settles onto its permanent after a flash. A planeswalker's loyalty
  changes about a second after its ability reaches the stack (IEXcjBn3BXk
  22:25.7, IEXcjBn3BXk 25:55.6, sMpAJ0HKDdg 54:54.5, 0VsYGs88NXA 31:13.25,
  5UHzpVvUkfI 19:59.25). Beryl: 400 ms, 125 ms after its cause, ease-out.
  Effects: pulse as it begins (but not life loss, life gain, counter add,
  counter add negative, counter remove, shield counter add, shield counter
  remove), counter-add as it begins (counter add), counter-add-neg as it begins
  (counter add negative), counter-remove as it begins (counter remove),
  shield-add as it begins (shield counter add), shield-remove as it begins
  (shield counter remove), loss as it begins (life loss), gain as it begins
  (life gain).
- **set.** The table changes and nothing moves: a step of the turn begins, a
  library is shuffled, a permanent's characteristics change. Nothing animates as
  a step begins, and nothing shows on a library when it is shuffled (IEXcjBn3BXk
  19:36.5, IEXcjBn3BXk 28:10.5). Beryl: 25 ms, in place.
- **showcase.** A card an effect reveals is held up for both players to read:
  its face joins the "Revealed Cards" pile at the top left, growing into its
  place there, and the card itself lies where it was. The face it holds up is on
  screen beside its card: the duplicate audit allows it. Revealed cards gather
  under a "Revealed Cards" pill at the top left, each growing into place in
  about 0.2 s. A click on the pile opens a browser of them under the same words,
  with a Done button, and the pile stays afterward until the cross on its pill
  puts it away (I3Te0m1bBFw 22:08.5, I3Te0m1bBFw 22:09.3, I3Te0m1bBFw 29:42.9,
  I3Te0m1bBFw 29:46.5). Beryl: 200 ms, ease-out.
- **dwell.** Nothing moves while something is read: the opponent's spell or
  ability, before it resolves unanswered, and the table as the game ends, before
  the result covers it. The opponent's spell sits on the stack for 0.7 to 1.9 s
  before it resolves. After the play that ends a game the table stays in view
  about 3 s, the loser's avatar bursting for the first half of it, before the
  result covers it (pE4XSNi-nZc 22:00.4, pE4XSNi-nZc 23:22.5, IEXcjBn3BXk
  38:37.0). Beryl: 800 ms, in place.
- **announce.** A notice passes over the table: the turn banner, the pill that
  says what a scry did, the pill beside a library that names the end of it the
  opponent chose for a card, a coin tossed. Play goes on under the banner and
  the pill, which are drawn over the cards in flight and fade in, stand and fade
  out, the banner sinking toward its player as it goes. A coin is waited for,
  and has the table to itself: it is thrown only once the turn's banner has
  gone, mid-table, rises a diameter and a half toward the viewer turning fast,
  falls back onto its plate quickening, lands with a swell and a flare, and only
  then says who won, which stands for the rest of its step. "Your Turn" or
  "Opponent's Turn" glows at the centre for about a second while cards move
  under it. A pill above an avatar reads "Scry: 1 top, 1 bottom." for about 3 s.
  A coin a card flips is tossed mid-table and lands in about 1.2 s (IEXcjBn3BXk
  19:35.7, WCknTZNbraE 16:39.7, 0VsYGs88NXA 41:27.3, IHPV6yRaFlk 52:54.6,
  SN9QFe2kM4Y 14:52.7, WCknTZNbraE 16:42.0). Beryl: 1200 ms, 116 ms after its
  cause, in place.
- **point.** An arrow or line between two things, from the step that begins it
  to the step that ends it: to a target, from an ability to its source, from an
  attacker to the planeswalker it attacks, from a blocker to its attacker. An
  attacker at a player has no line, only its wedge. Each is drawn as light, a
  ribbon with a hot core. An arrow at a target is a bowed arc, a thread at its
  spell that swells to a spear head, whole in the frame it begins once its spell
  or ability has landed on the stack, which lands with a flash and sparks and
  has light run along it toward its target while it stands. It comes onto a card
  from the open table in front of it, never along its row, and its head lies on
  the card's face; at a player it goes over the top onto their life total;
  between two things that all but touch it goes round their side. One at the
  stack item its own lies over leaves the side of its own item and comes round
  into the end of the other's strip, its head on the table beside the stack,
  pointing at that item's name. The line to a source has no head and appears
  whole in a frame, about 0.45 s after its ability leaves its source: it leaves
  the bottom of the source as a hairline, hangs low over the table, where it is
  widest, and comes up toward the bottom of its ability, fading out before it
  reaches it, from either seat. It hangs to about 0.9 of the table's height
  between ends far apart across it, and only a little below the lower end
  between ends close together across it, about a third as far below it as they
  lie apart (by-olby); to a source that lies right under its ability it goes
  down from the bottom of the ability into the top of the source, and to one
  above it whose hanging line would cross the ability (a planeswalker on the
  opponent's row over the stack) it goes up from the top of the ability into the
  bottom of the source, never over the ability's face (by-52mf); a phone's arcs
  across between them. A line of combat is a straight beam that narrows to a
  dart, from the middle of its creature to the middle of what it points at, over
  both. Each is one line for as long as it lasts, drawn every moment from where
  its two ends are, and each is cut whole in a frame as the step that ends it
  ends, never drained away from its tail, which left a stub of arrow on its
  target (by-2e5a), which waits until it has stood whole for its end's delay, so
  that one whose spell resolves at once is still read, and which what follows
  waits for, so that neither end moves under it. An ability and the line to its
  source stay a moment after what the ability did, and go in one tick. Every
  arrow and line is over everything at rest and under what flies. A browser with
  no WebGL draws each as a path instead. A cyan arc runs from a spell or ability
  to its target until it resolves. A thin line joins an ability to its source
  from the frame it lifts. A thin gold line runs from a blocker to its attacker
  until combat damage is done. An attacker has no line to the player, only its
  chevron (I3Te0m1bBFw 31:35.05, bJQDi6NIrZA 55:08.5, IEXcjBn3BXk 20:39.3,
  IEXcjBn3BXk 22:21.5, sMpAJ0HKDdg 54:50, IEXcjBn3BXk 25:52.5). Beryl: 66 ms, 0
  ms after its cause, in place.
- **mark.** A mark from the list below on a card, an ability or a player, from
  the step that begins it to the step that ends it. The light of what may be
  picked comes up over a moment, on all of it at once. Every other mark is there
  in the tick its step begins, so that one which takes another's place on a card
  swaps with it, and each is gone as the step that ends it begins. Beryl: 0 ms,
  0 ms after its cause, 0 ms apart, in place.
- **lift.** Cards the opponent is handling come away from their pile, face down,
  from the step that begins the look to the step that ends it. Cards the
  player's own prompt holds up come away from where they lie into its tray, face
  up, as the prompt opens; lie as each click sorts them; and go back to where
  the table has them as the first steps of what the answer did begin. Cards the
  opponent scries come off their library face down at its size and lie beside it
  (IHPV6yRaFlk 52:54.6, qF4GF-vfc8Y 52:59.9). Beryl: 433 ms, 500 ms apart,
  ease-out.

## Effects

An effect is light or a change of surface on one card or one player's life
total, begun by a step of a verb as it starts, as it ends or as its blow lands
and played on the client's script clock. Effects leave card placement and script
timing alone. The list is closed. Each effect has one look defined under
`data/client/effects/`, and a verb names the effects its steps begin. A mark
says what a player may do; an effect says what just happened, and is gone.

- **glow.** A halo of warm light around a card, bright at once, that spreads and
  fades: the card is new here, is leaving whole, or sends a streak. On the
  stack, where the spell that acts on a card lies over it, the light is a rim
  inside the card's own frame. Drawn under its card. Beryl: 300 ms, ease-out.
- **settle.** A permanent that lands flashes warm yellow-white with starburst
  rays (`MAT_CollisionFlash`), spreads a soft warm dust cloud and a wispy
  shockwave ring outward from under the card, and casts a dark ground shadow
  beneath it, scaled up for higher mana values (small up to 3, medium 4–5, large
  6+ with scrolling cloud and polar-distorted shockwave); flying creatures add
  wind-swirl wisps, and mythic rares add a wide soft glow, sharp starburst
  diamond flare, smoldering edge residue, and dual expanding rings tinted by
  `subject_colors` (`VFX_ETB_Mythic_Tier2_LUT`, `AhTcvQ-lQQQ` 29:13.5,
  `UrGalw_f35w` 17:34). Drawn over its card. Beryl: 470 ms, linear.
- **dissolve.** A card darkens, holds dark, and then breaks up along a stippled
  edge from the top down, for as long as its step: it is destroyed, countered or
  sacrificed. Drawn over its card. Beryl: linear.
- **dim.** A card darkens whole and fades where it lies, for as long as its
  step: a spell that has resolved is done. Drawn over its card. Beryl: linear.
- **blanch.** A card is lost in white light, for as long as its step: it is
  going to exile. Drawn over its card. Beryl: linear.
- **cover.** A veil of light closes over a card, hides it while it changes, and
  clears from what it now is, for as long as its step. Drawn over its card.
  Beryl: linear.
- **impact.** A burst of hot light on what a creature strikes or a streak of
  damage hits, a card or a player's life total: white-hot with rays and about
  one and a half of its widths across as the blow lands, spreading a little as
  it fades in about a third of a second, a ring leaving it. Drawn over its card.
  Beryl: 300 ms, ease-out.
- **shake.** The whole table jolts side to side and a little up and down, twice,
  each swing smaller than the last, as a creature's blow lands. Drawn over its
  card. Beryl: 170 ms, linear.
- **trail.** A streak of white light behind a creature as it dashes at what it
  strikes, widest at the card and thinning behind it, about as long as the card
  goes in a few frames, gone as the blow lands. Drawn over its card. Beryl: 150
  ms, linear.
- **flash.** A flare of white light with rays over a spell or ability as the
  mana that pays for it lands, brightest at once, that spreads to about twice
  the card's width and is gone. Drawn over its card. Beryl: 330 ms, linear.
- **mana-birth.** As mana leaves a tapped land or other source, light in the
  mana's color swirls round a hot white point over it, with a dark glow under
  it, a thin ring that runs outward and white sparks thrown out from the middle,
  and is gone in about 0.6 s. A land of two colors glows in the color of the
  mana it gives. In Baseline, a glow in the mana's color with a white middle and
  a ring that spreads. Drawn over its card. Each tapped land bursts into a swirl
  of its mana's color round the tap symbol, with sparks and a dark under-glow,
  as its streak rises out of it (3sEoP_s4DQE 45:30.85, TbrYIs2juCw 33:32.45).
  Beryl: 600 ms, linear.
- **mana-hit.** As mana lands on the spell or ability it pays for, a white
  four-point flare with long side rays flashes over it, light and wisps in the
  mana's color spread across it, and sparks of that color scatter outward, all
  gone in about 0.45 s. In Baseline, a white flare with rays and a glow in the
  mana's color. Drawn over its card. Where a land's streak meets the spell on
  the stack, a white star flare with side rays flashes and colored glints
  scatter over the card (3sEoP_s4DQE 45:31.2, TbrYIs2juCw 33:32.85). Beryl: 450
  ms, linear.
- **beam.** A cone of white and yellow light shoots from the side of a triggered
  ability that has come to rest on the stack, toward the middle of the table,
  from a little above its middle: it reaches nearly two cards' widths in a
  quarter of its time, narrows to a streak and fades, with a bloom on the card's
  edge where it leaves and gold sparks hanging along the way behind it. Drawn
  over its card. Beryl: 300 ms, linear.
- **pulse.** A ring of light round a card's frame while its numbers swell and
  settle: a counter put on it or taken off, its loyalty, its power and
  toughness. Drawn over its card. Beryl: 320 ms, ease-out.
- **loss.** A life total flares red, and the number that says how much it lost
  sinks away from it and fades, for as long as its step. Drawn over its card.
  Beryl: linear.
- **gain.** A life total flares green, and the number that says how much it
  gained rises from it and fades, for as long as its step. Drawn over its card.
  Beryl: linear.
- **counter-add.** A white-cyan vertical flare and hot core flash on the counter
  badge, followed by an expanding shockwave ring and wispy energy smoke across
  the card when a positive or standard counter is added (2VW6ZzmS8U4 20:29;
  PS_Counter_Spawn_Sm_KH). Drawn over its card. Beryl: 520 ms, linear.
- **counter-add-neg.** A harsher crimson-violet cross-flare, tight shockwave
  ring, and dark smoky wisps across the card when a harmful (-1/-1 style)
  counter is added (0VsYGs88NXA 31:09.5; vfx_counter_spawn_neg). Drawn over its
  card. Beryl: 500 ms, linear.
- **counter-remove.** A flash and expanding shockwave ring on the counter badge
  with outward sparks, wispy smoke, and tumbling shatter pieces when a counter
  is removed (vfx_counter_removal). Drawn over its card. Beryl: 560 ms, linear.
- **shield-add.** A diamond flash, teal-cyan energy shockwave, sparks, and
  glowing shield emblem on the badge with a subtle teal card-rim pulse when a
  shield counter is added (VFX_SNC_Shield_Counter_Add). Drawn over its card.
  Beryl: 620 ms, linear.
- **shield-remove.** Ice-white crackle across a deep steel-blue shield emblem on
  the badge that splits into three flying shards and bits when a shield counter
  is removed (VFX_SNC_Shield_Counter_Break). Drawn over its card. Beryl: 600 ms,
  linear.

## Marks

A mark is the only highlight a card, an ability or a player carries, in a script
or in a prompt (K26). The list is closed, and each mark has one look. What a
card's own state says (tapped, summoning sick, counters, attacking) is its face,
not a mark.

- **eligible.** The player may pick or use it now: a card to play, a permanent
  with an ability, a legal target, an attacker or blocker to declare, an option.
  A cyan outline that shimmers with a slow scrolling flare and pops outward once
  over 0.4 s when it first appears (Enhanced procedural shimmer and pop;
  Baseline animated outline tint). Arena outlines all of these in one shimmering
  cyan-mint outline with a one-shot 0.4 s outward pop on appear, and neither
  dims nor outlines what cannot be picked (IEXcjBn3BXk 17:59.5, IEXcjBn3BXk
  19:43.5, UrGalw_f35w 26:55.0, IEXcjBn3BXk 22:20.5, UrGalw_f35w 19:51.0).
- **selected.** The player has picked it in the open prompt. A gold outline. A
  clicked attacker and a chosen blocker turn gold (SswBO8aOOgc 33:30.5,
  UrGalw_f35w 42:50.0).
- **suggested.** The offered default would use it: a permanent Auto Pay would
  tap, the blocker the session would deal lethal damage to next. An ember ring.
  Not seen in Arena.
- **paying.** Picked to pay a cost. The permanent turns and carries a tap mark,
  with no outline. Not seen in Arena.
- **friendly-target.** A target its chooser controls. A green outline, by
  exception 4.
- **hostile-target.** A target its chooser does not control. A red outline, by
  exception 4.
