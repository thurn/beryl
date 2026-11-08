______________________________________________________________________

## name: specs description: Reproduce, debug, verify or QA a card's behavior in Beryl's UI through its spec — every UI moment of the card (stops, prompts, ability choosers, tokens, exile links, stack interactions, payments) as a live game from both seats at /specs/<card>/<moment>. Gives text descriptions, filmstrips, an animation audit and a Playwright driver for any moment. Use when implementing or fixing a card's UI, checking that what happens is animated clearly for both players, reproducing a UI bug report about a card, or extending what specs generate.

# Card specs

A spec shows every moment of play a card's UI must get right. Each moment is a
real game, live and playable, with the app's own engine behind it, seen from
both seats: yours, and the opponent's (**Them**), the same game with the viewer
across the table.

**The principle: everything that happens in the game is animated clearly for
both players.** What the opponent sees counts as much as what the controller
sees: which ability went on the stack, which card a choice took and where it
went, where a token came from, what lies exiled under a permanent.

## The loop

1. **Reproduce.** Find the moment (`just specs --check <card>` lists them), or
   write a scenario in the card's file that reaches the state: the generator
   turns it into moments on the next build. Every moment has a URL (below).

2. **Read it.** `just specs --describe <card> [--moment M]` prints, from each
   seat, the events shown on the way and what holds at its first live decision
   (abilities by their cost). The cheapest check: no images, two seconds. A view
   whose events all arrive on one line is a view the player sees all at once.

3. **Audit its motion.** `just specs --motion <card> [--moment M]` plays each
   moment from both seats with motion on and judges every event: `ok` (moved,
   faded or animated long enough, visibly), `instant` (a jump), `unseen` (moved
   off screen, covered or transparent), `still` (nothing showed it), and
   `crowded` (more than three events in one view). About 20 s a card; the JSON
   with every event, ok ones too, lands in `$BERYL_CACHE/spec-motion/`. Taps and
   untaps are `minor`: printed only with `--minor`. A finding is a lead, not a
   verdict: look at the film before fixing.

4. **Watch it.** `just specs --film <card> [--moment M]` writes one sheet per
   moment, `$BERYL_CACHE/spec-film/<checkout>/<card>/<moment>.png`: the bursts
   of motion as frames, each your seat over theirs, labelled with ms since play.
   Every frame, full size, is beside it in `<moment>/` (`<ms>-yours.jpg`,
   `<ms>-theirs.jpg`) for a closer look. Read the sheet, not every frame.

5. **Drive it.** `just specs --drive --port 0` serves the pages and a headless
   WebKit page that runs Playwright snippets POSTed to the printed driver URL
   (`ui/scripts/drive.mjs`):

   ```sh
   curl -s localhost:<port> -d '
   await spec.both("currency-converter", "ability-2");   // both seats, held
   const found = await probe.audit(() => spec.play());   // play, judge motion
   return { found, shot: await shot("after") };'
   ```

   `spec.open(card, moment, seat)` opens one seat, `spec.both` both, held until
   `spec.play()`; `spec.seat("theirs")` points `page`, `shot` and `probe` at
   that seat; `spec.settled()` waits for quiet. `probe.audit(action)` audits
   whatever the action sets going, a live click as much as a play; `probe.act`,
   `probe.motion`, `probe.covered`, `probe.timeline` and `probe.compare` are the
   ui-review skill's. Restart the driver to load a change to the scripts; a
   change under `ui/src` reloads with the next `open`, one in Rust needs a
   restart.

6. **Fix generally** (below), then run the loop again on the card and on the
   cards that share what you changed.

## Moment URLs

- `/specs/<card>`: every moment, yours, then theirs.
- `/specs/<card>/<moment>`: one moment, both seats side by side.
- `/specs/<card>/<moment>/<yours|theirs>`: one seat full size, a fresh game each
  load, which plays a beat after it shows; with `?hold` it waits for its spec
  server instance's `play` (`spec.play()`, or `POST <live>/play` where `<live>`
  is the page's `?live=` URL).

`just specs [card]` serves them for a human.
`just specs --shots --port 0 [card]` screenshots every moment settled from both
seats, for a quick look at end states.

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
  (`{T}, bounced`), each question the card asks for each cause (`Ask: mayMove`),
  with a yes or no flipped (`Answer No`). For a card with activated abilities,
  its end or a flipped answer where the human holds priority goes on twice more:
  they pass (`Then pass`: where does it stop them next?) and the game reaches
  their next turn (`Next turn`: what does it offer untapped?). Scenario boards
  are dressed to look like a game (life off 20, graveyards, the opponent's lands
  and a response in hand, unplayable spells under each library) only where the
  scenario still passes with the dressing. A scenario's name is its heading on
  the page, so name it for what it shows.

Each candidate is played headless from both seats to its first live decision (or
the game's end), and what holds there is recorded in the fixture conditions'
words (`fixture.rs`'s `Condition::parse`), which `--describe` prints. Candidates
that repeat an earlier question or end are dropped; candidates whose scripts
cannot be played are reported as `skip` lines.

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
- Short of its next action, a scripted seat passes priority, pays the first way,
  answers one-answer questions, declares no attackers or blockers, discards its
  first cards to hand size, and takes a scripted "may" the session folds into
  the question it leads to (a search's choice: yes is the choice, no is Done); a
  scripted target the engine chose silently (the only one) is skipped. Any other
  decision it cannot answer fails, naming it.
- Once both seats' actions are taken (and the game reaches the turn of a
  trailing `AdvanceTo`), the viewer's seat plays live. The other seat then
  passes and takes the first candidate of anything else.
- The human is under the smart stops, as in the app, and the session halts them
  at the start of their own first main phase (not when a moment starts there).

## When a moment is missing or wrong

Fix it in this order, preferring the general fix:

1. **A scenario in the card's file** that reaches the state. It also tests the
   rules and runs against XMage and Forge (`just oracle`), and the generator
   picks it up.
2. **A general capability in the generator**, which covers every card with the
   same ability, cost or question: a new probe in `from_definition`, a better
   cut or dedupe in `from_scenario`/`generate`, a condition `snapshot` records,
   or a scripted-seat rule in `boards.rs`'s `Scripted`. Say which other cards'
   moments it changed.
3. **A general fix in the UI**: an animation belongs to an event kind or a zone
   change, never to one card, so fix it where every card with that event gets it
   (`ui/src/game/playback.ts` holds and beats, `CardLayer.tsx` card motion), and
   audit a few other cards with the same event.

There are no hand-written moments: a case only one card has is a scenario in its
file. A fixed engine bug gets its entry in `docs/bugs.md`.

## Judging against the design

Beyond motion, judge each moment from both seats against the design philosophy
in `.agents/ui-review/SKILL.md`: common case one gesture, the client does the
bookkeeping, ask once in context, Arena as the reference, show don't tell, few
words. Report what each moment shows from each seat, what you fixed, which other
cards' moments changed, and any moment you could not get and why.
