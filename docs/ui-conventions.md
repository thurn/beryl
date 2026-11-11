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
   by-vrv). Arena uses one color.
5. **Lands unstacked until seven.** Each land is separate until its controller
   has seven or more; then same-named lands stack (by-p9e, by-goui). Arena's
   threshold has not been measured.
6. **Linked exile under its host.** A card exiled "until this leaves the
   battlefield" sits tucked under the permanent that exiled it (by-8g5). Arena
   shows no link on the table.
7. **Own-target warning.** Confirm before a counter or removal spell targets the
   player's own spell or permanent (by-cp2y, by-jmx9).

## Settled as Arena

Earlier reports asked for something else in some of these. Arena's way stands,
so do not restore the old behavior.

- **Tokens.** Tokens appear at their slot; they do not fly out of their source
  (by-kgc, by-81h).
- **Permanents that leave.** They dissolve where they are, tokens included
  (by-1ug).
- **Choosing how to cast.** Evoke, cycling and the like open a modal showing the
  card once per option (by-lzl, by-9fb).
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
- **An additional-cost chooser.** It opens before mana taps (by-jqd).
- **A countered spell.** It leaves before the spell that countered it (by-kxyq).
- **Cards the opponent looks at.** They stay library-size beside their library,
  face down, and visibly go to the top or bottom (by-pv6p, by-yfb2).
- **A line from an ability to its source.** It is always shown while the ability
  is on the stack, thin and faint as Arena draws it, and hidden during a prompt.
  A heavy arrow is not that line (by-wkr, by-afh, by-110h).

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
- **The phone.** It plays what the desktop plays; its layout and gestures are
  Beryl's own.
