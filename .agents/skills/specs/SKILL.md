---
name: specs
description: Reproduce, debug, verify or QA a card's behavior in Beryl's UI through its spec — every UI moment of the card (stops, prompts, ability choosers, tokens, exile links, stack interactions, payments) as a live game from both seats at /scenarios/<card>/<moment>. Gives text descriptions, filmstrips, an animation audit and a Playwright driver for any moment. Use when implementing or fixing a card's UI, checking that what happens is animated clearly for both players, reproducing a UI bug report about a card, or extending what specs generate.
---

# Card specs

A spec shows every moment of play a card's UI must get right. Each moment is a
real game, live and playable, with the app's own engine behind it, seen from
both seats: yours, and the opponent's (**Them**), the same game with the viewer
across the table.

**The principle: everything that happens in the game is animated clearly for
both players.** What the opponent sees counts as much as what the controller
sees: which ability went on the stack, which card a choice took and where it
went, where a token came from, what lies exiled under a permanent.

## Server lifecycle

Use `--port 0` whenever you start a long-running `just specs`, `just scenarios`,
or `just playground` session, including with `--release`. Use the URL it
prints. This gives parallel sessions separate ports and leaves 1430 available
for manual use.

Keep the terminal session or record the PID of your `by specs` process. When
finished, including after a failed check, stop your session with Ctrl-C or
send SIGTERM to that process so it cleans up its children. Verify that its
printed UI and driver ports no longer have listeners. Leave other sessions
alone.

## The loop

Cards are named by slug or by name
(`just specs --shots "Teferi, Time Raveler"`), several to a run. A card with no
spec is refused with the specced cards nearest it in rules text, whose moments
show the mechanics it shares.

1. **Reproduce.** Find the moment (`just specs --check <card>` lists them), or
   write a scenario in the card's file that reaches the state: the generator
   turns it into moments on the next build. Every moment has a URL (below).

2. **Read it.** `just specs --describe <card> [--moment M]` prints, from each
   seat, the events shown on the way and what holds at its first live decision
   (abilities by their cost). The cheapest check: no images, two seconds. A view
   whose events all arrive on one line is a view the player sees all at once.
   `--views` instead of `--describe` prints each view whole: its events, what it
   asks, the stack bottom first, and each side's battlefield, hand, graveyard,
   exile and library size, as that seat sees them (a hidden card is `?`).

   `--script` instead prints each seat's scripts, view by view: what the session
   told the stage to show, one step a line (`fly`, `vanish`, `+point`, `tap`,
   `streak`, ...; `&` joins a step to the beat above, `<-` names its cause;
   `beryl_session`'s `script/text.rs` documents the form). Read it first for any
   wrong motion, arrow, combat line or mana flow, before filming: a wrong script
   (a missing, extra or misordered step, a wrong cause, an arrow ended late) is a
   session bug, fixed in the builder (`beryl_session/src/script/`); a right
   script played wrongly is a stage bug (`ui/src/stage/`).

3. **Audit its motion.** `just specs --motion <card> [--moment M]` plays each
   moment from both seats with motion on and judges every event: `ok` (moved,
   faded or animated long enough, visibly), `instant` (a jump), `unseen` (moved
   off screen, covered or transparent), `still` (nothing showed it), and
   `crowded` (more than three events in one view). Arrows are judged too: `jump`
   (a new path while both its ends stood still) and `stretch` (drawn while an end
   moved). Beside the events,
   `duplicate` flags two visible instances of the same card in any recorded
   frame, including face-down cards, browsers and prompts. Hover inspection
   is exempt. Distinct slots and ability indicators may share card art.
   The `ui` findings judge what moved with no event behind it (a fan making
   room, a button sliding in). About 20 s a card; the JSON with every event, ok
   ones too, lands in `$BERYL_CACHE/spec-motion/`. Taps, untaps and `ui`
   findings of a scripted play are `minor`: printed only with `--minor`. A
   finding is a lead, not a verdict: look at the film, and at Arena's motion at
   the same moment, before fixing. Arena's tokens appear at their slot and its
   permanents dissolve where they are (UI Conventions, "Settled as Arena"), so
   a `still` or `instant` there can be right.

4. **Watch it.** `just specs --film <card> [--moment M]` writes one sheet per
   moment, `$BERYL_CACHE/spec-film/<checkout>/<card>/<moment>.png`: the bursts
   of motion as frames, each your seat over theirs, labelled with ms since play.
   Every frame, full size, is beside it in `<moment>/` (`<ms>-yours.jpg`,
   `<ms>-theirs.jpg`) for a closer look. Read the sheet, not every frame. A run
   replaces only its own cards' folders. `--viewport 1180x820` films (and shoots
   and audits) at another size: some faults show only in a small window.

5. **Drive it.** `just specs --drive --port 0` serves the pages and a headless
   WebKit page that runs Playwright snippets POSTed to the printed driver URL
   (`ui/scripts/drive.mjs`). Keep it in a terminal session you can interrupt.
   Snippets run one at a time in the order they arrive:

   ```sh
   curl -s localhost:<port> -d '
   await spec.both("currency-converter", "ability-2");  // both seats, held
   const found = await spec.audit();                    // play both, judge motion
   return { found, shot: await shot("after") };'
   ```

   `spec.open(card, moment, seat)` opens one seat (and points `page` at it),
   `spec.both` both, held until `spec.play()`, and `spec.audit(action?)` plays
   them (or runs the action, a live click) and audits both seats' motion. An
   action is the viewer's own, so what it moves before the game answers is
   judged too, not minor: a chooser opening, the hand rising (`ui` findings).
   `spec.seat("theirs")` points `page`, `shot` and `probe` at that seat at once;
   `spec.settled()` waits for quiet.

   `spec.ordered(patterns, { seat, action })` asserts the order things showed
   in: each pattern (a substring, any case, or a RegExp) takes the first entry
   of the seat's timeline it matches, and each must show no earlier than the
   one before. It reads the last `spec.audit`, or runs one (with `action`), and
   returns `ok`, each match's ms, why not, and the whole timeline
   (`spec.timeline(seat)`): event lines as `--motion` words them, and `ui`
   lines for what a click opens (`choice-card «Island»: appeared over 244 ms`).
   From their seat on Winds of Abandon, the search waits for the cast and the
   exile:

   ```js
   const m = "exiles-a-creature-its-controller-fetches-a-tapped-basic-0";
   await spec.both("winds-of-abandon", m);
   const steps = ["cast Winds", "->exile", "choice-card"];
   return spec.ordered(steps, { seat: "theirs" });
   ```

   A test with its own recording uses `timeline` and `ordered` from
   `ui/scripts/audit.mjs` the same way.

   `spec.fixture(name, at, { clock, viewport })` opens a ui-test or ui-shots
   fixture (`ui/fixtures.json`, recorded on first use) at its view `at` in a
   page of its own, which becomes `page`; `clock` installs Playwright's clock
   as ui-test does. `spec.fixtureViews(name)` lists its views with the index
   `at` takes, as `just ui-shots --views <name>` prints them.
   `spec.gestures()` drags each of the viewer's permanents (which must not
   follow the pointer or act) and a playable hand card onto the table (which
   must), and reports `faults`; `probe.drag(from, to)` is one drag, with what
   followed and what the page sent. `probe.audit(action)` audits whatever the
   action sets going; `probe.act`, `probe.motion`, `probe.covered`,
   `probe.timeline` and `probe.compare` are the ui-review skill's.

   The driver reloads itself: a change under `rust/crates` rebuilds beryl_cli
   and restarts the spec server, a change to `ui/scripts` restarts the driver
   (both on their ports, printed on its output), so open the moment again after
   either; a change under `ui/src` reloads with the next `open`. A browser that
   goes away is launched again, and opening a seat retries a slow or failed
   load.

6. **Fix generally** (below), then run the loop again on the card and on the
   cards that share what you changed.

## Moment URLs

- `/scenarios/<card>`: every moment, yours, then theirs.
- `/scenarios/<card>/<moment>`: one moment, both seats side by side.
- `/scenarios/<card>/<moment>/<yours|theirs>`: one seat full size, a fresh game each
  load, which plays a beat after it shows; with `?hold` it waits for its spec
  server instance's `play` (`spec.play()`, or `POST <live>/play` where `<live>`
  is the page's `?live=` URL).

`just scenarios --port 0 [card]` (or `just specs --port 0`) serves the scenario
viewer. `/scenarios/<card>` groups named scenarios with a checkpoint selector; direct moment and
seat URLs continue to work. The headless describe, film, motion and drive tools remain.

`just playground --port 0 [card]` serves the same host with `/playground`
for human card review.
Each card page stacks its examples, starting with play from hand, and each example has
independent full-width boards for both perspectives. The viewer answers their own prompts;
the unseen player is automated. Play releases opponent playback; Reset affects only its
board. Every variation has its own visible board pair, discovered by scrolling; Playground
has no example selector. A board's bug button
captures its current game. Reviewed is manual, independent of open bugs, and never expires;
existing marks remain in `~/Library/Application Support/ca.thurn.beryl/spec-reviews.json`.

Playground generation lives in `rust/crates/beryl_cli/src/specs/playground.rs`. It reuses
scenario setup data and ability definitions, adding support cards through the engine's target
filters. Change a general generator capability first; use a scenario in the card's file for
a missing interaction. The scenario viewer's replay setups are not modified by Playground.

To audit Playground itself, use `await spec.playground(card)` to list its examples, then
`spec.open(card, variant.name, seat)` or `spec.both(card, variant.name)`. Names starting
with `playground-` open the real independent Playground sessions. `spec.play()` releases
an opponent-play board; the viewer's own cast and choices stay interactive. Use
`spec.audit(action)` for those gestures. A fully scripted Scenarios replay alone does not
prove that a human Playground board offers the same interaction.

When checking a historical bug, verify its distinguishing conditions, not just the card:
token versus ordinary target, a lower stack item, known library top, both casting costs
affordable, and the affected seat's prompt. The unseen player retains scenario choices,
including originally automatic targets, and waits for the scenario's stack context before
responding. New scenario data needs no card-specific viewer code and appears by scrolling.

`just specs --shots --port 0 [card...]` screenshots every moment settled from
both seats, for a quick look at end states: `--moment M` one moment,
`--select .prompt-title` only the seats where that selector shows, and
`--crop .prompt` (or `--crop X,Y,W,H`) only that element's box or region.

For ui-test and ui-shots: `just ui-test <filter...>` runs only the checks whose
titles contain a filter, and `just ui-shots --views <fixture>` lists a
fixture's views with the index a route's `&at=` takes, each with its script's
steps in brief. A scripted board that
stalls names the actions still waiting and the last decision one fit no
candidate of (a target that resolved to nothing offered). A board's fixture
records from seat 1 with `"until": "first <kind> decision for seat 1"`.

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
  and a response in hand, unplayable spells under each library, and a twin of
  the card in the human's hand where it starts there, so two cards just alike
  must be kept apart) only where the scenario still passes with the dressing. A
  scenario's name is its heading on the page, so name it for what it shows.

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
2. **Each ability's path.** Clicking it (a permanent never drags); two or more
   non-mana activated abilities open the side-by-side chooser, unaffordable
   ones dimmed.
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
3. **A general fix in the UI**: a prompt, layout or wording belongs to a shape
   or a zone, never to one card, so fix it where every card with that shape
   gets it, and audit a few other cards that open it. A wrong motion, arrow or
   combat line is banked for the stage, not fixed in the code it replaces
   (`ui/src/game/beats.ts`, `playback.ts`'s holds, `CardLayer.tsx`): the
   `bugfix` skill, step 3, says how.

There are no hand-written moments: a case only one card has is a scenario in its
file. A fixed engine bug gets its entry in `docs/bugs.md`.

## Judging against Arena

Beyond motion, judge each moment from both seats against Arena and
[UI Conventions](../../../docs/ui-conventions.md): find the same kind of moment
in the recordings with the `arena-video` skill and compare what is asked, in
what words, with which buttons, what is highlighted, and what moves where and
for how long. Beryl differs only where UI Conventions lists a numbered
exception; any other difference is a finding, cited with the recording id and
time. Where the recordings show nothing, judge by the design philosophy in
`.agents/skills/ui-review/SKILL.md`, and where the user's eye is wanted, proceed
and file a bead labelled `human` saying what to look at. Report what each moment
shows from each seat, what you fixed or banked, which other cards' moments
changed, and any moment you could not get and why.
