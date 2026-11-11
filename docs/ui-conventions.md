# UI Conventions

MTG Arena is the reference for Beryl's flow, motion, timing, table layout and
wording (AGENTS.md, UI; [Project 50k](project-50k.md), K22). This page lists
only what Arena does not settle: where Beryl differs on purpose, what Arena has
no equivalent for, and what the recordings do not show.

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
  there. Nothing flies to the pile (by-1ug). A destroyed creature glows and
  falls to ash over about 1 s; the spell then goes to its graveyard (RwKoGGUmw2k
  27:36.4, bJQDi6NIrZA 51:46, TbrYIs2juCw 30:37, 5UHzpVvUkfI 58:17.0).
- **Choosing how to cast.** A card or permanent with more than one way to cast
  or use it opens a modal, "Choose One", showing the card once per option, with
  Cancel: an adventure, an alternative cost, a land's abilities (by-lzl,
  by-9fb). Brazen Borrower, Bonecrusher Giant and Force of Will open it from
  hand, Wasteland on the battlefield (SN9QFe2kM4Y 42:44.2, 3sEoP_s4DQE 50:44.3,
  3sEoP_s4DQE 46:50.2, MnhyzykajXw 35:43).
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
- **Two items on the stack.** The top one still gets its target arrow (by-i88).
  A cyan arc runs from the top item to its target, even when that is the one
  card under it; the top card sits a little right of and below the one beneath
  (I3Te0m1bBFw 39:49.4, IEXcjBn3BXk 51:34.5, MnhyzykajXw 36:19.5, 5UHzpVvUkfI
  58:15).
- **Additional costs.** A chooser that lowers the mana to pay (Delve) opens
  before mana taps; a sacrifice cost is chosen after the lands tap (by-jqd).
  Treasure Cruise's Delve chooser opens before any land taps; Natural Order's
  lands send their mana, then "Sacrifice a green creature." opens (5UHzpVvUkfI
  47:31.0, RwKoGGUmw2k 39:26.0, RwKoGGUmw2k 1:07:02.25).
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
- **The local player's scry.** A full-screen overlay, "Scry", shows the cards
  face up in one fan before a card back, BOTTOM on the left and TOP on the
  right, and Done; surveil's has Graveyard and Library boxes instead. On Done
  the overlay closes at once and nothing flies out of it: the cards show at the
  library at its size for a moment, and a pill reads "Scry: 1 top, 1 bottom."
  (by-8qq, by-2wn7, by-kyig, by-1mrh). Seen for a scry of two and for surveil;
  no card changes face (SN9QFe2kM4Y 14:50.5, SN9QFe2kM4Y 14:52.7, SN9QFe2kM4Y
  13:08.5, iRr-GeSF0lI 21:16.5).
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
  Auto Pay for the remainder, and stop taking picks once the cost is covered.
  Arena documents the preview and acceptance gestures in its
  [May 23, 2019 patch notes](https://devtrackers.gg/magic-arena/p/3e5941f1-may-23-0-15-00-00-patch-notes).
  A selected permanent turns and carries a tap mark, and an outline means one
  thing at a time (by-1y4, by-uld, by-4hjm, by-z973).
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
  place and flips; one leaving reveals its face during its flight. Nothing turns
  face up in the recordings, and one face-down card is seen leaving (by-5hcb,
  by-thod). Killed, Arena's face-down card dissolves as a card back and a
  "Revealed Cards" panel then shows its face at the top left (I3Te0m1bBFw
  29:42.9).
- **Cards a look returns.** Cards a look puts back into a library fly straight
  to it. No look at several cards with the rest returned is filmed (by-ubgt).
- **A linked exile returning.** The exiled card returns as its source departs,
  moving with it. No exiling permanent is seen leaving with its card returning
  (by-esr2).
- **Your own spell returned to hand.** It glides from the stack to the hand,
  turning as it goes. Only the opponent's spells are seen returned (by-e0l).
- **The opponent's draws and discards.** A card the opponent draws flies face
  down from their library to their hand. One they discard flies to their
  graveyard and turns face up on the way, as their spells do. Only the player's
  own are seen.
- **Milled cards.** They fly from the library to the graveyard face up, one
  after another as draws do. None is held up large first. No mill is seen.
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
  of more than one card: "Return {most} Cards". A count of targets asked first:
  "How many targets?". Manifest dread: "Manifest one of them." Cards put back
  from a hand: "Put a card on top.", "Put {most:words} cards on top." A
  fateseal: "Top of Library", answered "BOTTOM" or "TOP" as a scry's piles are
  named. A card that goes on top or bottom with no asker to name: "Top or
  Bottom?". Looked-at cards sent to a graveyard or the battlefield: "Put
  {most:words} of them into your graveyard.", "Put {most:words} of them onto the
  battlefield." An optional move of a card anywhere but to exile: "Put that card
  into your hand{from}?", "Put that card into your graveyard{from}?", "Put that
  card onto the battlefield{from}?", "Put that card into your library{from}?",
  "Move that card{from}?". A cost of no mana: "Pay {0}." The opening choices:
  "Play or draw?", answered Play or Draw, and "Keep or mulligan?" over Arena's
  own buttons. Catalogs: "Choose Creature Type", "Choose Card Name". Waterbend:
  "Tap permanents to Waterbend". What a declared creature is aimed at: "Choose
  attack target", "Choose what to block". Piles of a split: "Hand", "Exile",
  "Battlefield". The pass button's captions for where Arena shows none: "To
  Untap", "To Upkeep", "To Main 1", "To Main 2", "To Cleanup", "Nothing to play"
  under a halt and "To Opponent's Turn" under the player's own End Turn. The
  own-target warning (exception 7) asks Arena's question less its end, "Do you
  want to target {name}?", and a spell picked on the stack reads "Target
  {name}". Only the opponent's manifest dread, fateseal and put-back are seen,
  and games begin with the first player chosen (pE4XSNi-nZc 28:43.5, pE4XSNi-nZc
  34:55.0, pE4XSNi-nZc 27:18.0, 0VsYGs88NXA 16:58.0).
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
  Sounds follow the same steps.

## Verbs

Every change on the table is shown by one of these, applied to a card, an
ability or a player by a step of the script the session sends
(`rust/crates/beryl_session/src/script`, K23). The list is closed: what Arena
does not show is composed from these. A verb's duration, delay, stagger and
curve are Arena's medians over the instances in `data/arena-motion.json`, which
also holds each case's own (K27).

- **fly.** A card travels from where it is to its new place, turning over on the
  way if its face changes, and flashes as it lands on the battlefield: a land
  played, a spell cast, a permanent spell resolving, a draw, a discard, an aura
  onto its permanent. A played land glides from where it is released to its slot
  in about 0.4 s. The opponent's spell leaves their hand as a card back and is
  face up when it reaches the stack, about 0.4 s; the player's own flies from
  where it is released in about 0.35 s. A resolving permanent flies to its slot
  in about 0.6 s and the slot flashes. A drawn card flies large beside the
  library, holds about 0.2 s and drops into the hand, about 1.1 s in all; three
  are drawn a quarter second apart. A discard drops to the graveyard at hand
  size in about 0.2 s (UrGalw_f35w 16:14.8, UrGalw_f35w 16:46.3, UrGalw_f35w
  16:47.15, pE4XSNi-nZc 22:00.0, pE4XSNi-nZc 23:23.5, UrGalw_f35w 17:01.3,
  UrGalw_f35w 16:15.8, WQiYBv23buw 29:14.3, 3sEoP_s4DQE 39:04.3, 5UHzpVvUkfI
  1:01:50.3). Measured: 450 ms, 326 ms apart, ease-out.
- **appear.** A card comes to be at its place with no flight: a token, the
  opening hand, a card a look put in the graveyard. A token appears whole with a
  short glow, a batch as one group. The opening hand and a mulligan's new hand
  are there at once (UrGalw_f35w 17:38.4, WQiYBv23buw 23:57.0, UrGalw_f35w
  15:48, 0VsYGs88NXA 54:16.6, iRr-GeSF0lI 21:23.9). Measured: 66 ms, 135 ms
  after its cause, 33 ms apart, in place.
- **vanish.** A card or an ability goes from where it is with no flight, and a
  card is then at its new place: destroyed, exiled, sacrificed, countered,
  returned to a hand, or done resolving. A destroyed permanent falls to ash in
  about 1 s, an exiled one becomes a vortex, a sacrificed one flashes, and a
  returned one glows and is gone; none flies to a pile or a hand. A countered
  spell crumbles on the stack. A resolved instant darkens and dissolves there,
  or shrinks into its graveyard. A finished ability is gone between two frames
  (RwKoGGUmw2k 27:36.4, IEXcjBn3BXk 22:25.4, 5UHzpVvUkfI 26:06.7, UrGalw_f35w
  19:28.5, 3sEoP_s4DQE 24:49.4, I3Te0m1bBFw 39:50.0, WQiYBv23buw 29:15.67,
  pE4XSNi-nZc 22:01.3, IHPV6yRaFlk 39:46.9). Measured: 565 ms, 260 ms after its
  cause, in place.
- **emerge.** An ability lifts out of its source and goes on the stack. The
  source stays as it is. A small copy of the card appears on the source and
  travels along a curve to the stack, growing, in 0.25 to 0.35 s; an activated
  ability does the same. Two triggers of one event leave 0.3 s apart. A dies
  trigger leaves before its source breaks up (IEXcjBn3BXk 20:39.3, 5UHzpVvUkfI
  19:48.5, 5UHzpVvUkfI 19:58.25, krskVfn2v5Q 46:07.2, WQiYBv23buw 49:08.0,
  I3Te0m1bBFw 26:56.75). Measured: 300 ms, 190 ms after its cause, 300 ms apart,
  ease-in.
- **flip.** A card turns over where it lies: a double-faced card transforms, a
  face-down permanent turns face up (not seen: built as "A card turning face up"
  says). A transforming card is covered by an effect (a lava slab, a cocoon) for
  about 1.1 s and shows its new face as the effect clears; it neither turns nor
  grows (WCknTZNbraE 35:49.2, WCknTZNbraE 1:23:36.8, UrGalw_f35w 49:06.5).
  Measured: 1130 ms, in place.
- **tap.** A permanent turns as it taps or untaps. Arena tilts a tapped
  permanent's art crop 7 to 8 degrees in about 0.2 s and dims it; an untap
  restores it in about 0.1 s (IEXcjBn3BXk 19:35.3, UrGalw_f35w 19:49.1,
  0VsYGs88NXA 31:13.15). Exempt: exception 2.
- **strike.** A creature lunges at what it deals combat damage to and drifts
  back. Each attacker winds back, lunges from its row to the player's avatar or
  to its blocker, the screen flashes, and it drifts back, 0.65 to 1.3 s in all.
  A blocked attacker strikes about 0.4 s before an unblocked one. First strike
  is a strike of its own, seconds earlier (IEXcjBn3BXk 22:25.4, IEXcjBn3BXk
  25:55.6, IEXcjBn3BXk 35:35.5, sMpAJ0HKDdg 54:54.0, sMpAJ0HKDdg 54:57.5).
  Measured: 850 ms, 285 ms apart.
- **streak.** A streak leaves one thing and lands on another, which flashes:
  mana from its source into the spell or ability it pays for, and damage from a
  spell or ability to what it hits. Each tapped land sends one streak of its
  color to the spell on the stack in about 0.3 s. A burn spell or ability glows,
  and a streak of fire crosses to its target in about 0.25 s, with a burst where
  it lands; the spell leaves after (TbrYIs2juCw 33:32.45, 3sEoP_s4DQE 45:30.85,
  pE4XSNi-nZc 22:00.6, IEXcjBn3BXk 20:40.88, IEXcjBn3BXk 38:36.7, I3Te0m1bBFw
  31:37.8). Measured: 250 ms, 100 ms after its cause.
- **count.** A number changes and shows that it did: a life total, a kind of
  counter, a power and toughness, a mana pool. A life total jumps to its new
  value in a burst of about 0.4 s; a loss's red number drifts down from the
  opponent's avatar, and a gain's green one rises for about 0.5 s. A counter's
  badge settles onto its permanent after a flash. A planeswalker's loyalty
  changes about a second after its ability reaches the stack (IEXcjBn3BXk
  22:25.7, IEXcjBn3BXk 25:55.6, sMpAJ0HKDdg 54:54.5, 0VsYGs88NXA 31:13.25,
  5UHzpVvUkfI 19:59.25). Measured: 400 ms, 125 ms after its cause, in place.
- **set.** The table changes and nothing moves: a step of the turn begins, a
  library is shuffled, a permanent's characteristics change. Nothing animates as
  a step begins, and nothing shows on a library when it is shuffled (IEXcjBn3BXk
  19:36.5, IEXcjBn3BXk 28:10.5). Measured: 25 ms, in place.
- **showcase.** A card is held up for both players to read: it joins the
  "Revealed Cards" pile at the top left. Revealed cards gather under a "Revealed
  Cards" pill at the top left, which opens into a browser of them and stays
  afterward (I3Te0m1bBFw 22:08.5, I3Te0m1bBFw 29:42.9). Measured: 200 ms,
  ease-out.
- **dwell.** Nothing moves while a spell or ability is read: the opponent's,
  before it resolves unanswered. The opponent's spell sits on the stack for 0.7
  to 1.9 s before it resolves (pE4XSNi-nZc 22:00.4, pE4XSNi-nZc 23:22.5).
  Measured: 800 ms, in place.
- **announce.** A notice passes over the table while play goes on under it: the
  turn banner, the pill that says what a scry did, a coin tossed. "Your Turn" or
  "Opponent's Turn" glows at the centre for about a second while cards move
  under it. A pill above an avatar reads "Scry: 1 top, 1 bottom." for about 3 s.
  A coin a card flips is tossed mid-table and lands in about 1.2 s (IEXcjBn3BXk
  19:35.7, WCknTZNbraE 16:39.7, 0VsYGs88NXA 41:27.3, IHPV6yRaFlk 52:54.6,
  SN9QFe2kM4Y 14:52.7, WCknTZNbraE 16:42.0). Measured: 1200 ms, 116 ms after its
  cause, in place.
- **point.** An arrow or line between two things, from the step that begins it
  to the step that ends it: to a target, from an ability to its source, from an
  attacker to what it attacks, from a blocker to its attacker. A cyan arc runs
  from a spell or ability to its target until it resolves. A thin line joins an
  ability to its source from the frame it lifts. A thin gold line runs from a
  blocker to its attacker until combat damage is done. An attacker has no line
  to the player, only its chevron (I3Te0m1bBFw 31:35.05, bJQDi6NIrZA 55:08.5,
  IEXcjBn3BXk 20:39.3, IEXcjBn3BXk 22:21.5, sMpAJ0HKDdg 54:50, IEXcjBn3BXk
  25:52.5). Measured: 33 ms, 170 ms after its cause, in place.
- **mark.** A mark from the list below on a card, an ability or a player, from
  the step that begins it to the step that ends it. Measured: 50 ms, 125 ms
  after its cause, 0 ms apart, in place.
- **lift.** Cards the opponent is handling come away from their pile, face down,
  from the step that begins the look to the step that ends it. Cards the
  opponent scries come off their library face down at its size and lie beside it
  (IHPV6yRaFlk 52:54.6, qF4GF-vfc8Y 52:59.9). Measured: 433 ms, 500 ms apart,
  ease-out.

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
