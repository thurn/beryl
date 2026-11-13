# UI Conventions

MTG Arena is the reference for Beryl's flow, the order and timing of events,
table layout and wording (AGENTS.md, UI; [Project 50k](project-50k.md), K22).
This page lists only what Arena does not settle: where Beryl differs on purpose,
what Arena has no equivalent for, and what the recordings do not show.

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
   taps. Arena shows a crop of the art.
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

## Settled as Arena

Arena's way stands where an earlier report asked for something else, so do not
restore the old behavior.

- **Tokens.** They appear at their slot; they do not fly out of their source
  (by-kgc, by-81h). Arena's appears whole with a short glow, about 0.1 s after
  its ability leaves the stack; a batch appears as one group (UrGalw_f35w
  17:38.4, WQiYBv23buw 23:57.0, iRr-GeSF0lI 21:58.3).
- **Permanents that leave.** A permanent that is not a token dissolves where it
  is: destroyed, it breaks into ash at its slot; exiled, it becomes a vortex
  there. Nothing flies to the pile. What it leaves room for closes up only once
  it has gone (by-1ug). A destroyed creature glows and falls to ash over about 1
  s; the spell then goes to its graveyard. The row closes up after the permanent
  is gone (RwKoGGUmw2k 27:36.4, bJQDi6NIrZA 51:46, TbrYIs2juCw 30:37,
  5UHzpVvUkfI 58:17.0, 3sEoP_s4DQE 24:49.6).
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
- **A finished ability.** It is removed at once, with no fade (by-c8b).
- **Wording.** Arena's exactly, case and punctuation too: "All Attack", "Choose
  attackers." Every prompt's title and buttons are one table in the session
  (`rust/crates/beryl_session/src/prompt/wording.rs`), each entry beside the
  recording moments that show it. A browser's title is the line that asks: its
  sentence of rules text, or its heading where the line under it only says how
  to drag. A pick is confirmed by "Submit" and the number picked, an arrangement
  by "Done", a "may" by "Take Action" or "Decline" (by-u4g, by-a09o).
- **Button order.** It follows Arena prompt by prompt; there is no blanket
  "default on the right" (by-ceo3).
- **The opponent's turn.** Arena's "Opponent's Turn" label shows in the action
  button's place, and no spinner (by-58n, by-7ih, by-oyx).
- **Attacking a planeswalker.** No prompt: an attacker defaults to the player,
  and the player pulls an arrow to the planeswalker (by-p00u). Attackers that
  can attack get a cyan outline; a clicked attacker turns gold, the opponent's
  life total and the planeswalker glow, and an arrow follows the cursor to the
  planeswalker (SswBO8aOOgc 33:30.5).
- **Two items on the stack.** The top one still gets its target arrow. Each item
  beneath peeks out to the left of the one over it by a quarter of its width (a
  fifth past five items, as far as the room left of the stack allows), so a
  spell leaving from under another is seen as it goes (by-i88). A cyan arc runs
  from the top item to its target, even when that is the one card under it; the
  top card sits a little right of and below the one beneath, which shows about a
  third of its width beside it, and a countered spell crumbles in that strip
  (I3Te0m1bBFw 39:49.4, IEXcjBn3BXk 51:34.5, IEXcjBn3BXk 51:35.3, MnhyzykajXw
  36:19.5, 5UHzpVvUkfI 58:15).
- **Additional costs.** A chooser that lowers the mana to pay (Delve) opens
  before mana taps; a sacrifice cost is chosen after the lands tap (by-jqd).
  Treasure Cruise's Delve chooser opens before any land taps; Natural Order's
  lands send their mana, then "Sacrifice a green creature." opens (5UHzpVvUkfI
  47:31.0, RwKoGGUmw2k 39:26.0, RwKoGGUmw2k 1:07:02.25).
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
- **Choosing from a list.** The entries stand in a column over the shaded table.
  A press lights an entry and "Submit 1" answers with it, where Arena takes a
  color at the press: no card asks for an entry yet, and the prompt is the one a
  longer list needs, a creature type or a card name. Mother of Runes' color:
  Choose One over Choose Color, and White, Blue, Black, Red and Green in a
  column, each taken at a click (UrGalw_f35w 45:18.5).
- **A countered spell.** It leaves before the spell that countered it (by-kxyq).
  The countered spell crumbles in place under the counterspell, which stays
  until the debris is gone and then fades, 0.5 to 0.9 s later (I3Te0m1bBFw
  39:50.0, IEXcjBn3BXk 51:35.0).
- **Cards the opponent looks at.** They come off the top of their library face
  down at library size and lie beside it; a top-bound card slides back onto the
  library and a bottom-bound one slides away from it, to either side. A pill
  above their avatar reads "Scry: 1 top, 1 bottom." for about 3 s (by-pv6p,
  by-yfb2, by-nva). No card is enlarged (IHPV6yRaFlk 52:54.6, IHPV6yRaFlk
  53:31.2, qF4GF-vfc8Y 52:59.9, IEXcjBn3BXk 33:32.3).
- **A line from an ability to its source.** It is always shown while the ability
  is on the stack, thin and faint as Arena draws it, and hidden during a prompt.
  A heavy arrow is not that line (by-wkr, by-afh, by-110h).
- **Passing to the end step.** "End Turn" passes until the end step, but stops
  when the opponent puts a spell or ability on the stack: the button reads
  "Resolve", then "Pass" once it resolves (by-sqo). The button reads "End Turn"
  over "To End", and goes blank while it passes (SswBO8aOOgc 21:00, WQiYBv23buw
  37:52, jD7VqaT8VC8 45:29).
- **Targeting a spell.** With one spell on the stack, a spell that targets a
  spell takes it with no prompt and no Confirm. With more, a "Choose One"
  browser of the stack, "Target a spell.", opens with nothing preselected,
  Submit and Cancel (by-2lwc, by-69ps). Counterspell and Mana Leak at a lone
  spell go straight onto the stack; Spell Pierce over two spells opens the
  browser at "Submit 0" (vPdoSA5H5xU 21:36.5, qF4GF-vfc8Y 23:58.5, jD7VqaT8VC8
  17:54.5, SN9QFe2kM4Y 26:54.0, pE4XSNi-nZc 40:55.0).
- **Mana into what it pays for.** Each tapped land glows and sends one streak of
  its color straight to the spell on the stack, which flashes as they land, in
  about 0.3 s. Mana does not stop at a pool (by-p1j). Seen for casts paid with
  lands (TbrYIs2juCw 33:32.45, 3sEoP_s4DQE 45:30.85).
- **The local player's scry.** The table is shaded and "Scry" shows the cards
  face up over it, BOTTOM on the left and TOP on the right, and Done; surveil's
  has Graveyard and Library boxes instead. The cards are the library's own: they
  come up from it as the prompt opens, and on Done the shade lifts at once and
  they go back to it at its size, none shown large on its way to the top or the
  bottom. A pill reads "Scry: 1 top, 1 bottom." (by-8qq, by-2wn7, by-kyig,
  by-1mrh). Seen for a scry of two and for surveil; no card changes face
  (SN9QFe2kM4Y 14:50.5, SN9QFe2kM4Y 14:52.7, SN9QFe2kM4Y 13:08.5, iRr-GeSF0lI
  21:16.5).
- **A card a look puts in the graveyard.** It appears on the graveyard at pile
  size once the chooser closes; it is not shown large first (by-2ma). A
  surveilled card lands on the graveyard as the overlay closes; kept cards close
  with the overlay (iRr-GeSF0lI 21:23.9, SN9QFe2kM4Y 13:10).
- **A batch exile.** Cards exiled together leave one at a time, about 0.3 s
  apart (by-5pj). Delve's cards gather on the graveyard, then peel off its top
  one by one into a vortex. Delve is the only batch exile in the recordings, all
  in one recording (5UHzpVvUkfI 26:06.7, 5UHzpVvUkfI 47:36.2).
- **An opponent's spell returned to hand.** It breaks up in place on the stack
  over 0.5 to 1 s; it does not fly or flip toward their hand (by-e0l). Seen for
  Remand on the opponent's spells (3sEoP_s4DQE 45:32.0, 3sEoP_s4DQE 56:19.5,
  TbrYIs2juCw 36:04.5).
- **Hover inspection.** A hovered card in hand rises and grows in place to about
  twice its size. A hovered permanent gets a separate copy about three times its
  size, away from the cursor, with its keywords explained. A card on the stack
  gets none. Estimated from stills, not measured (jD7VqaT8VC8 45:27.5,
  SswBO8aOOgc 21:02.0, pE4XSNi-nZc 34:21.0, jD7VqaT8VC8 17:34.0).
- **An arrow to the pointer.** While a target is chosen, an arrow runs from the
  spell or ability that asks to the mouse, thin and pale. On something that may
  be chosen it becomes the arrow the choice will draw, in that arrow's color and
  along its way, and the click draws the stage's own. It follows the pointer
  itself, with no lag: only the swing onto a target takes time. Nothing is drawn
  over a target already chosen, and a touch pulls none (by-w8gv.21.67). As the
  prompt opens, a cyan arrow runs from the ability that asks to the creature
  under the cursor, and what may be chosen is outlined (IEXcjBn3BXk 23:13.17).

## No Arena equivalent

- **Undo.** Every answer can be undone, across turns and prompts, the cast and
  its payment together (by-2ml, by-96t, by-pwe).
- **Chrome.** Round icon buttons; the log toggle top left; the bug button above
  undo with no gap kept for a missing button (by-kib, by-8g4).
- **Why a click is refused.** An illegal click shows its reason at the permanent
  that forbids it (by-l2ez).
- **Suggestions are sensible.** The suggested untap targets and the auto-pay
  sources are the ones a player would pick (by-svq, by-ufmq).
- **Nothing overlaps.** The hand, the stack and their indicators never cover a
  control or each other. No layer of a library, graveyard or exile pile covers a
  card in play, for either seat, however long a row grows. The opponent's piles
  keep the band above their back row to themselves (by-biv, by-krxg).

## Not seen in Arena

Built from motions and shapes Beryl already has. Arena has these, but the
recordings do not show them.

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
  alone show, in their ring. The prompt says after what it asks how much is
  still to pay (by-1y4, by-uld, by-4hjm, by-z973, by-szrn, by-uq8, by-x45,
  by-hmfq, by-tyf).
- **A creature that enters attacking.** It gets an explicit choice of what it
  attacks, and its line stays drawn throughout (by-l26, by-y3sp).
- **A token that leaves.** It dissolves in place like any permanent. No token
  leaving the battlefield is seen (by-1ug).
- **Evoke and cycling.** A card with evoke, or a land with cycling, opens the
  same "Choose One" modal. No evoke is seen; Triomes cycle straight to the
  stack, but whether a land could still be played is not shown (by-lzl, by-9fb).
  Arena shows no chooser before these cycles (RwKoGGUmw2k 19:23.5, WQiYBv23buw
  45:29.5).
- **Known cards in a library.** A known top card shows face up on its library,
  and clicking a library lists its known cards. No known card in the opponent's
  library is seen (by-nva, by-9es). The player's own library stays a card back
  after their tutor puts a card on top, and after a look (gIhJjyoJ0_M 29:48,
  SswBO8aOOgc 20:15).
- **A card turning face up.** A face-down permanent turning face up enlarges in
  place and flips; one leaving reveals its face during its flight, and one that
  leaves for a hand or a library, where no face shows, joins the Revealed Cards
  as it goes. Nothing turns face up in the recordings, and one face-down card is
  seen leaving (by-5hcb, by-thod). Killed, Arena's face-down card dissolves as a
  card back and a "Revealed Cards" panel then shows its face at the top left
  (I3Te0m1bBFw 29:42.9).
- **Cards a look returns.** Cards a look puts back into a library fly straight
  to it. No look at several cards with the rest returned is filmed (by-ubgt).
- **A linked exile returning.** The exiled card returns as its source departs,
  moving with it. No exiling permanent is seen leaving with its card returning
  (by-esr2).
- **Your own spell returned to hand.** It glides from the stack to the hand,
  turning as it goes. Only the opponent's spells are seen returned (by-e0l).
- **The opponent's draws and discards.** A card the opponent draws flies face
  down from their library to their hand, as long as any card flies and never
  spinning round on the way. One they discard flies to their graveyard and turns
  face up on the way, as their spells do. Only the player's own are seen.
- **A mulligan's hand.** It goes from sight where it lies, every card of it, as
  a permanent that leaves does; then the next hand is dealt as an opening hand
  is. No mulligan is filmed.
- **Milled cards.** They fly from the library to the graveyard face up, one
  after another as draws do. None is held up large first. No mill is seen.
- **The Revealed Cards between reveals.** The pile holds what one spell or
  ability revealed, and what another reveals takes its place; a card revealed
  and then put where its face shows (a graveyard, exile, the battlefield, the
  stack) is not held up, since its flight shows it. The pile opens into the
  viewer every pile opens into. The recordings show the pile with one effect's
  cards at a time, dismissed before the next. One spell's four revealed cards
  lie in the pile together; the next reveal seen comes after the pile was
  dismissed (I3Te0m1bBFw 22:08.8, I3Te0m1bBFw 29:45.5).
- **Cards leaving a graveyard.** A card returned to the battlefield or to a hand
  flies there from its pile. None is seen.
- **An ability with no source on the battlefield.** It emerges from the pile its
  source lies in, and appears on the stack when it has no source. No trigger
  from a graveyard is seen.
- **A cost paid as a spell resolves.** Mana streaks from its sources into the
  resolving spell or ability, as into a spell being cast. No such payment is
  seen.
- **Mana spent from a pool.** It streaks from its player's pool into the spell
  or ability it pays for, as from a land. No floating mana is seen.
- **A target that moves.** An arrow or line ends before either of its ends
  moves, and an arrow is not drawn again at a card that changed zones while its
  spell or ability stays on the stack. No target is seen leaving in response
  (by-yfw, by-ad8, by-8uw, by-2wsl).
- **A copy on the stack.** It appears on the stack with no flight. None is seen.
- **A permanent changing sides.** It flies across the battlefield to its new
  controller's row. Seen only before and after its move (0VsYGs88NXA 27:39.5,
  gIhJjyoJ0_M 53:21.5).
- **Equipment.** It flies to the creature it becomes attached to. The equip
  ability leaves the stack with a flash at the creature; the equipment's own
  move is not seen (krskVfn2v5Q 46:10.45).
- **Other numbers.** A changed power and toughness, and mana left in a pool,
  count as a life total does. Neither is seen.
- **Words: what a card's text would say.** Arena words many prompts from the
  text of the card that asks, which a prompt's record does not carry. Beryl says
  what Arena says of any card, or the same sentence less the card's part:
  "Choose a target.", "Search for a card.", "Search your library?", "Exile a
  card.", "Exile {most:words} cards.", "Distribute damage.", "Distribute
  counters.", "Choose One" over a card chosen for an effect, and an optional
  effect under its asker's name ("{source}", or "Take Action?" with none).
  Braces are the table's holes. Arena names the card's own: Target a creature.,
  Search for a basic land card., Exile a blue card., Draw a card? (0VsYGs88NXA
  25:33.5, I3Te0m1bBFw 36:34.0, TbrYIs2juCw 32:00.5, XYkigNjhINU 1:17:11.0).
- **Words: prompts Beryl folds.** Where Arena asks before a choice, Beryl opens
  the choice (exception 3) and words it from both screens. One discard in place
  of several: "Discard a basic land card or {most:words} cards." A look that may
  take nothing is its sentence as a question, with "Decline": "Put {most:words}
  of them into your hand?", "Exile {most:words} of them?", "Put {most:words} of
  them into your library?", "Put {most:words} of them into your graveyard?",
  "Put {most:words} of them onto the battlefield?". The other way about, Arena
  opens a payment that may be declined and Beryl asks first: "Pay {cost}." is
  answered "Pay {cost}" or "Decline". A card offered to cast is cast by its
  button, "Cast". Arena asks Discard a basic land card? and then Discard two
  cards.; a look that may take nothing is headed Choose One over a question,
  with Submit and Decline; a payment that may be declined has Decline beside
  Auto Pay (UrGalw_f35w 21:13.5, 5UHzpVvUkfI 27:36.5, IHPV6yRaFlk 28:19.5,
  0VsYGs88NXA 56:48.5).
- **Words: prompts the recordings lack.** Each is composed from the nearest
  prompt Arena shows. A discard to hand size: "Discard {most} cards." A mulligan
  of more than one card: "Return {most} Cards". Manifest dread: "Manifest one of
  them." Cards put back from a hand: "Put a card on top.", "Put {most:words}
  cards on top." A fateseal: "Top of Library", answered "BOTTOM" or "TOP" as a
  scry's piles are named. A card that goes on top or bottom with no asker to
  name: "Top or Bottom?". Looked-at cards sent to a graveyard or the
  battlefield: "Put {most:words} of them into your graveyard.", "Put
  {most:words} of them onto the battlefield." An optional move of a card
  anywhere but to exile: "Put that card into your hand{from}?", "Put that card
  into your graveyard{from}?", "Put that card onto the battlefield{from}?", "Put
  that card into your library{from}?", "Move that card{from}?". A cost of no
  mana: "Pay {0}." The opening choices: "Play or draw?", answered Play or Draw,
  and "Keep or mulligan?" over Arena's own buttons. Catalogs: "Choose Creature
  Type", "Choose Card Name". Waterbend: "Tap permanents to Waterbend". What a
  declared creature is aimed at: "Choose attack target", "Choose what to block".
  Piles of a split: "Hand", "Exile", "Battlefield". The pass button's captions
  for where Arena shows none: "To Untap", "To Upkeep", "To Main 1", "To Main 2",
  "To Cleanup", "Nothing to play" under a halt and "To Opponent's Turn" under
  the player's own End Turn. The own-target warning (exception 7) asks Arena's
  question less its end, "Do you want to target {name}?", and a spell picked on
  the stack reads "Target {name}", or the warning's question where it is the
  player's own. Only the opponent's manifest dread, fateseal and put-back are
  seen, and games begin with the first player chosen (pE4XSNi-nZc 28:43.5,
  pE4XSNi-nZc 34:55.0, pE4XSNi-nZc 27:18.0, 0VsYGs88NXA 16:58.0).
- **The phone.** It plays what the desktop plays; its layout and gestures are
  Beryl's own. Every motion is one of the desktop's verbs between the phone's
  places. A card flies between your hand's fan, the stack in the middle band and
  its row; the opponent's leaves and joins the card backs at the top edge. A
  draw leaves the library's pile in its player's strip, and a card put into a
  graveyard lands on that pile. An ability lifts out of its source to the stack,
  and a token appears in its row. The rows make room as the middle band grows
  for the stack, so a source glides aside while its ability lifts. An exiled
  card rests unseen under its owner's graveyard, and an emblem's ability appears
  on the stack with no lift: the phone draws no exile pile and no emblem yet.
  Sounds follow the same steps. What the desktop shows around its table is
  composed for the phone from the same pieces. The menu button opens the
  settings with the log and the bug report in them. Undo and full control's mark
  stand at the end of the middle band, across from the phase. Unspent mana lies
  beside its player's life total, and the combat damage on its way on the
  total's rim. A touch on a graveyard, or on a library with a known card, lists
  its cards; a touch on a card shows it whole, and a touch beside it puts it
  away. The turn's banner crosses the middle band, a combat step's lies under
  the opponent's strip, and a scry's pill beside its player's lands, past the
  end of their row; a coin is tossed just above the middle band. A prompt picked
  on the table is the desktop's: the button it puts forward stands in the
  player's strip where the turn button is, with the one that passes on beside
  it; what it asks is a pill over that strip, and its other buttons are stacked
  over the turn button. A touch on a card the prompt can use uses it, and one it
  has no use for shows the card whole; a card in your hand is read by a touch
  and played by carrying it out of the hand. The Revealed Cards lie in the
  opponent's strip beside their graveyard, at a pile's size with no pill, and a
  touch opens them as it opens a pile.

## Verbs

Every change on the table is shown by one of these, applied to a card, an
ability or a player by a step of the script the session sends
(`rust/crates/beryl_session/src/script`, K23). The list is closed: what Arena
does not show is composed from these. A verb's duration, delay, stagger and
curve, and each case's, are the values below, which `just motion` holds the
stage to; Arena's measured instances are kept beside them as reference in
`data/arena-motion.json` (K27).

- **fly.** A card travels from where it is to its new place, and flashes as it
  lands on the battlefield, unless it is a land played from a hand: a land
  played, a spell cast, a permanent spell resolving, a draw, a discard, an aura
  onto its permanent. One whose face changes turns over on the way, top over
  bottom and seen in depth: a spell is edge-on a little past half way and lands
  face up, a drawn card turns over as it leaves the library and flies face up,
  and one that lay the other way up the table never spins round besides. A card
  you draw flies from the library across the table to a place beside the stack,
  where one drawn alone is held up large to be read, sinking a little, and then
  drops into the right end of the hand; each of several drawn together sweeps
  past that place without stopping and comes down into the hand, which has made
  room for them all as the first left. A card that comes to a hand joins its
  right end, and keeps its place there. A card that lands face down the other
  way up the table, as the opponent's draw does, leaves the way up it will lie.
  A spell cast leaves the straight line in a shallow arc that bows toward its
  controller's edge of the table. A permanent resolving, either player's, is
  thrown up off the stack: it is smaller in its first frame, rises up the screen
  at once, crosses most of the way to its place at that height, half way across
  after a quarter of its time, and then drops onto its place, shrinking as it
  comes down. The player's own land glides straight to its place. The opponent's
  land drops out of their hand into the open table on the side away from where
  it is going, tilted, and swings back along the table to its place. A permanent
  waits on the stack until the flare of the mana that paid for it has gone, and
  then leaves. A played land glides from where it is released to its slot in
  about 0.3 s, gathering speed, and lands with no flash. The opponent's land
  leaves their hand face up, drops down into the table on the side away from
  their lands, tilted, and swings back to its slot, about 0.6 s in all, with no
  flash. The opponent's spell leaves their hand as a card back and is face up
  when it reaches the stack, about 0.4 s; the player's own flies from where it
  is released in about 0.35 s. A resolving permanent, either player's, is the
  smaller battlefield card one frame after it leaves the stack, rises up the
  screen by about a fifth of the table's height within 0.1 s, crosses to above
  its slot at that height and drops onto it, about 0.5 s in all, and the slot
  flashes. A drawn card leaves the library at the lower left turning face up
  within about 0.2 s, flies right across the table to a place beside the stack
  growing as it comes, holds there about 0.2 s and drops into the right end of
  the hand, about 1.1 s in all; three drawn by one spell leave a quarter second
  apart and fly the same way with no hold, about 0.7 s each. A discard drops to
  the graveyard at hand size in about 0.2 s (UrGalw_f35w 16:14.8, UrGalw_f35w
  16:35.37, UrGalw_f35w 16:46.1, pE4XSNi-nZc 20:13.2, UrGalw_f35w 16:46.3,
  UrGalw_f35w 16:47.15, pE4XSNi-nZc 22:00.0, pE4XSNi-nZc 23:23.5, UrGalw_f35w
  17:01.3, UrGalw_f35w 16:15.8, WQiYBv23buw 29:14.3, vPdoSA5H5xU 17:46.2,
  3sEoP_s4DQE 39:04.3, 5UHzpVvUkfI 1:01:50.3, UrGalw_f35w 17:01.03). Beryl: 475
  ms, 326 ms apart, ease-out. Effects: settle as it ends (into the battlefield)
  (but not land played, opponent land played).
- **appear.** A card comes to be at its place with no flight: a token, the
  opening hand, a card a look put in the graveyard. Its row makes room in its
  delay, so it shows at a place already open. A token appears whole with a short
  glow, a batch as one group. The opening hand and a mulligan's new hand are
  there at once (UrGalw_f35w 17:38.4, WQiYBv23buw 23:57.0, UrGalw_f35w 15:48,
  0VsYGs88NXA 54:16.6, iRr-GeSF0lI 21:23.9). Beryl: 66 ms, 135 ms after its
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
  behind it, gathering as it leaves and quickening all the way, and its tail is
  drawn in as it lands. Mana leaves as its source begins to turn, several into
  one spell together, and has left the row before the row closes up; the mana of
  a pile of lands fans out to either side. A source with no place left on the
  table sends none: what its mana pays for only flashes. Each tapped land sends
  one streak of its color to the spell on the stack in about 0.3 s. A burn spell
  or ability glows, and a streak of fire crosses to its target in about 0.25 s,
  with a burst where it lands; the spell leaves after (TbrYIs2juCw 33:32.45,
  3sEoP_s4DQE 45:30.85, pE4XSNi-nZc 22:00.6, IEXcjBn3BXk 20:40.88, IEXcjBn3BXk
  38:36.7, I3Te0m1bBFw 31:37.8). Beryl: 250 ms, 100 ms after its cause, ease-in.
  Effects: glow as it begins (mana) (on what it leaves), flash as it ends (mana)
  (on what it strikes), glow as it begins (damage) (on what it leaves), impact
  as it ends (damage) (on what it strikes).
- **count.** A number changes and shows that it did: a life total, a kind of
  counter, a power and toughness, a mana pool. A life total jumps to its new
  value in a burst of about 0.4 s; a loss's red number drifts down from the
  opponent's avatar, and a gain's green one rises for about 0.5 s. A counter's
  badge settles onto its permanent after a flash. A planeswalker's loyalty
  changes about a second after its ability reaches the stack (IEXcjBn3BXk
  22:25.7, IEXcjBn3BXk 25:55.6, sMpAJ0HKDdg 54:54.5, 0VsYGs88NXA 31:13.25,
  5UHzpVvUkfI 19:59.25). Beryl: 400 ms, 125 ms after its cause, ease-out.
  Effects: pulse as it begins, loss as it begins (life loss), gain as it begins
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
  says what a scry did, a coin tossed. Play goes on under the banner and the
  pill, which are drawn over the cards in flight and fade in, stand and fade
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
  the bottom of the source as a hairline, hangs low over the table, to about 0.9
  of its height, where it is widest, and comes up toward the bottom of its
  ability, fading out before it reaches it, from either seat; a phone's arcs
  across between them. A line of combat is a straight beam that narrows to a
  dart, from the middle of its creature to the middle of what it points at, over
  both. Each is one line for as long as it lasts, drawn every moment from where
  its two ends are, and each is cut in a frame by the step that ends it, which
  waits until it has stood whole for its end's delay, so that one whose spell
  resolves at once is still read, and which what follows waits for, so that
  neither end moves under it. An ability and the line to its source stay a
  moment after what the ability did, and go in one tick. Every arrow and line is
  over everything at rest and under what flies. A browser with no WebGL draws
  each as a path instead. A cyan arc runs from a spell or ability to its target
  until it resolves. A thin line joins an ability to its source from the frame
  it lifts. A thin gold line runs from a blocker to its attacker until combat
  damage is done. An attacker has no line to the player, only its chevron
  (I3Te0m1bBFw 31:35.05, bJQDi6NIrZA 55:08.5, IEXcjBn3BXk 20:39.3, IEXcjBn3BXk
  22:21.5, sMpAJ0HKDdg 54:50, IEXcjBn3BXk 25:52.5). Beryl: 66 ms, 0 ms after its
  cause, in place.
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
and played on the stage's clock (`ui/src/stage/effects.ts`): it never moves a
card and never holds up the script. The list is closed, each effect has one look
in Beryl's skin (`ui/src/stage/stage.css`), and a verb names the effects its
steps begin. A mark says what a player may do; an effect says what just
happened, and is gone.

- **glow.** A halo of warm light around a card, bright at once, that spreads and
  fades: the card is new here, is leaving whole, or sends a streak. On the
  stack, where the spell that acts on a card lies over it, the light is a rim
  inside the card's own frame. Drawn under its card. Beryl: 300 ms, ease-out.
- **settle.** A permanent that lands is whole for two frames, then whitens over
  its whole face, bursts in a round bloom of white light with thin rays about
  one and a half cards wide, throws a soft ring to twice its width and flecks
  that drift off, and stands washed white until the light is gone. Drawn over
  its card. Beryl: 470 ms, linear.
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

## Marks

A mark is the only highlight a card, an ability or a player carries, in a script
or in a prompt (K26). The list is closed, and each mark has one look. What a
card's own state says (tapped, summoning sick, counters, attacking) is its face,
not a mark.

- **eligible.** The player may pick or use it now: a card to play, a permanent
  with an ability, a legal target, an attacker or blocker to declare, an option.
  A cyan outline. Arena outlines all of these in one cyan, and neither dims nor
  outlines what cannot be picked (IEXcjBn3BXk 17:59.5, IEXcjBn3BXk 19:43.5,
  UrGalw_f35w 26:55.0, IEXcjBn3BXk 22:20.5, UrGalw_f35w 19:51.0).
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
