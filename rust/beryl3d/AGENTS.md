# Working in `rust/beryl3d`

This adds to the root `AGENTS.md`, whose rules all apply here.

Beryl 3D is the native 3D client of Beryl. Its design is in `docs/beryl-3d.md`,
and `docs/beryl-3d-architecture.md` says how the client core is to be organized:
read that one before changing how the code is structured. This directory is a
Cargo workspace of its own, with its own `Cargo.toml`, `Cargo.lock` and
`target`, so that no check of the main workspace in `rust/` builds it and a
change here selects no lane of the main checks (decision C17). Its crates depend
on the main crates (`beryl_protocol`, `beryl_session`) by path. Its
`[workspace.lints]` table must equal the main one's, which the `lints-match`
check enforces.

A program here that spreads work over threads sizes them with
`beryl_inspect::runner::threads`, which follows the cores Ignite granted it
(`IGNITE_CORES`, and the file `IGNITE_CORES_FILE` names), never
`available_parallelism` alone.

## Crates

- `kit_base` holds shared data, geometry, poses, design tokens and bundled
  fonts.
- `kit_text` shapes text, breaks lines and requests glyphs.
- `kit_frame` describes resolved elements and draw records for one frame.
- `kit_ui` provides elements, styles, widgets, flat layout and device input.
- `kit_space` fits cameras and evaluates motion tracks.
- `kit_fx` produces card effects, lighting, particles and camera shake.
- `kit_scene` resolves hit testing and element coverage.
- `kit_probe` formats frame trees, parses driver actions and checks frames.
- `game_table` places and draws zones and selects the faces they display.
- `game_play` plays scripts, verbs, pointer motions, sound cues, marks and
  floats.
- `game_hud` builds the game screen, prompts, inspector and navigation types.
- `game_manual` builds the manual table, lobby and their commands.
- `game_probe` supplies fixtures, replay timing and game-specific checks.
- `app_screens` builds the app screens and their settings controls.
- `app_client` owns the client, settings, commands and replay driver.
- `scaffold_web` is reserved for fitting zones to the web interface during the
  transition.

The client core uses no GPU, window, clock or thread (`scene-purity`).

- `beryl_render` draws frames with wgpu.
- `beryl_assets` prepares card pictures, glyphs and icons for the renderer.
- `beryl_host` supplies the window, event loop and native phone host.
- `beryl_inspect` reads frames and recordings for the text commands.
- `beryl_record` plays the engine to build the recordings pack.
- `beryl_pictures` draws pictures and films offscreen.
- `beryl_3d` ties the client, host, assets and renderer together.

## Commands

`just b3d` lists every command, and `just b3d <command> --help` explains one.
Beryl 3D's checks are the `b3d-*` families in `ignite/checks/beryl3d.toml`;
`ignite test <check>` runs any of them.
`just b3d timeline-parity [--fixtures] [prefix]` compares the web interface's
and Beryl 3D's timelines of the recordings (see
`data/beryl3d/timeline-parity.json`, below). The `b3d-timeline` family runs it
per fixture against the committed web timelines in `data/beryl3d/timeline/web/`,
which `--refresh-web` rewrites from the live web interface, and
`b3d-timeline-drift` finds when they are out of date. `just b3d parity` prints
the parity list (docs/beryl-3d.md, "The parity list"): for each kind of item the
web interface shows (fixtures, card specs, timelines, verbs, effects, marks,
prompt shapes, screens, UI Conventions entries, tools and launch tests), how
many Beryl 3D has and the first few it lacks; `--full` names all of them and
`--json` prints the whole report. It runs `just b3d test --all` on the desktop
and phone windows, or reads their `--json` outputs from `--results <dir>`
(`desktop.json`, `phone.json`). `just b3d parity --check` fails when
`data/beryl3d/parity.json` misses a skill directory or a web screen file or
names something gone; it runs as a repository check on every change.
`just b3d bug <report>` prints a Beryl 3D bug report and the commands that bring
the app back to its state; `just b3d app --replay <replay.json>` resumes the
game of a replay file, such as a report's, at its latest decision and saves
nothing.

Each crate's integration tests are one binary, `tests/all/main.rs` with one
module per file, because every test binary links the crate and its dependencies
again after an edit. Add a test file under `tests/all/`. Only a file that
installs a global allocator gets a binary of its own in `tests/`. The tests,
`record` and `gate` share one build in cargo's `test` profile (`SHARED_BUILD` in
`tools/src/beryl_tools/beryl3d/cli.py`), so don't build them with `cargo run`.
Don't compile with `just b3d cargo build` either: it builds the `dev` profile,
which none of those commands reuse, and in a tree without that build it compiled
77 crates for ten minutes. `just b3d cargo check -p <crate>` type-checks, and
`just b3d cargo-test -p <crate>` builds and runs a crate's tests on the shared
build.

## Seeing and checking

To see what the client shows, use the text commands, which play a recording
through the client with no window or GPU (the `b3d` skill has the details):

- `just b3d list [prefix]` names every recording, and `just b3d info <name>`
  every moment of one (`view:3`, `step:4.1.0`, `prompt:1`).
- `just b3d perf [fixture...] [--verbs]` plays fixtures in the running app and
  prints their frame timing with the machine's load, as `just ui-perf` does for
  the web interface.
- `just b3d tree <name> --at <moment>` prints the elements on screen. Each line
  is a locator for `--do`, as in `--do 'hover card "Island" #8'`.
- `just b3d diff <name> --from <moment> --to <moment>` prints what appeared,
  left, moved or changed state.
- `just b3d check <name>` runs every interface check on every tick, and exits
  with 1 on a fault.
- `just b3d corpus` lists the first recording and count for each distinct
  signature. `--update` writes their names and every fixture to
  `data/beryl3d/corpus.txt`. The [game probe guide](crates/game_probe/AGENTS.md)
  defines the signature.
- `just b3d test` runs the corpus and the standalone scenes from each seat in
  each desktop window. `--all` runs the entire pack. A prefix chooses any test
  whose name starts with it. Tests are named
  `b3d:<recording>/<seat>/<width>x<height>`, which `tree` and `check` also take.
  The 1600x900 window plays at a Retina display's scale and its tests are named
  `<width>x<height>@2` (`pages::RETINA_WINDOWS`). `--phone-only` runs the three
  phone windows instead, in the phone layout with a fixed safe area
  (`pages::PHONE_SAFE_AREA`), and never writes the list of allowed faults.
- `just b3d timeline <name> [--seat <seat>]` prints one JSON line for each step
  of each script of a recording, and `--all` writes every recording and seat to
  a file under `$BERYL_CACHE/b3d/out`.
- `just b3d export <directory>` writes each recording's messages to
  `<directory>/<name>.<seat>.json`, the fixture files the web interface reads.
- `just b3d png <name> --at <moment> [--tree] [--crop <locator>]` draws that
  moment offscreen with the real renderer, and
  `just b3d film <name> --from <moment> --to <moment> --frames 12` draws up to
  24 frames as a grid in one picture, with a line of text for each. Both print
  the file's path, under `$BERYL_CACHE/b3d/out/`. Cards show the scans in the
  app's scan cache (`~/Library/Caches/ca.thurn.beryl/art`) and Beryl's drawn
  card frame where a scan is missing; neither command downloads one. Set
  `BERYL3D_SCANS` to an empty directory to see every face drawn.
- `just b3d compare <name> --at <moment>` writes one PNG with the web
  interface's screenshot of the same moment on the left and `png`'s frame on the
  right. Beryl 3D plays at real time (`--no-catch-up`), as the web page does, so
  both sides show the same moment. It exports the recording
  (`just b3d export <directory> --only <name>`), shoots the page as
  `just ui-shots` does, and keeps the screenshot, which `--web <png>` reuses.
- `just b3d match snapshot <name> --at <moment>` writes both clients' pictures
  and element records (box, paint order, CSS style, lines of text) of the same
  moment, mid-animation too, to `$BERYL_CACHE/b3d/match/<tree>/<target>/`, with
  a `report.txt` that warns of fonts or card scans the two sides do not share.
  It takes `compare`'s target or a test's name, at scale 2 by default. The other
  `just b3d match` subcommands read its directory. Beryl 3D's records alone are
  `just b3d tree <name> --at <moment> --records`.
- `just b3d match boxes <name> --at <moment>` pairs the snapshot's elements
  (writing `joined.json`) and prints, worst first, each pair whose box differs
  by more than half a point or whose text breaks into a different number of
  lines (`WRAP n/m`, or `LINES n/m` when the words differ), the elements left
  unpaired on each side, and text that overlaps in Beryl 3D but not on the web
  (or the reverse). `--all [prefix]` does it for every desktop and phone test
  target at its end (`just b3d test --list`) and prints each one's totals.
- `just b3d match pixels <name> --at <moment>` prints the elements answerable
  for the pixels the two pictures differ in, worst first, from
  `b3d-render pixels`, with glyph pixels left out and a magnified crop of the
  worst ten in the snapshot's `crops/`. `--only <locator>` (also on `snapshot`,
  and on `just b3d png`) draws one element subtree alone on both sides, on
  black.
- `just b3d match motion <name> --from <moment> --to <moment>` steps both
  clients through the span 60 frames a second and prints, for each moving
  element and its web pair, the largest difference of position, size and opacity
  and the frame of each, worst first, with a plot of both tracks in `plots/`
  (PNG). `--only <locator>` picks one element, moving or not. Effects are
  elements of role `effect` on both sides. `--all` ranks every verb, effect and
  mark of `data/beryl3d/match/motion-census.json` by how far its tracks are from
  the web's (an effect by its own elements only, with the pixels that differ in
  its box where it is strongest), worst first, with the command that shows each.
  Beryl 3D's frames alone are
  `just b3d tree <name> --at <moment> --to <moment> --records`.
- `just b3d match styles <name> --at <moment>` prints each paired element's
  differing style properties as `<element>  <property>  <web>  <Beryl 3D>`,
  largest element first: colors within one level per channel, lengths within a
  quarter point, shadows, gradients and filters field by field. A web value
  `Style` cannot express is counted at the end instead of listed.
- `just b3d match gaps <name> --at <moment>` (or `--all [prefix]`) lists every
  web style value `Style` cannot express, grouped by property and shape (such as
  "box-shadow, 2 layers, 1 inset"), ranked by the screen area of the elements
  using it, with three examples each. It writes the snapshot's `gaps.json`, or
  `$BERYL_CACHE/b3d/match/gaps.json` with `--all`. A change that teaches `Style`
  a new shape adds it to `data/beryl3d/match/supported.json` in the same commit.
- `just b3d match score [prefix]` counts each reference target's differences
  from the web interface (`unmatched`, `boxes`, `styles`, `unsupported`,
  `pixels`) and holds them to `data/beryl3d/web-match.json`: a count above its
  entry fails, and so does one below it, with the `--update` command that lowers
  it. `--update --raise` raises or adds an entry (say why in the commit body),
  and `--refresh-web` rewrites the committed web reference,
  `data/beryl3d/match/web/<target>.json`. The `b3d-web-match` check holds the
  record counts without a browser; `b3d-web-drift` files a ticket when the live
  web interface moves. A prefix of a test's name scores those tests instead.
- `just b3d match census` measures the moments listed in
  `data/beryl3d/match/census.json` (up to eight of each recording's check times)
  as `boxes`, `styles` and `pixels` do, gives each difference to a screen area
  (arrows, hand, battlefield, stack, prompt, game log, piles, life orbs, steps,
  card face, cloth, screens) by its Beryl 3D element, and ranks the areas, each
  with its source directory and its three worst moments. `--areas`,
  `--recordings <prefix>` and `--json` narrow and format the report, written to
  `$BERYL_CACHE/b3d/match/census/<digest>/`; `--choose` chooses the moments
  again.
- `just b3d match samples [prefix]` is the conformance suite for CSS effects:
  each sample of `data/beryl3d/match/samples.json` is one element with the web
  interface's CSS (shadows, gradients, drop shadows, filters, masks, borders,
  plain panels), shot in WebKit at scale 2 and drawn by `b3d-render sample`
  through the flat layer and the real renderer. It prints each sample's `p995`
  (the difference in levels that 99.5% of pixels are within), its largest and
  mean difference and a picture of web, Beryl 3D and the difference amplified; a
  sample passes at a `p995` of 4 or less. A sample's text is set in Open Sans on
  both sides, rasterized by the real glyph worker, and its glyph pixels are left
  out of the score. A sample whose CSS `Style` cannot say yet is `unsupported`,
  with what it lacks and the web's picture. `data/beryl3d/web-match.json` holds
  the results; `--update` lowers them.

These tools lay the frame out at scale 1 unless `--scale` gives another, while
the desktop app on a Retina Mac runs at scale 2, so a picture of a look you
changed is drawn with `--scale 2` (the windowless app runs at 2 on macOS). A
tree line shows that text was laid out, not that its glyphs drew. The text
commands give the client a stub atlas (`driver::glyphs`), which delivers every
glyph it asks for and each icon at its artwork's size (`icons::atlas_size`), so
the "drawn" check faults text drawn with a glyph never asked for and an icon at
a size the atlas does not hold; `just b3d app --for` fails a run that ends with
glyphs missing (`missing_glyphs` in its frame log). A worker or reviewer closes
a ticket that changes how something looks only after reading a picture of it
beside the web interface's at the same moment (`just b3d compare --scale 2`),
and names the file on the ticket ("What these tools cannot show" in the `b3d`
skill).

The text commands are the `b3d` program in `beryl_inspect` (`src/cli`), and the
pictures are `b3d-render` in `beryl_pictures` (with `Headless` in `beryl_host`),
built in debug. A wrong name, moment or locator exits with 2 and lists what
would work.

## Phones

`just b3d ios-smoke` and `just b3d android-smoke` (`--gles` for OpenGL ES 3.0)
build the library and its host, replay a recording in the headless iOS Simulator
or Android emulator, and save the frame log and a PNG of the screen in
`$BERYL_CACHE/b3d/phone`. The app writes its errors and panics to `beryl3d.log`
and wgpu's warnings to `wgpu.log` in its data folder, and the smoke prints them
when it fails. `just b3d ios-smoke --touch` runs a UI test
(`ios/Beryl3DUITests`) that touches, rotates and idles a live game, then checks
the frame log for each event, a nonzero top inset and no frame in the last 2
seconds before the test suspends the app. `just b3d ios-smoke --lifecycle`
backgrounds and resumes a replay, checks that frames resume on the reattached
surface, and exercises low memory, haptics, sharing and opening a URL.
`just b3d ios-device --user-asked` installs on the paired iPhone, only when the
user asked for it in chat. `just b3d phone-search [--gles]` plays a live game
against `mcts-hidden` on each platform for 5 minutes and compares late frames
while the AI searched and while it did not: a measurement of whether the search
starves the frame thread. None of these are check families yet.

## Paths

Beryl 3D's files live only under `rust/beryl3d`, `data/beryl3d` and
`tools/src/beryl_tools/beryl3d`. A change outside them is a change to the main
project, and selects the main lanes.

## Data files

- `data/beryl3d/levels.json` records the client core's crates, their levels and
  dependency order, and `cap_lines`, the default limit on Rust lines in each
  crate's `src/`. Look for a natural seam before exceeding it. If there is none,
  add a `cap_allowances` entry for the crate with its current `lines` and a
  `reason` explaining which seams were considered. The levels report prints each
  allowance and its reason. Raise an allowance by editing the file. `--update`
  lowers it or removes it when the crate fits the default limit. The `levels`
  check reads manifests for dependency order and counts lines that name
  `beryl_protocol` in toolkit crates. `just b3d levels --update` lowers those
  counts when references are removed.

- The host passes validated `kit_base::data::Data` to the client. The app embeds
  the files, and the tools load them from the checkout or `BERYL3D_DATA`.
  `just b3d data` runs the same validation as the `b3d-data` source check.
  `verbs.json` holds verb motion and `audit.json` holds interface thresholds.
  `pointer.json` holds mouse, touch and scrolling thresholds. `catch-up.json`
  and `slide.json` govern queued scripts and table regrouping. `frame-rate.json`
  and `tier-probe.json` control host pacing and startup tier selection.
  `notices.json`, `revealed.json` and `shown.json` control notices, the Revealed
  Cards fan and cards shown over the flat layer. `lighting.json` holds the light
  and shadow darkness both tiers share, the Baseline contact shadows, and the
  Enhanced lights, shadow map and bloom, which the hosts pass to
  `beryl_render::Renderer` when they make it.

- `data/beryl3d/effects/*.json` defines drawing layers for runtime effects. The
  app embeds them. Tools read them from the checkout or `BERYL3D_DATA`. The
  effect names and durations shared by both clients are in
  `data/ui-conventions.json`.

- `data/beryl3d/profiles/` holds the layouts, including their window conditions,
  the camera and container tuning, named slots, interface thresholds and test
  windows. Hosts load the typed profiles with the verb tuning. Only the
  remaining HUD and flat-layer settings are also exported as constants.

- `data/beryl3d/space-constants.json` holds the shared tuning that still
  compiles to constants. The `kit_base` build script publishes it under
  `data::space`. An object becomes a module, a number an `f32` constant and an
  array of two or three numbers a `glam::Vec2` or `Vec3`. The top-level `counts`
  array names the numbers generated as `usize`.

- `data/beryl3d/camera-fit.json`: cards measured in Arena's recordings, each a
  rectangle on a card's face (the art frame) with its four corners in pixels of
  a 1920 by 1080 frame. `just b3d camera-fit` solves the desktop camera's pitch
  and field of view from them. The camera in `profiles/desktop.json` holds the
  result, which a tools test keeps equal to the fit.

- `data/beryl3d/timeline-parity.json`: `{"recordings": {"<name>/<seat>": n}}`,
  the number of steps of each recording and seat whose start or end, verb, case,
  effects, sounds or place differ between the web interface's timeline
  (`ui/scripts/timeline.mjs`) and `just b3d timeline`, or that only one of them
  has. The `timeline-parity` check fails for a recording that differs by more
  steps than its entry, or is not listed. A count may only go down:
  `just b3d timeline-parity --update` lowers entries and refuses to raise or add
  one unless `--raise` is given, and a commit that does says why in its body.
  `just b3d export` puts the session's `stage_timings` response first in every
  recording, so the web script can time the steps a prompt makes itself. A
  recording the web script still cannot time prints no lines there and counts
  every step. `--update` drops entries for recordings that no longer exist.

- `data/beryl3d/parity.json`: the parity list's hand-kept data. `tools` lists
  each job that a skill or a `just` command does for the web interface
  (`{"job", "web", "skills", "b3d"}`), with `b3d` the Beryl 3D command or skill
  section that does it, or null until one does. `skills` maps every directory of
  `.claude/skills` to a job, or to `"none: <why it looks at no client>"`.
  `web_screens` maps every `ui/src/screens/*.tsx`, every `ui/src/table` file
  ending in `View.tsx` or `Opening.tsx` and every `ui/src/table/lobby/*.tsx` to
  the `name` of a `screens.json` screen, to `"missing"` when Beryl 3D has no
  such screen, or to `"none: <why>"`. `verbs`, `effects` and `marks` map each
  name of those lists of `data/ui-conventions.json` to one recording that plays
  it, as `screens.json`'s `prompt_shapes` maps each prompt shape.
  `just b3d parity --check` keeps the maps complete, and a `beryl_inspect` test
  checks that each named recording exists.

- `data/beryl3d/match/motion-census.json`: the span that
  `just b3d match motion --all` measures for each verb, effect and mark of
  `parity.json`, `{"kind", "name", "recording", "seat", "start", "end"}`, or
  `why` none shows it. It is kept so that two runs measure the same frames;
  `--all --refresh-census` rewrites it from the recordings' timelines.

- `data/beryl3d/match/roles.json`: CSS selectors of the web interface's
  elements, each with the Beryl 3D role it stands for, which
  `just b3d match snapshot` gives the web's element records. The first selector
  an element matches wins; an element matching none takes its ARIA role, then
  its tag's.

- `data/beryl3d/match/census.json`: the moments `just b3d match census`
  measures, in milliseconds, by recording (every built screen and every UI
  fixture two agents play from a seed), one recording to a line.
  `just b3d match census --choose [--recordings <prefix>]` rewrites them from
  the check times `just b3d info --json` lists.

- `data/beryl3d/match/samples.json`: the conformance samples of
  `just b3d match samples`, each `{"name", "about", "box", "page", "style"}`
  with an optional `text`, written with the values `ui/src`'s CSS uses. Adding a
  sample is adding an entry; `--update` then records it.

- `data/beryl3d/match/web/<target>.json`: the web interface's element records of
  one reference target, trimmed to what the score reads and written one element
  to a line, which `just b3d match score --refresh-web` rewrites and the
  `b3d-web-match` check reads.

- `data/beryl3d/web-match.json`:
  `{"targets": {"<test name>": {"unmatched", "boxes", "styles", "unsupported", "pixels"}}, "samples": {"<sample>": p995 | "unsupported"}}`,
  the ratchet of the match tools. A target's counts may only go down
  (`just b3d match score`). A sample's value may only go down, and a supported
  sample never becomes unsupported: `just b3d match samples --update` lowers and
  adds entries, and raises one only with `--raise`, whose commit says why in its
  body.

- `data/beryl3d/screens.json`: Beryl 3D's screens, each with the recording, the
  moment (`at`) and the tree line (`shows`) that show it once it is `built`, and
  `prompt_shapes`, a fixture for each shape of `ui/shapes.json`. A screen's
  `web`, `{"hash", "query"?, "click"?}`, says how `just b3d match` opens the web
  interface's own page of it: the route's hash, any words added to its query,
  and a button to press once the page has loaded. A screen without one is
  compared with the game page, `#/game`.

- `data/beryl3d/conventions/<kind>.json`, one per section kind of
  `data/ui-conventions.json` (`exception`, `settled`, `no-equivalent`,
  `not-seen`): `{"entries": {"<convention name>": entry}}`, where an entry is
  `{"recording", "at", "shows"}` with the meaning a `screens.json` screen's has,
  showing Beryl 3D following the convention, or
  `{"open": "<why Beryl 3D cannot follow it without a numbered exception>"}`. A
  recording entry may add `do`, actions run at `at` as `just b3d tree --do` runs
  them (such as a hover); `seat: "opponent"` to see it from the opponent's seat;
  `layout: "phone"` to play it in the phone layout; and `still: false` to see
  the moment mid-animation, as `tree --at` does, instead of once the table
  rests. Its `shows` may leave out the line's id and box, as
  `card "Island" battlefield.yours.lands tapped`. An open entry for work that is
  only not done yet names its ticket (`needs bead <id>`, where a bead is a
  ticket). Beryl 3D never edits `data/ui-conventions.json` itself. A
  `beryl_inspect` test checks each key and each recording.

- `data/beryl3d/gates.json` holds per-material shader limits under `materials`
  (checked by `shader_cost`) and the interface test frame limits under `tests`
  (`just b3d test`).

- `data/beryl3d/budgets.json` holds each board's worst-frame and total work
  budgets under `boards`, along with its peak draw load, handle allocations, and
  fragment budgets for both tiers. Each budget has a `max` and a `why`.
  `just b3d gate` checks native work, `--counts` checks Callgrind instructions
  and the relative cost of the two stress battlefields, and `--gpu` checks
  shader cost, fragments and Enhanced draw calls. The VM runs take turns in
  `beryl3d-bench`. A count at or below its budget passes. A frame allocation
  always fails. Change a budget deliberately and explain the new ceiling.
  `--json` prints the measured counts under `boards` for each mode.

- `data/beryl3d/test-faults.json`:
  `{"tests": {"<test name>": {"<rule>": <count>}}}`, the faults each interface
  test of `just b3d test` may have today, counted by the rule's word
  (`Rule::word`). These are faults that a later milestone's verb, mark or
  placement removes, so the run passes while they exist and still fails for any
  fault that is not listed or a count over its entry. An entry that is now too
  high also fails, with the command that lowers it, so that a fix is recorded in
  the commit that makes it. `just b3d test --update` lowers entries and removes
  those at zero, and changes nothing when nothing improved. It raises or adds an
  entry only with `--raise` beside it, and the commit that does so says which
  ticket owns the fault. `--every-tick` is held to the same list, and `--update`
  does not go with it.

A break caused by a change to the main workspace (a renamed `beryl_session`
item, say) is fixed in the Beryl 3D change that meets it, not by holding the
main change back.
