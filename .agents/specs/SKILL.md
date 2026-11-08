---
name: specs
description: Review or extend a card's spec — the live, scrollable page at /specs/<card> showing every UI moment of that card (stops, prompts, ability choosers, tokens, statuses, exile links, stack interactions, payments) as playable game states, from both seats — and QA the card's UI through it. Every card's spec is generated; use when asked to spec a card, review its UI states, QA a card end to end, approve changed spec records, or check that a card's UI follows the design.
---

# Card specs

A spec is a scrollable page, `/specs/<card-slug>`, that shows every moment of
play a card's UI must get right. Each moment is a real game, live and fully
playable in the page: the engine behind it is the app's. Scrolling a spec is QA
testing the card.

Every moment shows twice, side by side and played in step: from your seat and
from the opponent's (**Them**), the same game with the viewer across the table.
What the opponent sees of a card counts as much as what its controller sees:
which ability went on the stack, which card a choice took and where it went,
where a token came from, what lies exiled under a permanent.

**Every implemented card has a spec, generated.** Prefer reviewing the generated
spec and adding missing general capabilities over hardcoding specific QA cases.

## How a spec is generated

`rust/crates/beryl_cli/src/specs/auto.rs` builds each card's moments from two
sources, with short general labels:

- **The definition.** A permanent with activated abilities (not mana abilities)
  gets probes: `Stops: main 2` and `Stops: their end` (instant-speed abilities),
  `Lands tapped` (what stays offered unpaid), `Nothing to do` (an ability whose
  `Needs` is unmet must not stop) beside the same board with the need met
  (`Stops: card exiled`, through `Permanent::exiled_with`), each ability
  activated (labelled by its cost: `{2}{T}`, `+2`, `−1`), and one bounced in
  response (`{2}{T}, bounced`).
- **The card's scenarios** (`beryl_cards::SCENARIOS`), each under its own name
  as a heading (less the card's name). `Start` opens on its first board, and
  play runs the whole scenario (held to the turn a trailing `AdvanceTo` names).
  It is cut at each cast or activation of the card (`{T}`, played out once
  shown), the same activation with the card bounced in response
  (`{T}, bounced`), each question the card asks for each cause
  (`Ask: mayMove`), with a yes or no flipped (`Answer No`). For a card with
  activated abilities, its end or a flipped answer where the human holds
  priority goes on twice more: they pass (`Then pass`: where does it stop them
  next?) and the game reaches their next turn (`Next turn`: what does it offer
  untapped?). Scenario boards are dressed to look like a game (life off 20,
  graveyards, the opponent's lands and a response in hand, unplayable spells
  under each library) only where the scenario still passes with the dressing.
  A scenario's name is its heading on the page, so name it for what it shows.

Each candidate is played headless from both seats to its first live decision
(or the game's end), and what holds there is recorded in the fixture conditions'
words (`fixture.rs`'s `Condition::parse`). Candidates that repeat an earlier
question or end are dropped; candidates whose scripts cannot be played are
reported as `skip` lines.

The records are approved in `data/specs/<slug>.json`. `cargo test -p beryl_cli`
(and so `just check`) fails on a moment whose record changed, one not yet
approved, or an approved one no longer generated.

## Commands

- `just specs --check [slug]`: checks every moment; `skip` lines name the
  candidates generation could not play, and why.
- `just specs --approve [slug]`: records each generated moment as approved, then
  checks. Approve only after looking (below); the diff of `data/specs/` is the
  review record, so read it.
- `just specs --shots --port 0 [slug]`: screenshots every moment from both
  seats, settled, into `$BERYL_CACHE/spec-shots/<checkout>/<slug>/`
  (`<moment>.png`, `<moment>-theirs.png`) and prints the paths.
- `just specs [slug]` serves the pages for a human and prints the URL.

## When a moment is missing or wrong

Fix it in this order, preferring the general fix:

1. **A scenario in the card's file** that reaches the state. It also tests the
   rules and runs against XMage and Forge (`just oracle`), and the generator
   picks it up.
2. **A general capability in the generator**, which covers every card with the
   same ability, cost or question: a new probe in `from_definition`, a better
   cut or dedupe in `from_scenario`/`generate`, a condition `snapshot` records,
   or a scripted-seat rule in `boards.rs`'s `Scripted`. Say which other cards'
   records it changed.
There are no hand-written moments: a case only one card has is a scenario in its
file.

A record that shows behavior the design does not want is a bug to fix, in the
engine, the session or the UI, not a record to approve. A fixed engine bug gets
its entry in `docs/bugs.md`.

## Choosing what a card needs

Use this list to judge whether a generated spec is complete, and what general
capability to add when it is not:

1. **Priority and stops.** When the card makes the human stop, and when it must
   not: an affordable instant-speed ability stops them; one that would do
   nothing (`Needs`) must not, though it stays activatable.
2. **Each ability's path.** Clicking it, dragging it out; two or more non-mana
   activated abilities open the side-by-side chooser, unaffordable ones dimmed.
3. **Costs and payments.** Mana, X, alternate and additional costs, life,
   sacrifice, tapping creatures, loyalty.
4. **Prompts.** Each question the card asks: may, choose, target, order,
   discard.
5. **What it makes and changes.** Tokens, emblems, counters, damage, control
   changes, attachments, tapping.
6. **Zones.** Cards it exiles (and where they show), cards it makes playable
   from exile or the graveyard, what it puts into a graveyard.
7. **The stack.** Responses to it and from it: countered, bounced in response,
   its targets gone, its source gone.
8. **The other side.** Each moment's Them frame: the opponent watches the card
   played, and stops on it while they hold a response.
9. **Edges a player hits.** Empty hand, no legal target, nothing to choose, one
   choice (answered silently), more than one.

## How scripts run

- Both seats' actions run in the order listed, across seats: a response waits
  until what it answers has been played. An `AdvanceTo` holds the actions after
  it until the game reaches its turn and step.
- Short of its next action, a scripted seat passes priority, pays the first
  way, answers one-answer questions, declares no attackers or blockers,
  discards its first cards to hand size, and takes a scripted "may" the session
  folds into the question it leads to (a search's choice: yes is the choice, no
  is Done); a scripted target the engine chose silently (the only one) is
  skipped. Any other decision it cannot answer fails, naming it.
- Once both seats' actions are taken (and the game reaches the turn of a
  trailing `AdvanceTo`), the viewer's seat plays live. The other
  seat then passes and takes the first candidate of anything else.
- The human is under the smart stops, as in the app, and the session halts them
  at the start of their own first main phase (not when a moment starts there).

## Judging

1. `just specs --check <slug>`; read the `skip` lines.
2. `just specs --shots --port 0 <slug>` and look at every screenshot, both
   seats, against the design philosophy in `.agents/ui-review/SKILL.md`: common
   case one gesture, the client does the bookkeeping, ask once in context,
   Arena as the reference, show don't tell, few words.
3. For what a still cannot show (a click, a drag, a chooser, a browser), drive
   the moment's page with Playwright WebKit from `ui/scripts`, as
   `ui/scripts/specs.mjs` does: open
   `<vite>/?live=<vite>/spec-api/m/<slug>/<moment>/<yours|theirs>/<any-id>#/game`,
   click `.card-el[data-zone=battlefield][data-card='<name>']`, screenshot.
4. What the opponent sees is mostly motion, which stills miss. Record the Them
   frame as video (Playwright's `recordVideo`) while its actions play, and look
   at it frame by frame (`ffmpeg -vf fps=20`). Delete any probe script after.
5. Fix what is wrong, generally (above), then `just specs --approve <slug>`
   and read the records' diff.
6. Report what each moment shows from each seat, what you fixed, which other
   cards' records changed, and any moment you could not get and why.
