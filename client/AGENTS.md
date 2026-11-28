# Working in the client

The client is Beryl's native client on desktop and phones. See
[the client](../docs/beryl-3d.md) for the frame contract, rendering and hosts.
See [the client architecture](../docs/beryl-3d-architecture.md) for the client
core's crate boundaries, arrangement, layout profiles and checks.

## Client core

- Toolkit crates provide geometry, text, frames, widgets, layout, motion,
  effects, hit testing and probes. Give them plain values and opaque IDs.
- Game crates adapt the protocol to the toolkit. They own table placement,
  script playback, the HUD, the manual table and game probes.
- App crates supply the other screens and assemble the client. The host supplies
  time, input and assets. The renderer consumes the finished frame.

`just client map` lists the inventories. Give it a kind such as `crates`,
`features` or `containers` for names and source paths.
`just client map containers --full` shows where each profile's prompt tray is
laid out and which fitting policy it uses. `just client map data` lists the
shared timing file, and `just client new effect --help` shows how to register an
effect.

Read the guide in the crate you change. At the same level a crate may depend on
earlier crates in `levels.json`. Dependencies between levels point down. The
client core reads time and assets through its inputs. Given the same inputs it
draws the same frames, and an animation-only frame allocates nothing.

## Building and testing

This directory has its own Cargo workspace. Its lints match the main
workspace's. Use `just client cargo check -p <crate>` to type-check, and
`just client cargo-test -p <crate>` to build and run a crate's tests. Tests, the
recorder and the gate share a build in Cargo's test profile. Use these commands
to reuse it. Avoid `just client cargo build` and `cargo run`, which compile a
separate profile.

Put integration tests in `tests/all/main.rs` with one module per file. Tests
that install a global allocator have their own binaries. Programs that spread
work over threads use `beryl_inspect::runner::threads` for the core count Ignite
granted.

`ignite test <check>` runs the `client-*` families in
`ignite/checks/client.toml`. Use the smallest check that observes the change.
`just client test` checks the corpus and standalone scenes in the desktop and
phone window matrices. Use a test name prefix for one scene or `--phone-only`
for only the phone windows. `just client record` builds the corpus pack from the
fixtures in `data/client/fixtures.json` and card spec moments.
`just client corpus --sweep` checks card spec moments for signatures the corpus
lacks.

## Seeing the result

`just client` lists the commands. `just client <command> --help` gives their
options. Use `list` and `info` to find a recording and its moments, `tree` and
`diff` to inspect what the player sees, and `check` to check one scene on every
tick. `tree` prints locators you can pass to `explain` or use in `--do` actions.
The [client skill](../.agents/skills/client/SKILL.md) shows how to read their
output.

For a change in how something looks, open the picture from
`just client png --scale 2` or a film of the affected span. Record the picture's
path and what you saw on the ticket.

Use `just smoke` for windowless desktop startup and `just bundle` to package a
desktop release. Use `just table-pair --headless` for a localhost manual table
and `just client live` to inspect a running debug process.

For phone host changes, `just ios-smoke` and `just android-smoke` replay a
recording headlessly and save a screenshot and frame log under
`$BERYL_CACHE/b3d/phone`. Use the documented touch and lifecycle options for
host input and suspend or resume, and `just ios-smoke --game wu-control` or
`--game forgetful-fish` to play a whole seeded game of that deck against the AI.
The Android smoke's cards show Beryl's frame without scans, because the emulator
cannot reach Scryfall, and it fails when the text still lacks glyphs after the
recording ends. Install on a paired iPhone with `just ios-device --user-asked`
only when the user asks. `just mobile-bugs` pulls the bug reports the phone
smokes saved into the desktop's bugs folder, and `--device` also pulls from a
paired iPhone.

## Sizes on a phone

Everything a finger presses on a phone is at least 40 by 40 points on its own
(`checks.target_points` in `data/client/profiles/phone.json`). Where a group of
things cannot all keep that size, such as a crowded row of permanents, collapse
the group into one target that opens a browser of all of them. Never shrink them
below the target, and never show a count of the ones left out. Lands are read by
pressing them and are not targets of their own, so their row may draw them
smaller.

## Frame budgets

`just client gate` checks native work against per-board budgets. `--counts`
checks Callgrind instruction counts and `--gpu` checks shading and draw work.
Every budget has a reason in [`budgets.json`](../data/client/budgets.json). An
animation-only frame has a zero-allocation limit. `just client trend` shows the
saved measurements of gated boards from verified tips. The trend flags a rise
over 5% between measurements or over 10% within seven days.

## Where a change goes

Start with `just client new` for the available scaffolds and
`just client new <kind> --help` for the recipe. Keep a story beside the crate or
feature it exercises. The `guides` source check limits this guide to 150 lines
and each crate guide to 60. Use a card scenario for something the engine must
play, and a story for a client state you can supply directly.

The workspace and native hosts are under `client/`. The data and its
[guide](../data/client/AGENTS.md) are under `data/client/`. The CLI is in
`tools/src/beryl_tools/client/`. The Ignite adapter is
`tools/src/beryl_tools/ignite/client.py`.
