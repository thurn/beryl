# Working in `ui/`

This adds to the root `AGENTS.md`, whose rules all apply here. Its "UI" section
says how Beryl follows MTG Arena.

## Source

- `src/ipc.ts` has typed wrappers for the Rust commands. Always call Rust
  through it.
- `src/ipc.ts` also has a fixture transport that replays recorded sessions, from
  seeded games or from scripted boards (`fixtures.json` and `beryl_cli`'s
  `boards.rs`).
- `src/generated/protocol.ts` is generated from `beryl_session` by ts-rs. A test
  fails when it is out of date, and its header explains how to regenerate it.
- `src/stage/` is the "stage", the code that draws the game table and animates
  it. It plays the session's animation scripts on a clock that tests can step
  through one tick at a time (plan K25).
- The UI asks for card records by name through `beryl_session`'s `card_records`
  (printing, artist, text and image URLs for each face), and
  `src/cardRecords.ts` caches them before drawing. The UI bundles no card data
  of its own.

## Testing

- `npm run typecheck` runs tsc. `npm test` runs the tests that need no browser
  (`scripts/*.test.mjs`, which cover the stage's clock and the order animations
  play in).
- `just ui-shots [names]` takes headless screenshots of the UI states listed in
  `shots.json` and prints their paths.
- `just ui-test` plays the game screen through a fixture (`scripts/kept.mjs`).
  Then it runs each animation verb on the stage's own clock
  (`scripts/verbs.mjs`; `just ui-test verb:` runs only these). Then it runs each
  prompt type, answering it with the clicks recorded in its fixture along with
  the session's replies (`scripts/shapes.mjs`; `just ui-test shape:`).
- `just ui-play [fixtures]` plays every recorded fixture to the end, on both the
  desktop and phone layouts, with the stage's checks turned on.
- `just specs --phone` and `just ui-live --phone` open the phone layout.
- Every card has a spec page (`/specs/<card>`), generated from its definition
  and scenarios. It shows each UI moment of the card as a live game from both
  players' seats. The `specs` skill covers it: `just specs --describe`,
  `--film`, `--motion` and `--drive` show what happens at each moment for both
  players as text, as pictures, as an animation report and under Playwright.
- `just ui-perf <fixtures>` plays fixtures live on a production build and
  reports frame lengths and the share of dropped frames. `--verbs` lists the
  animations the long frames came in, and `--browser chromium` adds main-thread,
  GPU and raster time. The machine is always loaded, so compare builds over
  several interleaved runs; Chromium's raster time is the steadiest number.
  Anything that animates every frame should change only `transform` or `opacity`
  on an element with a layer of its own (`will-change`), or WebKit repaints what
  lies under it, filters and all, every frame.
- `just ui-counts <fixtures>` counts the work the page does to play fixtures:
  React components rendered (`--components` lists which), DOM mutations, and
  elements Chromium restyled and laid out. It plays on Playwright's paused
  clock, one frame at a time, so the counts don't depend on load and compare
  across runs on this Mac; use them, not `ui-perf`'s timings, to tell whether a
  change made the UI do more work. Slow CI holds a few fixtures to the limits in
  `data/ui-counts.json`.
- `just motion` records each animation verb in stepped stage time and compares
  it with its own values in `data/ui-conventions.json` (the `motion` lane in
  `docs/checks.md`).

The machine runs at most half its core count's worth of browser pages at once,
across all runs (`scripts/slots.mjs`). Runs that aren't checks share a third of
those slots. Headless WebKit runs at a higher priority than any build, so run
only the checks your change needs (`just ui-test <title>`, `just specs <card>`)
rather than a full sweep.
