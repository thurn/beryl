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
  falls to ash over about 0.7 s; the spell then goes to its graveyard
  (RwKoGGUmw2k 27:36.4, bJQDi6NIrZA 51:46, TbrYIs2juCw 30:37, 5UHzpVvUkfI
  58:17.0).
- **Choosing how to cast.** A card or permanent with more than one way to cast
  or use it opens a modal, "Choose One", showing the card once per option, with
  Cancel: an adventure, an alternative cost, a land's abilities (by-lzl,
  by-9fb). Brazen Borrower, Bonecrusher Giant and Force of Will open it from
  hand, Wasteland on the battlefield (SN9QFe2kM4Y 42:44.2, 3sEoP_s4DQE 50:44.3,
  3sEoP_s4DQE 46:50.2, MnhyzykajXw 35:43).
- **A finished ability.** It is removed at once, with no fade (by-c8b).
- **Wording.** Arena's exactly: "All Attack", "Choose attackers." (by-u4g,
  by-a09o).
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
  above their avatar reads "Scry: 1 top, 1 bottom." for about 2.5 s (by-pv6p,
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
- **The phone.** It plays what the desktop plays; its layout and gestures are
  Beryl's own.
