# Working in `rust/beryl3d`

This adds to the root `AGENTS.md`, whose rules all apply here.

Beryl 3D is the native 3D client of Beryl (the design is in `docs/beryl-3d.md`).
This directory is a Cargo workspace of its own, with its own `Cargo.toml`,
`Cargo.lock` and `target`, so that no check of the main workspace in `rust/`
builds it and a change here selects no lane of the main checks (decision C17).
Its crates depend on the main crates (`beryl_session`) by path. Its
`[workspace.lints]` table must equal the main one's, which the `lints-match`
check enforces.

## Crates

- `beryl_scene`: poses, layout and animation as pure data; no GPU, window, clock
  or thread (`scene-purity`). Its build script writes `data/beryl3d/space.json`
  and the `audit` object of `data/beryl3d/gates.json` into constants
  (`src/data.rs`).
- `beryl_render`: draws a scene with wgpu. The only crate that names `wgpu`.
- `beryl_assets`: decodes and packs card pictures for the renderer.
- `beryl_host`: the window and event loop (winit) that drive a scene and render
  it.
- `beryl_inspect`: reads a scene and reports it as text; it has no GPU
  dependency.
- `beryl_3d`: the client that ties the other five crates together.

## Commands

`just b3d` lists every command, and `just b3d <command> --help` explains one.
`just b3d lane` runs the checks of the `beryl3d` lane that the fast check runs
for a change here.

## Paths

Beryl 3D's files live only under `rust/beryl3d`, `data/beryl3d` and
`tools/src/beryl_tools/beryl3d`. A change outside them is a change to the main
project, and selects the main lanes.

## Data files

- `data/beryl3d/space.json`: the table's spatial values; its keys under `verbs`
  name verbs or cases of `data/ui-conventions.json` (`data-names`). Each value
  under `verbs.<name>` becomes an `f32` constant in `data::space::verbs::<name>`
  (`fly`'s `arc`, in millimetres), and each count under `catch_up` a `usize` in
  `data::space::catch_up`: more than `double` updates waiting make the script
  clock run twice as fast, more than `quadruple` four times.

- `data/beryl3d/gates.json`: the audit, frame, tier, material and test gates.
  Each number under `audit` becomes an `f32` constant in `beryl_scene`'s
  `data::audit`, named for its key in upper case:

  - `target_points` and `phone_target_points`: the side of the square an
    interactive card at rest must leave uncovered (the "reachable" check), on
    the desktop and phone layouts.
  - `line_end_points` and `line_reshape_points`: how far an arrow's end may move
    and still be at rest, and how far a control point must move to count as a
    change of shape (the "lines hold" check).

  Under `frames.<board>` are the limits `just b3d gate` holds each gated board's
  work counters, load and allocations to (docs/beryl-3d.md, "Finding the worst
  frame"). `just b3d gate --update` lowers them to what was measured. A limit
  rises only when the work itself changed (a new verb, a new element on every
  screen) or a compiler or dependency upgrade moved the count with no change to
  Beryl's code, and the commit that raises it gives the old and new numbers. It
  needs nobody's approval.

A break caused by a change to the main workspace (a renamed `beryl_session`
item, say) is fixed in the Beryl 3D change that meets it, not by holding the
main change back.
