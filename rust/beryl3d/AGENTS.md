# Working in Beryl 3D

Beryl 3D is Beryl's native client. See [Beryl 3D](../../docs/beryl-3d.md) for
the frame contract, rendering and hosts. See
[Beryl 3D architecture](../../docs/beryl-3d-architecture.md) for the client
core's crate boundaries, arrangement, layout profiles and checks.

## Client core

- Toolkit crates provide geometry, text, frames, widgets, layout, motion,
  effects, hit testing and probes. Give them plain values and opaque IDs.
- Game crates adapt the protocol to the toolkit. They own table placement,
  script playback, the HUD, the manual table and game probes.
- App crates supply the other screens and assemble the client. The host supplies
  time, input and assets. The renderer consumes the finished frame.

`just b3d map` lists the inventories. Give it a kind such as `crates`,
`features` or `containers` for names and source paths.
`just b3d map containers --full` shows where each profile's prompt tray is laid
out and which fitting policy it uses. `just b3d map data` lists the shared
timing file, and `just b3d new effect --help` shows how to register an effect.

Read the guide in the crate you change. At the same level a crate may depend on
earlier crates in `levels.json`. Dependencies between levels point down. Only
`app_client` depends on `scaffold_web`, which owns new web fit values. The
client core reads time and assets through its inputs. Given the same inputs it
draws the same frames, and an animation-only frame allocates nothing.

## Building and testing

This directory has its own Cargo workspace. Its lints match the main
workspace's. Use `just b3d cargo check -p <crate>` to type-check, and
`just b3d cargo-test -p <crate>` to build and run a crate's tests. Tests, the
recorder and the gate share a build in Cargo's test profile. Use these commands
to reuse it. Avoid `just b3d cargo build` and `cargo run`, which compile a
separate profile.

Put integration tests in `tests/all/main.rs` with one module per file. Tests
that install a global allocator have their own binaries. Programs that spread
work over threads use `beryl_inspect::runner::threads` for the core count Ignite
granted.

`ignite test <check>` runs the `b3d-*` families in `ignite/checks/beryl3d.toml`.
Use the smallest check that observes the change. `just b3d test` checks the
corpus and standalone scenes in the desktop window matrix. Use a test name
prefix for one scene or `--phone-only` for the phone windows. `just b3d record`
builds the corpus pack. `just b3d corpus --sweep` checks card spec moments for
signatures the corpus lacks.

## Seeing the result

`just b3d` lists the commands. `just b3d <command> --help` gives their options.
Use `list` and `info` to find a recording and its moments, `tree` and `diff` to
inspect what the player sees, and `check` to check one scene on every tick.
`tree` prints locators you can pass to `explain` or use in `--do` actions. The
[b3d skill](../../.agents/skills/b3d/SKILL.md) shows how to read their output.

For a change in how something looks, open the picture from
`just b3d png --scale 2` or a film of the affected span. Compare with the web
interface when checking parity. Record the picture's path and what you saw on
the ticket. The [web-match skill](../../.agents/skills/web-match/SKILL.md)
covers side-by-side comparisons.

For phone host changes, `just b3d ios-smoke` and `just b3d android-smoke` replay
a recording headlessly and save a screenshot and frame log under
`$BERYL_CACHE/b3d/phone`. Use the documented touch and lifecycle options for
host input and suspend or resume. Install on a paired iPhone with
`just b3d ios-device --user-asked` only when the user asks.

## Frame budgets

`just b3d gate` checks native work against per-board budgets. `--counts` checks
Callgrind instruction counts and `--gpu` checks shading and draw work. Every
budget has a reason in [`budgets.json`](../../data/beryl3d/budgets.json). An
animation-only frame has a zero-allocation limit. `just b3d trend` shows the
saved measurements of gated boards from verified tips. The trend flags a rise
over 5% between measurements or over 10% within seven days.

## Where a change goes

Start with `just b3d new` for the available scaffolds and
`just b3d new <kind> --help` for the recipe. Keep a story beside the crate or
feature it exercises. The `guides` source check limits this guide to 150 lines
and each crate guide to 60. Use a card scenario for something the engine must
play, and a story for a client state you can supply directly.

The workspace and native hosts are under `rust/beryl3d/`. The data and its
[guide](../../data/beryl3d/AGENTS.md) are under `data/beryl3d/`. The CLI is in
`tools/src/beryl_tools/beryl3d/`. The Ignite adapter is
`tools/src/beryl_tools/ignite/beryl3d.py`.
