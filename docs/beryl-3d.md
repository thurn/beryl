# Beryl 3D

*A design for a native Rust client that replaces Beryl's web interface and its
Tauri shell. Decided 2026-11-14 in a question-and-answer session with the
project's owner ("the design interview"), on a reading of the current client, a
survey of the Rust graphics and text crates as published that week, and
measurements taken that day on the development machine.*

Beryl is an app that plays Magic: The Gathering against an AI. Its rules engine,
its AI and the code that runs a game are Rust. Its interface is a web page,
written in TypeScript and React, shown in a WebView by Tauri on the Mac, iOS and
Android.

The web interface works, and it has costs that this design removes.

- **Frames.** The WebView drops frames on a loaded Mac with a high-density
  window, which is why the app already times its own frames and attaches the
  late ones to bug reports. How long a frame takes is decided by the browser's
  style, layout and paint stages, which Beryl can measure but not control.
- **Checks.** Every interface check runs in a headless browser. Browsers are
  slow to start and limited in number, so the full set of checks runs every
  three hours, not on every change, and 27,000 lines of test scripts drive them.
  From 11 to 14 November 2026 the browser checks took half of all the time the
  project's checks ran, and one check of one animation took 36 seconds at the
  90th percentile.
- **Phones.** The phone apps are the same page in a WebView, with Swift and
  Kotlin code around it for what a page cannot do.
- **Looks.** The table's perspective, shadows and lighting are imitated with CSS
  transforms and filters, because a page has no 3D scene.

**Beryl 3D** is a second client for the same game code, written in Rust, which
draws the table as a real 3D scene with the GPU and has no WebView, no
JavaScript and no Tauri. It is built beside the web interface, and replaces it
when the project's owner decides it is ready.

The requirements, most important first:

1. **Performance.** Smooth animation on very old hardware, low memory use, and
   no dropped frame on any board the engine can represent. The AI searches on
   background threads without disturbing the interface.
2. **Built for coding agents.** An agent such as Claude Code can build, drive,
   inspect and check the client from a terminal, with layout written in CSS
   terms and tests written in the style of Playwright.
3. **Deterministic performance tests**, in the manner of the engine's
   instruction-count gate (defined below).
4. **Deterministic interface tests** that enforce layout and motion rules on
   every state, fast enough that tens of thousands of them run in seconds before
   every push.
5. **Phones.** Native iOS and Android builds.
6. **Easy to program.** Expressive APIs, with good support for many screen sizes
   and for text.
7. **High-fidelity 3D visuals** (objects moving in space, shadows, lights),
   never at the cost of a frame.

The design in one paragraph: a library with no GPU, no clock and no threads in
it turns the game's updates into a list of things to draw, one frame at a time,
as a pure function of its inputs. A small renderer built on `wgpu` draws that
list. Thin platform-specific programs feed the library its input and its frame
times. Because the library is pure, a frame's cost can be counted exactly, every
moment of every recorded game can be checked for layout faults in microseconds,
and an agent can run the whole client from a command line with no window.

Some parts of Beryl, and a few terms, are used throughout and are defined here
once.

- **The session** is the Rust code that runs a game and tells a client what to
  show (`beryl_session`). This design changes it in one way: it sends the timing
  of each animation step (see [Timing a script](#timing-a-script)).
- **A view** is everything one player may see at one moment. **A seat** is one
  of the two players' places at the table; every view is from one seat.
- **A script** is the list of animation steps that turns the previous view into
  the next. The session sends a client a view and a script for every change. The
  steps of a script are grouped into **beats**: the steps of one beat play
  together, and the beats play in order.
- **A verb** is one kind of animation step. There are 16, such as `fly` (a card
  travels to a new place) and `vanish` (a card leaves with no flight). The list
  is closed.
- **A mark** is something drawn on a card to show its state in a prompt, such as
  an outline on a card the player may pick. There are 6.
- **A prompt shape** is one of about ten kinds of prompt the session can open,
  such as "pick one of these cards" or "divide this damage". Every prompt in the
  game is one of them.
- **A fixture** is a recorded session: the views, scripts and prompt replies of
  a real or scripted game, which a client can replay with no engine running.
  Fixtures have names, such as `verbs-zones` and `pay-default`.
- **A card spec** is a set of generated fixtures for one card, one for each
  moment the card produces in the interface (a prompt, a trigger, a token).
- **The count gate** is an existing check on the rules engine. It runs a fixed
  workload under Callgrind, which counts every CPU instruction executed, and
  fails when the count rises above a recorded limit. Counts are identical on
  every run, so the check has no noise.
- **MTG Arena** ("Arena") is Wizards of the Coast's own Magic client. Beryl
  copies its flow, timing, layout and wording, measured from recordings.
- **Scryfall** is the public card database Beryl downloads card images from.

## Related information

- [Engine architecture](engine-architecture.md), section 19.2 and decisions K22
  to K29: the session, views, scripts, the closed verb list, prompt state kept
  in the session, and Arena as the reference for the interface. Beryl 3D
  consumes all of it unchanged.
- [UI Conventions](ui-conventions.md): where Beryl differs from Arena on
  purpose. Generated from `data/ui-conventions.json`, which also holds each
  verb's timing values. Both clients read that data.
- [Benchmarking](benchmarking.md): the count gate, Callgrind in the
  `beryl-bench` Linux virtual machine, and timing under load. The performance
  gates here use the same tools.
- [Checks](checks.md): the fast check that runs before every push, the slow
  check that runs after, and their parts. Beryl 3D's checks are a lane and a
  sweep of their own (see [Checks and isolation](#checks-and-isolation)).
- [Manual multiplayer](manual-multiplayer.md): a rules-free table for two
  people. Its screens are part of what Beryl 3D must reproduce.
- `ui/AGENTS.md` and `rust/AGENTS.md`: how the web interface and the Rust crates
  are tested today. `rust/beryl3d/AGENTS.md` is the reference for work in Beryl
  3D's own workspace.
- [wgpu](https://docs.rs/wgpu/30.0.2/wgpu/),
  [taffy](https://docs.rs/taffy/0.15.1/taffy/) and
  [parley](https://docs.rs/parley/0.13.0/parley/): the three third-party crates
  the design leans on most.

## Decisions

The design interview settled the following. The rest of the document specifies
the client they describe, and later sections refer to them by number.

- **C1. Both clients stay fully alive until the switch.** The web interface
  keeps taking every fix and feature, and each is also made in Beryl 3D. The
  shipping app never stalls. The cost is that interface fixes are written twice.
  One part is moved so that it is written once: the rules for when each step of
  a script plays leave the web interface for the session, which sends each
  step's timing to both clients (see [Timing a script](#timing-a-script)).

- **C2. The performance floor is a 2015-class device**: an iPhone 6s, a 2015
  mid-range Android phone with an OpenGL ES 3.0 GPU, a 2013 laptop with
  integrated graphics. This document calls that class of device **the floor**. A
  card table drawn plainly needs nothing newer.

- **C3. Screens are a retained tree of named elements laid out with flexbox and
  grid.** Tests and agents drive the client inside the test process, with
  locators, presses and printouts of the tree. These are the patterns coding
  agents have the most practice with, and a tree of named elements lets one
  generic pass enforce layout rules.

- **C4. The table is a real 3D scene with one camera of fixed orientation**,
  fitted to Arena's recordings. Prompts, menus and text are a flat layer over
  it. Shadows and lights are only correct when cards have real positions, and a
  real camera matches Arena more closely than the web interface's imitation.

- **C5. No check compares whole pictures, and there is one renderer.** Interface
  rules are checked on each frame's numbers. A second renderer for reference
  images would double every shader, and a stored image fails on every intended
  visual change. A few renderer tests sample single pixels of known scenes.

  The owner amended this on 2026-11-16 for one kind of comparison: a check may
  compare Beryl 3D's frame with the web interface's screenshot of the same
  moment, held by a ratchet file (see "Matching the web interface's look"). The
  web interface as WebKit draws it is the reference, so there is still one
  renderer of ours.

- **C6. A card face is composed once into one texture.** It is held at a small
  size for every card in the match and at full size for the few being read, in a
  compressed GPU format. A card's face is then one textured shape, and a match's
  faces take at most about 32 MB instead of about 500 MB.

- **C7. Phones run thin native programs in Swift and Kotlin; the desktop uses
  `winit`.** All of them, and a windowless one for tests, drive the client
  through one interface. `winit`'s Android text entry is an unmerged draft, and
  frame pacing on phones is best owned natively.

- **C8. Two visual tiers, Baseline and Enhanced, that differ only in shading.**
  The tier is chosen by a measurement at startup and steps down for the rest of
  the session when frames run late. A tier that could move things would need its
  own layout checks.

- **C9. Search threads use every core at a lower scheduling priority than the
  thread that draws.** No core is reserved. Drawing needs about a tenth of one
  core at worst, and reserving a core would cost 30 to 50% of search throughput
  on a phone. The search runs in the default class, the web app's, so that it
  searches as fast as the web app does. Over 40 whole games on a loaded Mac, the
  lower utility class neither sped up drawing nor measurably slowed the search.

- **C10. The performance gate holds the worst single frame as well as the
  total**, over recorded games and over hand-built boards at the engine's
  capacity. A total hides one slow frame, and recorded games never reach the
  boards where frames would drop.

- **C11. The project's owner decides when Beryl 3D replaces the web interface,
  and when the old code is deleted.** A generated list showing that Beryl 3D
  covers everything the web interface does is necessary and not sufficient. This
  is a deliberate exception to Beryl's rule that plans do not wait for approval.

- **C12. Beryl writes its own layers over `wgpu` (GPU), `taffy` (layout) and
  `parley` (text).** It uses no game engine and no UI toolkit. Nothing but
  Beryl's own code runs in a frame that only animates, so that code can be
  counted, kept free of allocation and gated.

- **C13. GPU performance is held by counts alone.** The checks count the
  fragments a frame shades and the work each shader does for one. No 2015-class
  phone is bought or attached to confirm them. A counted number is a stand-in
  for device performance: where the two are found to disagree, the device is
  right, and no design choice is made to suit a count at the device's expense.

- **C14. Interface tests are fast enough to run all of them before every push.**
  A test costs about a millisecond, because it checks the moments at which
  something can go wrong instead of every tick, and never runs the engine.
  Slower, exhaustive checking runs after the push. Interface checks are half of
  the project's check time today, and the full set runs only every three hours.

- **C15. Default to good performance.** Where a choice trades smoothness for
  something else, the faster choice is the default and the other is opt-in. The
  desktop app therefore starts fullscreen, as Arena does, and a key leaves
  fullscreen. On macOS that is a window over the whole display that the window
  server composes as usual, not a fullscreen Space: on a ProMotion display a
  fullscreen Space's frames came late at every rate down to 30 Hz under load,
  while the composed window held 120 Hz (see [Measurements](#measurements)). The
  frame log records the window mode, so the choice keeps being tested.

- **C16. An agent measures frame delivery without a person.** Every number a
  person was needed for in the prototype can be produced by a run an agent
  starts: input comes from a script played on the display's clock, load from a
  load generator, and results go to a frame log file an agent reads. Before
  this, finding why a window dropped frames took the project's owner moving the
  pointer and pasting terminal output back, several times.

- **C17. Beryl 3D's checks are its own.** Beryl 3D is a separate program in a
  Cargo workspace of its own. A change to it runs only Beryl 3D's checks, and a
  change to anything else runs none of them and builds none of its code. The
  project's owner asked for this on 2026-11-14: the rest of the project lands
  about eight changes an hour, and none of them may wait for, or be failed by, a
  client that is not yet the app. The cost is that a change to the session can
  break Beryl 3D without its author being told. A sweep finds that afterwards,
  and Beryl 3D's own work fixes it (see
  [Checks and isolation](#checks-and-isolation)).

- **C18. The highest frame rate the display and device can hold, but smooth
  motion comes first.** The project's owner decided this on 2026-11-15. Every
  host asks for the display's highest refresh rate, and a frame counts as late
  against the rate the host chose. Smooth animation is mandatory, so an even
  lower rate beats an uneven higher one. When frames miss the chosen rate, the
  renderer first drops the Enhanced tier to Baseline (C8). If frames still miss
  it, the host steps down to the next rate that divides the display's (120 to 60
  Hz) and stays there until a sustained run of frames shows room to step back
  up. The thresholds are data, not literals in code. Being shown at 60 Hz on a
  120 Hz display without asking for it is a fault, not a step down. The late
  frames the measurements first took for 60 Hz in fullscreen on macOS came from
  the fullscreen Space, and the desktop app no longer uses one (C15).

## What does not change

Beryl 3D is a second player of scripts the session already writes. Nothing in
the engine, the rules, the AI or the session changes for it.

- **The session's protocol**, with one addition. Beryl 3D receives the same
  `GameUpdate` values (a `PlayerView`, which carries its `Script`) and sends the
  same commands (`choose`, `click`, `cancel`, `undo`, `pass_on`, `concede`,
  `set_stops`). It links `beryl_session` directly, so the values cross a channel
  as Rust types with no serialization. The addition is that a script carries the
  timing of each of its steps, which both clients play.
- **Scripts and verbs.** The client plays scripts and works out nothing about
  the game: the script says what happened, and how each step is timed. A
  script's steps name verbs from the closed list, and the client has one
  implementation of each.
- **Prompt state.** A prompt's state lives in the session. Each click is a
  command, and the reply says which marks to draw and which buttons are enabled.
- **Arena as the reference** for flow, order, timing, table layout and wording,
  and the numbered exceptions in UI Conventions. Beryl keeps its own colors,
  fonts, icons and card frames.
- **Fixtures and card specs.** Both clients replay the same recordings.
- **Saves and replays.** A game started in one client resumes in the other.

One script step, as both clients receive it when the viewer casts a spell from
their hand:

```rust
Step {
    act: Act::Fly { from: ZoneName::Hand, object, item: Some(spell), .. },
    cause: Some(Subject::Player { player: Side::You }),
}
```

The web interface turns this into a CSS transform on a DOM node. Beryl 3D turns
it into a path through 3D space for one object in its scene. Neither client
works out what happened by comparing two views: the step says it.

## Components

Beryl 3D is seven crates and two native programs for phones. Each crate has one
job, and each depends only on crates below it in this list.

The seven crates are a Cargo workspace of their own, `rust/beryl3d`, with its
own lock file and build directory (decision C17). They reach the game through
path dependencies on five crates of the main workspace: `beryl_protocol`, the
wire types (views, scripts, prompts and fixture messages) with no rules or
cards; `beryl_card_records`, which builds the card records those types name from
the card store, with no engine or cards; `beryl_session`, which plays a game and
builds those types; the fixture recorder in `beryl_cli`; and `beryl_art`, the
image download queue and disk cache that the Tauri shell also uses. Only
`beryl_3d` and `beryl_record` depend on `beryl_session` or a card crate, so an
edit to the engine rebuilds no other crate or test binary (`scene-purity` checks
their manifests).

- **`beryl_3d`**, the app. It connects the session, the client core, the
  renderer, the background workers and audio. It is built as an executable on
  the desktop and as a library for the phone programs, and it exports the C
  functions those programs call.
- **`beryl_host`**, the platform boundary. It defines the interface between the
  client and a platform (see the [Hosts](#hosts) section below), and contains
  the windowless and desktop implementations.
- **`beryl_render`**, the renderer. It draws a frame with `wgpu`. It is the only
  crate that names a `wgpu` type.
- **`beryl_assets`**, everything prepared off the drawing thread: decoding the
  card images that `beryl_art` downloads, composing card faces, compressing
  textures and rasterizing glyphs. Listing a match's faces from its decklists
  needs the card store, through `beryl_card_records`, so that function is behind
  the `decklists` feature, which `beryl_pictures` leaves off.
- **`beryl_record`**, the `record` program, which plays the engine through
  `beryl_cli`'s fixture and spec recorders to build the file of recordings when
  it is missing. It is the only crate of the workspace that plays the engine
  apart from the app, so that the text commands do not link it.
- **`beryl_inspect`**, the tools that play recordings with no window and no GPU:
  the file of recordings, the interface test runner, the frame benchmarks, and
  the commands of `just b3d` that print text (see
  [The agent interface](#the-agent-interface)). It depends on the client core
  and `beryl_protocol`, and on no GPU, engine or card crate, so it rebuilds,
  links and starts quickly after an edit to the client core.
- **`beryl_pictures`**, the commands of `just b3d` that draw pictures, `png` and
  `film`, as the program `b3d-render`. It depends on `beryl_inspect`, the
  renderer and the windowless host, and like `beryl_inspect` on no engine or
  card crate.
- **`beryl_scene`**, which this document calls **the client core**. It holds the
  3D table, the element tree, animation, layout, hit testing and the interface
  checks. It depends on `taffy`, `parley` and `beryl_protocol` (plain data:
  views, scripts, commands), and on nothing that touches a GPU, a clock, a
  thread or a file. It defines `Input` and `Request`, the types everything above
  it speaks to it in.

The boundary that matters most is around the client core. It has three entry
points, and everything a frame shows is a function of what has passed through
them:

```rust
impl Client {
    /// Input: a session update, a pointer or key event, a new window size.
    pub fn handle(&mut self, input: Input);
    /// Compiles what the frame at `now` needs of the scripts to come.
    pub fn prepare(&mut self, now: u64);
    /// The frame at time `now`, in nanoseconds since the client started.
    pub fn frame(&mut self, now: u64) -> &Frame;
}
```

`handle`, `prepare` and `frame` never read a clock, start a thread, take a lock
or touch a file. Given the same sequence of calls, a client executes the same
instructions and returns the same frames. The [Determinism](#determinism)
section below lists what keeps that true.

The two entry points divide the work by how often it happens.

- **`handle` does what an input changes.** It applies a session update, rebuilds
  a screen whose inputs changed, lays out elements and text, and starts
  animations. This is where `taffy` and `parley` run. It may allocate.
- **`prepare` does the part of what time changes that allocates.** A host calls
  it before each `frame`, with the same `now`. It compiles the steps of the
  playing script that the frame at `now` needs, and a few more (see
  [Threads](#threads)), and lays out the floats of any life total, pool or
  notice those steps add. `frame` calls it too, for a caller that does not, and
  a second call for the same `now` does nothing.
- **`frame` does what time changes.** It computes where every animated thing is
  at `now` and writes the frame. Only Beryl's code runs here, and it allocates
  nothing.

## The frame

A **frame** is what the client core hands the renderer and the checks for one
moment in time. It has two parts, built together.

- **The resolved elements**: every named thing on screen with the numbers that
  describe where it is. The interface checks and the agent tools read this part.
- **The draw list**: a flat array of fixed-size records, one per shape to draw,
  already in drawing order. The renderer reads this part.

A resolved element carries what a check or a locator needs:

```rust
pub struct Resolved {
    pub id: ElementId,
    pub parent: u32,           // index of its parent in the array; u32::MAX
                               // at the root. Parents come before children.
    pub role: Role,
    pub name: NameRef,         // "Counterspell", "Auto Pay"
    pub place: Place,          // a card's zone and row; Place::Layer for an
                               // element of the flat layer
    pub corners: [Vec2; 4],    // on screen, in logical points
    pub depth: f32,            // nearer the camera is smaller
    pub opacity: f32,
    pub clip: Option<Rect>,
    pub interactive: bool,
    pub state: StateBits,
    pub face: Option<u16>,     // a card's face layer; None while it has no
                               // face of its own and is drawn blank
    pub really: NameRef,       // the card a copy really is; empty otherwise
}

/// Closed. A new role is added here with the word the tree printout uses.
/// An arrow's four corners are the control points of its cubic Bezier
/// curve: its start, two control points and its end.
pub enum Role {
    Card, Ability, Pile, Player, Region, Button, Text, Icon, Field, Toggle,
    List, MenuItem, Arrow,
}

/// One bit each, in this order from bit 0.
pub struct StateBits(pub u16);
// TAPPED, FACE_DOWN, MOVING, HOVERED, HELD, ELIGIBLE, SELECTED, TARGET,
// DISABLED, FOCUSED
```

The frame itself, and what `Client` says about when it next needs one:

```rust
pub struct Frame {
    pub resolved: Vec<Resolved>,  // capacity set at startup, cleared each frame
    pub names: String,            // every resolved element's name, end to end
    pub draws: Vec<Draw>,         // in drawing order
    pub opaque: u32,              // draws[..opaque] are the opaque records
    pub layer: u32,               // draws[layer..] are the flat layer's
                                  // records: no depth test, not shaken
    pub view_projection: Mat4,    // the camera, for every record
    pub clips: [Rect; 16],        // physical pixels; entry 0 is not used
    pub clips_full: bool,         // a sixteenth clip was asked for: a fault
    pub shake: Vec2,              // physical pixels, see Effects
    pub sounds: Vec<Sound>,       // the sounds that begin in this frame, each
                                  // with whether it is the opponent's
    pub lines: Vec<TextLine>,     // each laid-out line of text: its element's
                                  // index and its box, for the "text fits" check
    pub load: Load,               // draw calls, instances and upload bytes;
                                  // `Frame::areas` measures the screen areas
    pub work: Work,               // elements visited, segments evaluated,
                                  // layout nodes computed, records written
}

pub enum Wake { Now, At(u64), Never }

impl Client {
    /// When the next frame is needed, as of the last call to `frame`.
    pub fn wake(&self) -> Wake;
    /// The next thing the client asks of the platform, if any.
    pub fn next_request(&mut self) -> Option<Request>;
}
```

The small types those records name:

```rust
/// A card's element id is its object's slot in the view. A flat-layer
/// element's is given when it first enters the tree, and kept while it stays.
pub struct ElementId(pub u32);

/// A range of `Frame::names`, so that a resolved element owns no string.
pub struct NameRef { pub start: u32, pub len: u32 }

pub struct Load {
    pub draw_calls: u32,
    pub instances: u32,
    pub upload_bytes: u32,
}

pub struct Areas {            // `Frame::areas`, measured when asked
    pub opaque: f32,          // in screens
    pub blended: f32,         // in screens
}

pub struct Work {
    pub elements: u32,        // elements visited
    pub segments: u32,        // segments evaluated
    pub layout_nodes: u32,    // layout nodes computed, in `handle`
    pub text_runs: u32,       // runs of text shaped, in `handle`
    pub records: u32,         // draw records written
    pub placed: u32,          // objects placed, mostly compiling scripts in `handle`
    pub steps: u32,           // script steps compiled, mostly in `handle`
}
```

A draw record is 80 bytes, so 1,000 of them are 80,000 bytes written in order:

```rust
#[repr(C)]
pub struct Draw {
    pub model: [f32; 12],   // a 3 by 4 transform, world space     (48 bytes)
    pub uv: [u16; 4],       // by material, see the table below     (8 bytes)
    pub layer: u16,         // which layer of the texture array     (2 bytes)
    pub material: u8,       // index into the closed material list  (1 byte)
    pub flags: u8,          // texture array, mesh and clip         (1 byte)
    pub tint: [u8; 4],      // color and opacity, not premultiplied (4 bytes)
    pub params: [f32; 4],   // by material, see the table below    (16 bytes)
}
```

The prototype's record was 64 bytes and had no field to spare: a card's back
layer had to go in its unused `uv`. The flat layer and the effects need more per
shape (a panel's size and corner radius, how far a dissolve has gone), so the
record has four more numbers, `params`.

The bits of `flags`:

| Bits   | Meaning                                                                                                                                                                                             |
| ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0 to 1 | The texture array: 0 the small card faces, 1 the full-size card faces, 2 the atlas.                                                                                                                 |
| 2 to 3 | The mesh: 0 the card (a rounded rectangle), 1 the card's edge strip, 2 the unit quad.                                                                                                               |
| 4 to 7 | The clip: 0 for none, or 1 to 15 for an entry of the frame's `clips` array, a rectangle in physical pixels that the renderer sets as the scissor. A frame that needs more than 15 clips is a fault. |

What `uv`, `layer` and `params` hold for each material. A field not named is
zero. The numbers in the first column are the values of `material`:

| Material         | `uv`                             | `layer`    | `params`                                                          |
| ---------------- | -------------------------------- | ---------- | ----------------------------------------------------------------- |
| 0 table          | unused                           | unused     | unused                                                            |
| 1 card           | `uv[0]` is the back's layer      | the face   | `[dissolve, whiten, 0, 0]`, each from 0 to 1                      |
| 2 contact shadow | unused                           | unused     | `[blur in millimetres, 0, 0, 0]`                                  |
| 3 glow           | the sprite's rectangle in texels | atlas page | `[progress from 0 to 1, 0, 0, 0]`                                 |
| 4 streak         | the sprite's rectangle in texels | atlas page | `[head from 0 to 1, tail length from 0 to 1, 0, 0]`               |
| 5 panel          | the border's color, as RGBA      | unused     | `[width, height, corner radius, border width]`, in logical points |
| 6 glyph          | the glyph's rectangle in texels  | atlas page | unused                                                            |
| 7 icon           | the icon's rectangle in texels   | atlas page | unused                                                            |
| 8 particle       | the sprite's rectangle in texels | atlas page | `[age from 0 to 1, seed, 0, 0]`                                   |

- **A rectangle in texels** is `[x, y, width, height]` within one atlas page.
- **`dissolve`** is how far a card has broken up (the `dissolve` effect), and
  **`whiten`** how far its face is replaced by white light (`settle`, `blanch`
  and `cover`). A card with either above zero, or with a `tint` opacity under
  255, is drawn with the blended records.
- **A new material or a new meaning for a field** is added to this table in the
  same change as the code. No material may read a field this table does not give
  it.

The rules for building a frame:

- **No allocation.** The resolved elements and the draw list are arrays the
  client owns and clears each frame. They are sized at startup for the largest
  board the engine can hold. After a game has started, `frame` performs zero
  heap allocations; a frame that outgrew an array would grow it, and the
  allocation gate would fail.

- **A card has two layers.** A card that turns over shows its face from one side
  and its back from the other, so its record must name two layers. A card's face
  always fills its layer, so a card record has no use for the `uv` rectangle,
  and `uv[0]` holds the layer of its back. The shader picks the layer by which
  side of the triangle faces the camera, and mirrors the texture coordinates on
  the back. The prototype confirmed this works with one mesh and no second
  record.

- **Three texture arrays.** A record's `flags` name one of them: the small card
  faces, the full-size card faces, or **the atlas**, which holds everything else
  (glyph pages, icons, and the shapes of shadows and glows). The atlas is an
  array of single-channel pages, each 1,024 texels square, that hold coverage:
  page 0 holds the shapes of light, which every host uploads before its first
  frame, and the glyph worker fills the pages after it with glyphs and icons.

- **Icons are rasterized on the glyph worker.** Each icon of `widgets::icons` is
  kept in `beryl_assets` as the web interface's own SVG path data, and
  `GlyphWorker::icons` fills and strokes every one with `tiny-skia` at the
  display's scale, at the size the web draws it, into a square cell. It sends
  the cells as an `AssetReady::Icons`, and the client keeps a table from an icon
  and its size in points to its cell. One channel holds one color, so where the
  web paints an icon in two colors (the phase icons' dark fills under cream
  outlines), the darker fill's coverage is scaled by its brightness. When the
  scale changes, the host asks again: the new cells go into fresh space, and the
  old ones are drawn scaled until they arrive.

- **One projection.** Every record's transform is in the table's space. A
  flat-layer record's transform places it on a plane just in front of the
  camera, so the same camera matrix serves the table and the flat layer.

- **Closed materials.** A **material** is one way of shading a shape. The list
  is closed and short: table, card, contact shadow, glow, streak, panel, glyph,
  icon and particle. A record's `material` selects a branch of a shader, so
  shapes of different materials can be drawn by one call. Every GPU pipeline is
  created at startup, and nothing is compiled during play.

- **Opaque first, nearest first.** Cards and the table are opaque. Their records
  come first in the list, cards nearest the camera first and the table last, and
  are drawn with a depth buffer. A pixel of the table that a card covers, or of
  a card that a nearer card covers, is then never shaded. A card that is partly
  transparent, as one fading out is, is drawn with the blended records instead.
  The [Rendering](#rendering) section below says why this matters on old GPUs.

- **Then blended, in a fixed order of bands.** Everything translucent follows,
  farthest first. Within a band of the table, a record is then moved earlier to
  join records drawn with the same mesh, texture array and clip, past only
  records it does not overlap on screen, so that a fading card among marks does
  not cut the band into many draw calls. Records that share no pixel blend the
  same in either order. A **band** is a group of records drawn together. The
  bands, in order:

  1. contact shadows of cards above the table;
  2. the soft edges of cards, and the marks on cards lying on the table;
  3. marks on cards above the table (in flight, in a hand, on the stack, held up
     to be read);
  4. glows and streaks;
  5. particles, which only the Enhanced tier draws, tested against the depth
     buffer like the bands before them;
  6. the flat layer, in the order of its tree;
  7. cards shown inside flat-layer elements, such as a card browser, with their
     marks;
  8. flat-layer elements that must cover those cards, such as a tooltip.

  `Frame::bands` gives the index in the draw list where each band starts, so
  that a pass can draw one band's records again, as bloom does the glows.
  `Frame::tier` is the tier the frame is drawn in. The two tiers' draw lists are
  the same but for contact shadows, which only Baseline writes, and particles,
  which only Enhanced writes.

- **Marks are hidden with their cards.** Blended records on the table are tested
  against the depth buffer, so a mark on a card that another card covers is
  covered too. Every card in a row lies 0.3 millimetres above the one before it,
  whether or not the row overlaps, because a tapped card is wider than the step
  between untapped ones and would otherwise share a height with its neighbour
  and flicker where they overlap (the prototype showed this). Which card is on
  top is therefore a matter of position, never of drawing order. The flat layer
  is drawn without the depth test.

- **Few draw calls.** Consecutive records that use the same mesh and texture
  array are one instanced draw call, whatever their materials. The table and a
  shadow can be the card mesh scaled, its rounded corners far off screen or
  inside the shadow's blur, so in the prototype every opaque record was one call
  and every blended record another: two calls a frame on every board. Every
  blended record outside a card face samples the atlas, so a whole band is one
  call. The flat layer's quads are a third call, cards at full size a fourth,
  and each clip adds two, so a frame is normally two to six calls.

- **Load counted as it is built.** While it builds the list, the client core
  adds up numbers that predict GPU load: draw calls, instances and bytes to
  upload. The screen area of its opaque and of its blended records, each divided
  by the screen's area, is measured only when a gate or test asks for it
  (`Frame::areas`): the client itself never reads it, and measuring every record
  took about a fifth of a stress board's frame between scripts. The
  [Performance gates](#performance-gates) section below holds these to limits,
  and counts the pixels actually shaded. A record's area is the rectangle that
  bounds its mesh, cut to the view volume. The one-pixel edge strip counts as no
  area, and clip rectangles are not taken into account.

- **Ordered once, without allocating.** Records may be pushed in any order;
  `FrameBuilder::finish` sorts them by band, then by depth, then by the order
  they were pushed, with keys held in a scratch array the frame keeps. The
  frame's arrays are sized at startup for the engine's largest board, 1,024
  objects (`frame::DRAWS`, `ELEMENTS` and `NAME_BYTES`; `Config::LARGEST`).

### Drawing only when something changes

The client draws a frame only when the picture would differ from the last one. A
still table costs no CPU and no GPU, which matters most for a phone's battery
and heat.

- After each `frame`, the client reports when it next needs one: now (something
  is animating), at a given time (for example when the turn banner is due to
  disappear) or never (wait for input).
- The platform program asks for display refresh callbacks only while the answer
  is "now". The phone hosts pause their display link or frame callback when
  `beryl3d_frame`'s wake says at a time or never, and start it again at that
  time or when the library calls their wake callback.
- Any input ends the wait. A session update or a finished asset counts: the
  thread that sends one also wakes the host's event loop.

## Determinism

The performance gates and the interface checks both depend on the client core
doing identical work on every run. These rules keep it so, and a source check
enforces the ones that can be seen in source.

- **Time is an argument.** The client core never reads a clock. The platform
  passes `now` to `frame`, tests pass whatever times they like, and the same
  times give the same frames.
- **An input has no time of its own.** Animation that an input starts (a script,
  a card lifting under the pointer) begins at the `now` of the next `frame`
  call.
- **Positions are functions of time.** Where an object is at time `t` is
  computed from the animation segments it holds, never by adding a step to its
  position in the previous frame. A late frame shows a later position and no
  error accumulates. Any moment of any fixture can be drawn directly, without
  playing up to it.
- **No hashing with a random seed.** `std::collections::HashMap` seeds itself
  per process, which changes instruction counts between runs. The client core
  uses arrays indexed by id, sorted vectors and fixed-seed hashers.
- **Third-party code is held to the same test.** `taffy` and `parley` run inside
  `handle`. The performance gate runs each workload twice and requires identical
  counts, so a dependency that varies between runs is caught the day it is
  added. The work that dependency does then moves to `beryl_assets`, outside the
  counted boundary, or the dependency is replaced. Both were measured before any
  code was built on them (2026-11-15): a row of one text and three buttons laid
  out by `taffy` 0.15.1, and "Pay {1}{U}" and a 200-character rules text shaped
  by `parley` 0.13.0 with a fresh and with a warmed cache, gave identical
  instruction counts under Callgrind on three runs out of three. `fontique` and
  `parley` keep `hashbrown` maps with a per-process hash seed inside them; the
  counts did not vary, so the rule against seeded hashing binds Beryl 3D's own
  code and the gate holds the dependencies.
- **No threads, locks or files.** Work that needs them lives in `beryl_assets`,
  and its results arrive as inputs.
- **No system fonts.** Text is laid out with the fonts bundled in the app
  (Cinzel, Crimson Pro, Open Sans and the mana symbol font), with system font
  fallback switched off, so a line breaks in the same place on every machine.
  The web interface takes these fonts from npm packages, as WOFF2 files that the
  Rust text crates cannot read. Beryl 3D commits the same families as TrueType
  files, with their licences, in `rust/beryl3d/crates/beryl_scene/fonts`, and
  embeds them in the client core at build time. Where a family has a variable
  font file, that file is committed instead of separate weights: `parley`
  selects a weight from it (Open Sans at weight 700 keeps the glyphs of weight
  400, with the variation coordinates of 700 and wider advances), and the
  rasterizer draws each glyph run with the run's variation coordinates.
- **Seeded effects.** A particle effect's randomness comes from a fixed
  generator seeded by the object's id and the step's index.
- **Data is compiled in.** The client core's data files (spatial values and the
  checks' thresholds, in `data/beryl3d/`) are read by its build script, which
  writes them out as constants. Nothing is parsed when the app starts, and the
  client core needs no JSON crate.

A **pose** is where one object is and how it is turned: position, rotation,
scale, opacity and how far its face has turned over. A **segment** is one
stretch of animation between two poses:

```rust
pub struct Segment {
    pub start: u64,        // nanoseconds
    pub duration: u64,
    pub curve: Curve,      // a cubic Bezier easing, from the verb's data
    pub from: Pose,
    pub to: Pose,
    pub arc: f32,          // how far the path rises above the straight line
    pub shape: Shape,      // the path: `Line`, `Fading`, `TurnedOver` (flip),
                           // `Grown` (showcase), `Bowed` (sideways on the
                           // screen, leaning), `Thrown` (up the screen and down)
                           // or `Swept` (past a place without stopping)
    pub turning: Option<Turning>, // when the card turns over, top over bottom,
                           // as shares of its time: edge-on at `at`, turning
                           // from `at - each` to `at + each`
    pub step: Option<StepTime>, // the step that made it and whether it keeps
                           // to the step's start or its room, so that a
                           // script laid out again as it starts moves it
}

impl Segment {
    // A straight segment with no rise. Other code changes only the fields it
    // needs, `Segment { arc, ..Segment::between(..) }`, so a new field or
    // `Shape` breaks none of it.
    pub const fn between(from: Pose, to: Pose, over: Interval, curve: Curve) -> Self;
}

fn pose_at(segments: &[Segment], t: u64) -> Pose { /* pure */ }
```

Two cases of changing targets are specified, because they are where animation
code usually goes wrong.

- **A new view arrives while a script is playing.** It waits, and takes effect
  when the current script ends: its script plays then, and its prompt and
  buttons appear once that script has played out too, as the web interface's
  prompt opens when its view has played out. Scripts never overlap.
- **The window changes shape while a card is moving.** Every resting position is
  recomputed for the new shape. A segment in progress keeps its start time and
  duration and takes the new end pose. Its start pose is recomputed so that the
  object's pose at the current time is unchanged, so the card does not jump and
  arrives at the right place. A bowed or thrown path is worked out again from
  the new ends and camera before its start pose is.

### Motion the pointer drives

Three motions follow the pointer and not a script. Their values are in
`data/beryl3d/space.json`, under `pointer`. They start as copies of the
constants in the web interface's `ui/src/stage/raised.ts` and `heft.ts`.

- **Where the pointer is, is state. When it moved is not.** `handle` stores the
  pointer's latest position and phase. `frame` finds what the pointer is over
  with a hit test on the frame it is building, because a card can move under a
  pointer that is still.
- **A hand card rises under the pointer.** When the card under the pointer
  changes, the card it left gets a segment back down to its place in the hand
  (160 milliseconds) and the card it entered gets a segment up (100
  milliseconds). Both ease out, and both start at that frame's `now` from the
  pose the card has then; a card part way up or down takes that part of the
  time. Raised, the card stands upright, `raise_scale` times as large about its
  foot and `raise_mm` higher, and is moved toward the eye so that it covers the
  rest of the hand without changing its place on the screen. A card that a
  script moves is drawn where the script has it, and is hovered again only once
  it has landed. A touch never hovers.
- **The hit test finds hand cards where they rest in the fan.** While a card is
  up, the hover test puts its resting box in place of its raised one, as the web
  interface's hand does, so that the part of the fan a raised card has left
  still finds that card and the pointer does not flicker between two cards. The
  frame lists these resting boxes (`Frame::lifted`), and the "reachable" check
  judges a card the pointer has up at its resting box too, since a press on a
  card that the raised card covers first moves the pointer there.
- **A held card follows the pointer exactly.** A card is held from a press on it
  until the release, if the session says it may be picked up. Its position is
  the point under the pointer on a plane above the table, at the held height in
  the data. That is a function of the stored pointer position alone.
- **A held card leans by a spring, advanced in fixed steps.** The card tips
  toward the way it is being moved and settles flat when it stops. This is the
  one piece of animation state that is integrated instead of computed from a
  time: a damped spring driven by the pointer's speed. `frame` advances it in
  steps of 1/240 second from the previous frame's `now` to this one's. The
  pointer's speed is the distance it moved between the two frames divided by the
  time between them, and a gap longer than 1/20 second counts as 1/20. The lean
  depends only on the sequence of inputs and frame times, so a test that passes
  the same sequence gets the same lean, and the feel is the same at 60 and at
  120 frames a second.
- **Letting go.** On release the client sends the session what the gesture
  means, as the web interface does in `ui/src/stage/pointer.ts` and `prompt.ts`
  (a `Click::Drop`, or a click on the card). The card gets a segment from its
  held pose to its resting place. If the session's reply moves the card, that
  script's flight starts from the held pose instead.
- **A touch screen has one active finger.** The first finger down is the only
  one the table hears. A second finger landing within 80 milliseconds of it
  makes a multi-finger tap, which ends the first finger's press with nothing
  done; one landing later is ignored. A tap on a card the prompt can use uses
  it, and a tap on any other card, the hand's included, shows it whole until the
  next touch. A finger held still for `long_press_ms` shows its card whole, or
  opens the manual table's menu, and its lift does nothing. A hand card can be
  carried off as soon as the finger moves; on the manual table a finger rests
  `touch.pickup_ms` first. A double tap on bare felt, or with two fingers
  anywhere, opens the bug report. The timings are under `pointer.touch`, and
  `beryl_scene`'s `pointer::touch` keeps the fingers' state.
- **The checks treat a held card as moving.** Its resolved element has the
  `HELD` state bit, and the checks on cards at rest skip it.

Floating-point results are not required to match bit for bit between a Mac, a
Windows PC and the Linux virtual machine. Counts are taken on one machine, the
Linux virtual machine. The interface checks compare positions with a tolerance
of a hundredth of a logical point, and every threshold in a check has a margin
wider than that.

## Hosts

A **host** is the program that owns the window, the GPU surface, input and the
display's refresh on one platform. The client core never knows which host it is
under.

Every host does the same four things: it turns platform events into `Input`
values, it calls `frame` when the display is ready for one, it hands the frame
to the renderer, and it carries out the few requests the client makes of the
platform.

```rust
pub enum Input {
    Update(Box<GameUpdate>),                 // from the session
    Cards(Box<CardRecords>),                 // the match's cards, listed as faces
    Surface { size: Size, scale: f32, safe_area: Insets },
    Pointer(Pointer),                        // mouse, touch or pen
    Key(Key),
    Text(TextChange),                        // typed, composed or a phone field's whole text
    Paste(String),                           // the clipboard's text, for Request::Paste
    Assets(AssetReady),                      // a face or a glyph page is ready
    Lifecycle(Lifecycle),                    // suspended, resumed, low memory
    Tier(Tier),                              // Baseline or Enhanced
    Reply(Reply),                            // the session's answer to a command
    Recordings(Vec<String>),                 // a tools build's recordings, for the testing screen
}

/// What the client asks of the session: one variant for each function of
/// `beryl_session::Sessions`, or of the manual table's `TableConnections`, that
/// a player's action calls. Each carries that function's own argument types.
pub enum SessionCommand {
    Start(StartRequest),
    Resume,
    Choose(ChooseRequest),
    Click(ClickRequest),
    Cancel(DecisionId),
    Undo(DecisionId),
    PassOn(DecisionId),
    SetStops(SessionId, StopsSetting),
    Concede(SessionId),
    TableSolo(TableSoloRequest),             // start a solo manual table
    TableAct(TableAction),                   // change the manual table
    TableLeave,                              // close the manual table
    CardSearch(String),                      // the card store's tokens, for the token search
}

/// The session's answer. `Sessions::click` returns the prompt's next state
/// (`ClickReply`); every other function returns nothing or an error.
pub enum Reply {
    Click(Result<ClickReply, SessionError>),
    Done(Result<(), SessionError>),
    TableSolo(Result<SessionId, TableRefused>),
    TableAct(TableActReply),
    TableLeave(Result<(), String>),          // the reason the table gave
    CardSearch(String, Vec<CardSearchHit>),  // the tokens found, with the text asked for
}

impl Client {
    /// The next command for the session, if any. The host sends it to the
    /// session thread and passes the answer back as `Input::Reply`.
    pub fn next_command(&mut self) -> Option<SessionCommand>;
}

pub enum Request {
    BeginTextEntry { field: ElementId, rect: Rect, text: String },
    EndTextEntry,
    Copy(String),                            // put a field's selection on the clipboard
    Paste,                                   // answered with Input::Paste
    Share { path: PathBuf },
    Haptic(Haptic),
    OpenUrl(String),
    Replay { name: String, seat: Side },     // the testing screen's press on a recording
}
```

The testing screen exists only in a tools build of the desktop app (the `tools`
feature of `beryl_3d`). That build sends `Input::Recordings` at startup with the
name of every recording in the recordings pack, and the start screen answers the
key T with the testing screen once some are listed. The screen filters the names
by what is typed in its field, and a press on one asks for `Request::Replay`,
which the app answers by replaying that recording from its start, as `--fixture`
does, from the seat the screen's toggle shows. A release build sends no
recordings, so its client never shows the screen.

What a worker tells the client core when it finishes. The pixels themselves go
to the renderer, and the client core learns only where they are:

```rust
pub enum AssetReady {
    /// A face is in a layer of the small or of the full-size array.
    Face { face: FaceId, size: FaceSize, layer: u16 },
    /// Glyphs are in a page of the atlas.
    Glyphs { page: u16, glyphs: Vec<GlyphCell> },
}

/// A glyph's rectangle in texels, and its top left corner against the
/// glyph's pen position on the baseline, in physical pixels, y down.
pub struct GlyphCell { pub key: GlyphKey, pub rect: [u16; 4], pub bearing: [i16; 2] }

/// An index into the list of faces the client made when the match loaded.
pub struct FaceId(pub u16);
pub enum FaceSize { Board, Full }
/// One glyph of one bundled font at one weight and one size in physical
/// pixels. The text fonts are variable, so the weight is the axis value.
pub struct GlyphKey { pub font: u8, pub weight: u16, pub glyph: u16, pub size: u16 }

impl Client {
    /// The glyphs that laid-out text needs and no page holds yet. Each is
    /// returned once. The host hands them to the rasterizer.
    pub fn wanted_glyphs(&mut self) -> impl Iterator<Item = GlyphKey> + '_;
    /// The faces a card being read needs at full size, such as the card in
    /// the inspector. Each is returned once. The host hands them to its
    /// asset workers, which report each one's layer as an `AssetReady::Face`.
    pub fn wanted_faces(&mut self) -> impl Iterator<Item = FaceId> + '_;
}
```

A pointer or key event carries no time (see [Determinism](#determinism)):

```rust
pub struct Pointer {
    pub id: u8,                // 0 for the mouse, then one for each finger
    pub kind: PointerKind,     // Mouse, Touch, Pen
    pub phase: PointerPhase,   // Down, Move, Up, Cancel, Leave, Wheel
    pub position: Vec2,        // logical points from the window's top left
    pub button: Button,        // Primary, Secondary, Middle, None
    pub wheel: Vec2,           // logical points scrolled, for Wheel
    pub modifiers: Modifiers,  // shift, control, alt, command
}

pub struct Key {
    pub code: KeyCode,         // closed: the keys Beryl binds, and Back for
                               // Android's back button
    pub phase: KeyPhase,       // Down, Repeat, Up
    pub modifiers: Modifiers,
}
```

There are four hosts.

- **The headless host** has no window. A test or the command-line tool calls
  `handle` and `frame` directly and reads the frames. It can also own a renderer
  that draws into a texture instead of a window, for pictures.
- **The desktop host** uses `winit` 0.30, the current stable release, on macOS
  and Windows. It starts fullscreen (decision C15): on macOS winit's "simple"
  fullscreen, a window over the whole display with the menu bar and the Dock
  hidden, and not a fullscreen Space. On macOS it paces frames with a display
  link (`NSView`'s, asking for exactly the rate the rate rule chose, at first
  the display's highest) and draws only on the link's ticks: an input or a
  session update reaches the client core at once and is drawn on the next tick.
  In the prototype, `winit`'s own redraw requests left a 120 Hz display at 60
  Hz, and drawing on every input used up macOS's three drawables. On Windows,
  `winit`'s redraw requests are the ticks. F11, and ⌃⌘F on macOS, leave and
  re-enter fullscreen. The `background` window mode makes the app an accessory
  app on macOS, with no Dock icon, so its window never becomes active and never
  gets the keyboard.
- **The iOS host** is a Swift app of a few hundred lines (`ios/Beryl3D`). It
  owns a `CAMetalLayer` and a `CADisplayLink`, and calls the Rust library
  through C functions. It passes each touch with the lowest free pointer id from
  1, the view's size and safe-area insets on every layout (rotating passes a new
  size for the same layer), and the display's highest refresh rate. After each
  frame it sets the display link's frame rate to the `hz` the library returned,
  and pauses the link while the library wants no frame, until a timer at the
  wake time, a touch, a layout, a lifecycle event or the library's wake callback
  unpauses it. Its audio session is `ambient`, so Beryl mixes with the player's
  music and respects the silent switch.
- **The Android host** is a Kotlin app of a few hundred lines. It owns a
  `SurfaceView` and a `Choreographer` frame callback, and calls the same C
  functions through JNI. It sends touch events in logical points, keeping each
  finger's id until it lifts or Android cancels it. Back goes to the client as a
  key press. Rotating the device resizes the surface of the running game. The
  host draws edge to edge and reports the system bars and cutout as insets. On
  API 30 and up, it passes the requested rate to `Surface.setFrameRate`. It
  schedules the next callback at the returned wake time and stops callbacks when
  the client is idle. Input and the Rust waker can request a callback sooner.

**Choosing a frame rate** (decision C18). Every host feeds the time between its
animating frames to one rule, `RateRule` in `beryl_host::rate`, through the
frame loop that all hosts share, and carries out the steps it returns. Its
thresholds are `frame_rate` in `data/beryl3d/space.json`.

- **A frame misses the rate** when more than `late_intervals` (1.5) of the
  chosen rate's intervals passed since the frame before it while the ticks ran.
  The first `settle_frames` (120) animating frames after the host opens are not
  counted, because a new window's first second was often late.
- **Stepping down.** When `misses` (3) of the last `window` (60) animating
  frames missed, the rule first drops the Enhanced tier to Baseline, if the tier
  is chosen automatically and is Enhanced. If frames still miss, it asks for the
  next rate that divides the display's (120, 60, 40, 30 Hz), down to `lowest_hz`
  (30).
- **Stepping back up.** After `up_frames` (600) animating frames in a row that
  came on time and whose own work (the client's frame and encoding it) took no
  more than `room` (half) of the next rate up's interval, it asks for that rate.
- **Not going back and forth.** When frames miss again within `up_frames` of a
  step up, the next step up waits twice as long, up to `up_frames_most` (9,600
  frames). A step up that holds restores the ordinary wait.
- **Who steps.** The macOS display link asks for exactly the chosen rate. The
  windowless host's timer steps the same way, so `--slow-gpu` shows the rule at
  work in a run with no window. winit's redraw requests on Windows cannot ask
  for a rate, so Windows counts frames against a fixed rate and never steps the
  rate, though it still drops the tier. The phones step: the host passes the
  display's highest rate in `BerylConfig`, the rule starts there, and every
  `BerylWake` carries the rate it has chosen, which the iOS host applies to
  `CADisplayLink`'s preferred frame rate range and the Android host to
  `Surface.setFrameRate` when it changes. Some cannot change it: Android below
  API 30 has no `Surface.setFrameRate`, and a display may ignore the request. So
  the library draws on a tick only once the chosen rate's interval has passed
  since the last tick it drew, less half the display's interval
  (`beryl_host::phone::due`). With the display at 120 Hz and the rule at 60 Hz
  every other tick draws, and a skipped tick is not a frame to the rule.

The C functions the phone hosts call, as `include/beryl3d.h` in `beryl_3d`
declares them. Every enumerated value is a `uint32_t` with named constants, and
a value no constant names is dropped, so that no value from C can be an invalid
Rust enum:

```c
typedef struct {
    const char *fixture;   /* the recording to replay; NULL plays a live game */
    const char *data_dir;  /* recordings, frame logs, replays and the ledger */
    const char *stand_in;  /* debug builds: an agent that plays for the person */
    float display_hz;      /* the display's highest refresh rate */
    void (*wake)(void *context); /* any thread: ask for a frame soon; may be NULL */
    void *context;
} BerylConfig;

typedef struct { float top, right, bottom, left; } BerylInsets;
/* kind: BERYL_WAKE_NOW, AT or NEVER; hz: the rate to ask the display for. */
typedef struct { uint32_t kind; uint64_t at; float hz; } BerylWake;

typedef struct {
    uint32_t id;           /* 0 for the mouse, then one per finger; at most 255 */
    uint32_t kind;         /* BERYL_POINTER_MOUSE, TOUCH, PEN */
    uint32_t phase;        /* BERYL_PHASE_DOWN, MOVE, UP, CANCEL, LEAVE, WHEEL */
    float x, y;            /* points from the surface's top left */
    uint32_t button;       /* BERYL_BUTTON_PRIMARY, SECONDARY, MIDDLE, NONE */
    float wheel_x, wheel_y;
    uint32_t modifiers;    /* BERYL_SHIFT 1, CONTROL 2, ALT 4, COMMAND 8 */
} BerylPointer;

/* code: BERYL_KEY_A = 0 to BERYL_KEY_BACK = 46, in the order of `KeyCode`;
 * phase: BERYL_KEY_DOWN, REPEAT, UP. */
typedef struct { uint32_t code; uint32_t phase; uint32_t modifiers; } BerylKey;

typedef struct {
    uint32_t kind;     /* BERYL_REQUEST_BEGIN_TEXT_ENTRY, END_TEXT_ENTRY, SHARE,
                        * HAPTIC, OPEN_URL */
    uint32_t haptic;   /* BERYL_HAPTIC_LIGHT, MEDIUM */
    float x, y, width, height; /* the edited field, in points */
    const char *text;  /* the field's text, the path or the URL, else NULL; valid
                        * until the next beryl3d_next_request or beryl3d_frame */
} BerylRequest;

Beryl3d *beryl3d_new(const BerylConfig *config);
void beryl3d_surface(Beryl3d *, void *native_surface,
                     uint32_t width, uint32_t height, float scale,
                     BerylInsets safe_area);
void beryl3d_surface_lost(Beryl3d *);
void beryl3d_pointer(Beryl3d *, BerylPointer event);
void beryl3d_key(Beryl3d *, BerylKey event);
void beryl3d_text(Beryl3d *, const char *utf8);
void beryl3d_lifecycle(Beryl3d *, uint32_t event); /* BERYL_SUSPENDED, RESUMED,
                                                    * LOW_MEMORY */
/* Draws one frame. Returns when the next is needed: now, at a time, never. */
BerylWake beryl3d_frame(Beryl3d *, uint64_t now_nanoseconds);
/* The next thing the client asks of the platform, or false for none. */
bool beryl3d_next_request(Beryl3d *, BerylRequest *out);
```

- **A `Beryl3d` is the whole app**: the session, the client core, the renderer
  and the workers. `beryl_3d` exports these functions.
- **The session runs inside the Rust library.** Session updates and finished
  assets never cross the C boundary. They arrive on channels that
  `beryl3d_frame` drains before it draws.
- **Requests are polled.** After each frame the host calls
  `beryl3d_next_request` until it returns false.

The Swift and Kotlin hosts build on the apps used to test the graphics paths on
each platform. They can replay a recording for a smoke test or start a live
game:

- **The functions.** `beryl_3d` exports every function above (`src/ffi`,
  declared by hand in `include/beryl3d.h`). With a recording named, the app
  reads `<recording>.json` from its data folder. With none, it plays a live game
  on the session thread, as the desktop app does, and keeps its replays and its
  ledger of games in the data folder. In a debug build, `stand_in` names an
  agent that answers the person's decisions in a fresh game, for unattended
  runs. The app writes its frame log and its error log (`beryl3d.log`) in the
  data folder. The frame log has a line for each pointer, key, text, surface and
  lifecycle input, so that a smoke test can see its events arrive. The client's
  requests to save settings and play sound never cross: the app will carry them
  out itself. Android's Kotlin host calls JNI functions that turn its strings
  into UTF-8 and its `Surface` into an `ANativeWindow`, then call the same code.
  Its wake is an object whose `wake()` the library calls after attaching the
  calling thread to the JVM.
- **The header's test.** `tests/c/header.c` calls every function of the header
  from C, with an offscreen target in place of a native surface
  (`beryl3d_offscreen`, which only test builds export), and `tests/all/ffi.rs`
  checks the frame log it leaves.
- **The hosts** are `ios/` (Swift, an xcodegen spec, iOS 15) and `android/`
  (Kotlin, built with the Tauri app's Gradle wrapper, API 24) beside the crate.
  Their bundle and application ids are `ca.thurn.beryl3d`, so both apps can be
  installed beside the Tauri app. The Android host passes platform events to the
  library, applies its requested rate and schedules its callbacks from the
  returned wake times. The iOS host uses a display link, applies the requested
  rate and pauses between wakes and while the app is inactive.
- **The library** is built with `cargo rustc --crate-type staticlib` for iOS and
  `cdylib` for Android, so that desktop builds link neither. On Android it loads
  at API 24 and needs the system's libraries and the NDK's `libc++_shared.so`,
  which the texture encoders link and `just b3d android-smoke` puts in the APK
  beside it.
- **A phone has no recordings pack.** `just b3d ios-smoke` and `android-smoke`
  write the recording on the Mac with
  `beryl3d --fixture <name> --write-fixture <path>` and copy it into the app's
  folder. `just b3d ios-device` does the same on the paired iPhone.
- **The phone's device is made with OpenGL ES 3.0's limits**
  (`downlevel_webgl2_defaults`), the floor's, on every backend.
- **No debug names on GPU objects.** The Android emulator's Vulkan driver
  crashed with SIGSEGV in `vkSetDebugUtilsObjectNameEXT` while wgpu named the
  device's first objects, which a debug build does for every object. The phone
  instance leaves wgpu's `DEBUG` flag off and keeps validation.
- **wgpu's Android surface needs a display handle**, even though Android's
  carries nothing: without one, `create_surface_unsafe` refuses the window.
- **An Android renderer uses one backend.** With every backend enabled, wgpu
  also made a Vulkan surface on the window, Android's Vulkan loader then tied
  the window to EGL, and configuring the GL surface failed with "Invalid
  surface". `native_backend` in `beryl_render/src/surface.rs` picks Vulkan when
  an instance with only Vulkan finds an adapter, and GL otherwise, before any
  surface exists.

The phone hosts handle the following platform behaviors.

- **Text entry.** On phones the host shows a native text field over the element
  being edited, so the system keyboard, autocorrect, dictation and input methods
  all work as the platform intends. The client sends `BeginTextEntry` with the
  field's rectangle and contents, and the host sends a `Text` input
  (`TextChange::Replace`) for every change. On the desktop the field is edited
  in place, with `parley`'s `PlainEditor`: the host sends typed text and an
  input method's committed text as `TextChange::Insert`, its composing text as
  `TextChange::Preedit`, which the field shows underlined at the caret, and
  answers `Copy` and `Paste` with the system clipboard (arboard).
  `BeginTextEntry` turns the input method on with the field's rectangle as its
  candidate area, and `EndTextEntry` turns it off.
- **Losing the surface.** The Android host stops its frame callbacks on pause
  and drops the GPU surface before returning from `surfaceDestroyed`. On resume
  it waits for a valid surface before scheduling a frame. iOS forbids GPU work
  while suspended. The host stops its display link when the app becomes
  inactive, calls `beryl3d_surface_lost` on entry to the background and supplies
  the layer again on entry to the foreground. Frames resume when the app becomes
  active. The renderer drops the surface but keeps the GPU device and queue.
  wgpu's `Device` outlives its `Surface` on Metal, Vulkan and OpenGL, so every
  uploaded face, glyph page and pipeline survives the suspension. On return the
  host supplies a new surface and the renderer attaches it to the same device
  (`Renderer::attach`), uploading nothing. The client core's state is untouched,
  so the table reappears exactly as it was. A lost device, which would need
  every texture made again from the disk cache, is not handled yet.
- **Low memory.** On a low-memory `Lifecycle` input the host tells the renderer
  directly (`Renderer::low_memory`), beside handing the input to the client. The
  renderer frees the full-size face array and the full-size faces waiting to be
  uploaded, and makes the array again, empty, at its next full-size upload. It
  will also free the Enhanced tier's buffers once that tier has any. The client
  forgets every full-size face's layer, so those cards draw from their
  board-size faces until a full-size face arrives again, and it stays in the
  Baseline tier for the rest of the session, ignoring a later Enhanced tier.
  Board-size faces and glyph pages are kept. The Android host forwards
  `onLowMemory` and critical trim notices, using the foreground or background
  threshold according to the activity's state. The iOS host forwards UIKit's
  memory warning and schedules a frame when active.
- **iOS requests.** The host presents a share sheet for a file, uses UIKit
  impact feedback and opens links through `UIApplication`.
  `just b3d ios-smoke --lifecycle` exercises suspend and resume in the headless
  Simulator. The debug host also accepts test notifications for low memory and
  the three platform requests and records the dispatched requests in `host.log`.
- **Android requests.** The host uses Android's haptic feedback, opens links
  with an `ACTION_VIEW` intent and shares report files through a read-only
  `FileProvider` URI. `just b3d android-smoke --lifecycle` exercises Home and
  return, low memory and these requests in a headless emulator. The frame log
  marks the first presentation after resume and any presentation while
  suspended. Add `--gles` to exercise the same path on OpenGL ES.

## The table

The table is a real 3D scene. This section specifies its coordinates, its
camera, how game objects are placed in it and how verbs move them.

### Space and camera

- **Units are millimetres.** A card is 63 by 88 and its corners have a 3
  millimetre radius, as printed cards do.

- **The table is the plane `z = 0`.** `x` runs to the viewer's right, `y` away
  from the viewer toward the opponent, and `z` up toward the camera.

- **Cards have real heights.** A card on the battlefield lies at `z = 0`. Each
  card in a pile lies 0.3 above the one below, so a 60-card library is 18
  millimetres tall and visibly shrinks as it is drawn from. A card in flight
  rises along an arc.

- **A pile draws few records however tall it is.** Only its top card and the
  card under it are drawn as cards. The cards beneath keep their scene objects
  and resting poses, undrawn, so a card taken from inside the pile starts where
  it lay. One record of the card mesh, stood on its edge and scaled to their
  height in the color of card stock, is the side of the pile facing the camera.
  Each pile with a card has a `Role::Pile` element named as the web interface's
  `pileName` names it, and its count is a float beside it. The piles stand in
  the corners of the window where the web interface's `PILES` puts them, at the
  web's size against a creature, on the point of the table under each.

- **One camera per layout, with a fixed orientation.** The desktop layout and
  the phone layout each have one perspective camera, defined by its pitch, its
  vertical field of view and the point on the table it looks at. It never turns,
  and it never moves while the window keeps its shape. The one exception is the
  `shake` effect, which slides the picture of the table a few pixels for a sixth
  of a second (see [Effects](#effects)).

- **The desktop camera is fitted to Arena.** Its pitch and field of view are
  solved from the corners of cards measured in Arena's recordings, so the
  table's foreshortening matches the reference. Arena's battlefield cards are
  cropped art tiles of unknown proportions, so `just b3d camera-fit` measures
  the art frame printed on each tile and gives each its own aspect ratio: the
  sides of untapped cards fix the pitch, and the slight turn of tapped cards
  fixes the field of view. The fitted values are data: the desktop layout's
  camera, rows and piles are `layout.desktop` in `data/beryl3d/space.json`, the
  hands and the stack, which are placed relative to the camera, are `hand`,
  `opponent_hand` and `stack`, contact shadows are `shadow`, and the card's size
  is `card`. A build script turns the file into constants by one rule that names
  no key.

  The phone camera is not fitted to Arena: `just video list` found no local
  recordings when it was written. Its source is the web interface's portrait
  layout instead. The fixed orientation (pitch 10 degrees from straight down,
  field of view 24.29 degrees, both in `layout.phone` of `space.json`) was
  chosen so that a tapped card in the far row keeps a 44-point square at 375 by
  647 points. Only the distance is fitted: the required rectangle must match the
  battlefield rectangle that `phoneLayout` (`ui/src/mobile/layout.ts`) gives at
  393 by 759, sized as `useViewport` (`ui/src/mobile/chrome.tsx`) sizes the
  viewport. That is a column inside 8-point gutters, from the top safe inset
  plus 48 points to the window's height less the hand band and 60 points. With
  safe insets of 44 and 34 points the projected edges measure (8.01, 92.00,
  384.99, 577.99) against the target (8, 92, 385, 578), each within 0.01 points.
  The camera is framed to the safe area and does not turn; a window of another
  shape moves it along its line of sight.

A window of another shape does not turn the camera. It moves the camera along
its own line of sight, nearer or farther:

- The layout names a rectangle of the table that must always be visible (both
  players' rows, the piles, the stack's place).
- The camera's distance is chosen so that rectangle fits in the window, inside
  the safe area and clear of the regions the flat layer has reserved.
- A wider window then shows more table cloth at the sides, and a narrower one
  moves the camera back.

The phone layout (`table::phone`, `layout.phone` in `space.json`) follows the
web interface's portrait bands (`phoneLayout` in `ui/src/mobile/layout.ts`): the
opponent's strip under the top safe inset, the battlefield, the viewer's strip
and the hand, which runs off the window's bottom. Its camera fits the required
rectangle inside the battlefield's part of the window rather than the whole safe
area, and at 393 by 759 the rectangle fills it to within a hundredth of a point.
It looks down at 10 degrees from straight down, against the desktop's 27, so
that a tapped card in the far row keeps a 44-point square even at 375 by 647.
Each side has two rows, as on the desktop, because the web's separate lands and
other-permanents bands are too short for the phone's touch target. The middle
band between the creatures' rows stays the same height whether or not the stack
holds anything, and the stack's items stand side by side about its middle in one
row. A stack with more items than fit across shows its top items, as many as fit
beside room at each end for a count (`stack.count_room`), and scrolls as a
battlefield row does: rows of items stacked over the battlefield would cover the
creatures' rows past what a press can reach. The viewer's hand, too, shows only
as many cards as keep their strips in rows under the top safe inset, and scrolls
(below, "How a row scrolls"). The rows never move for the stack, so an item
keeps the web's size, 68 points wide, only where the band is tall enough: at 375
by 647 it is 51 points wide, so that a tapped creature under it, 56 points tall
there, keeps an uncovered strip of 44 points and a cell of the hit test's grid
at each edge (`stack_height`).

### Objects and placement

Each game object has one **scene object** that lasts as long as the game object
does, keyed by the object's id in the view. A card that moves from a hand to the
stack to the battlefield is one scene object throughout, which is what lets a
flight start exactly where the card was.

- **Placement is a pure function.** For a view and a window shape, a **placement
  function** gives every object the pose it rests in. There is one function per
  zone: battlefield rows, hands, the stack, each pile, and the places cards are
  held up to be read.
- **The battlefield fits what it holds.** Each row groups plain identical
  permanents into piles and shows the count on the lowest-slot card, as the web
  interface does. Sources and targets on the stack stand alone. The lands row
  starts piling once its controller has seven lands across the battlefield. A
  row lays its groups out at full size while they fit, then overlaps them down
  to the least uncovered strip and scrolls if needed.
- **Rows stand as the web interface's do.** The creatures' row is centred; the
  lands run from the table's edge inward and the other permanents from the
  middle outward, so a permanent that leaves one of those rows moves only the
  cards to its right.
- **Some cards lie under a permanent, in its row**, as the web interface tucks
  them: the other cards of a merged permanent and the cards a permanent holds in
  exile peek out to its left, and the Auras and Equipment attached to it,
  whoever controls them, peek out to its right, unless it is a land or is
  attached to something itself. Each peeks out by `beneath_step` of a card's
  width, or by the row's least strip where that is wider, so that it can still
  be pointed at. Where a permanent and the cards under it would then be wider
  than the row, as six cards held in exile are on the phone, the cards peek out
  closer together until they fit, since a row scrolls only whole groups. A card
  held in exile is not in its exile pile or that pile's count.
- **An overlapped card keeps a strip of at least 24 by 24 logical points
  uncovered**, 44 by 44 on the phone layout, so it can always be pointed at.
- **A row that cannot give every card its strip scrolls.** After piling
  identical permanents, a row with more cards than fit at the minimum strip
  shows as many as fit and scrolls sideways under a drag or the wheel, with a
  count of the cards off each end. On a phone a board of 255 different
  permanents cannot fit any other way. Arena's recordings show no board this
  large, so UI Conventions lists the behavior among those not seen in Arena.
  Cards scrolled out of view are not drawn.
- **How a row scrolls.** The table keeps a scroll offset, in groups, for each
  row of each side. The wheel over a card of the row scrolls it one group for
  each notch (`pointer.wheel_notch_points` of travel), toward the end the wheel
  turns to. A drag that begins on a card of a row with cards off an end scrolls
  it one group for each `pointer.scroll_step_points` the pointer travels
  sideways, the row following the pointer; no battlefield card can be picked up,
  so any card of the row may start one. The offset stays within the row. A
  scroll places every object again and compiles every script not yet ended
  again, as a change of window shape does: each card that stays in view slides
  to its new place over `slide.regroup_ms`, and a card that comes into view
  fades in where it rests. A card out of view rests undrawn at the nearer end of
  the row: at its left end, or one gap past its right end, but no farther right
  than the battlefield's rightmost row ends. A script moves a card that is out
  of view from there. It is drawn while its own step's verb moves it across the
  table, such as a flight to the row's end, but not while it only slides because
  other cards moved, or only drops a layer because a card before it left the
  row. A card that moves between two places out of view, as one past the right
  end moves to the row's end, is not drawn at all. Each end with cards off it
  shows their number, as "+12", in a float just outside that end
  (`floats::ROW_COUNT`).
- **The phone's hand and stack scroll as a row does.** The table keeps a scroll
  offset for the viewer's hand, in cards from its left end, and for the stack,
  in items from its top, so that an item cast while the stack shows its top
  comes into view. The wheel over either scrolls it, and so does a drag on a
  stack item. A hand card can be picked up by a drag, so a drag on a hand that
  scrolls scrolls it only when it first travels the drag threshold more sideways
  than up or down, and then picks no card up; a card is played by dragging it
  up. A card out of view rests undrawn where the nearer end card in view does.
  The stack's counts stand beside its row's ends (`floats::STACK_COUNT`) and the
  hand's over the corners of the column at the hand's top edge
  (`floats::HAND_COUNT`), since a hand that scrolls fills the window from side
  to side.
- **An ability on the stack is a scene object like a spell.** It is drawn as a
  card showing its source's face, at its place among the stack's items, and
  arrows, marks and steps reach it by its number, which the session's scripts
  give it (a view's stack names no ability). The table keeps those numbers from
  a reset, from each `emerge` and from the table each script ends with.
- **Hands, the stack and cards being read keep their place on screen.** Their
  positions are given relative to the camera, not the table, so they are the
  same size and in the same place at any window shape. They are still 3D
  objects: they tilt, they are lit and they cast shadows on the table below.
  Each hand is fanned as the web interface's is: the middle of each card's inner
  edge lies on one circle whatever the hand's size, so a crowded hand turns and
  sinks less for each card. The viewer's cards close up as the hand grows, but
  never so far that a card's uncovered strip is narrower than `strip` plus 8
  points, widened for the card's turn, since a slanted strip holds a square only
  if it is wider than the square. A hand that one row of such strips would carry
  past the window's sides is laid in two or more rows, each fanned the same way,
  in hand order from the back row to the front, and each row stands high enough
  above the one in front of it to leave every card a square of `strip` above it.
  Sixty cards take two rows in every desktop window. On the phone, where the
  rows may rise no higher than the top safe inset, a hand too large for the rows
  that fit there shows as many cards as keep their strips and scrolls; 60 cards
  at 375 by 647 would need about 12 rows where 10 fit. The stack's items stand
  at the right of the screen, the top item in front and each item beneath
  peeking out above and to its left, and a tall stack shrinks to fit. Each item
  beneath is pressed through the strip it shows at its left. A stack so tall
  that this strip would be narrower than the reachable check's square (8 items
  in a 1440 by 900 window) stands in rows instead, which the web interface does
  not do: each row stands a little above the one in front of it, and each item a
  little to the left of the one in front of it in its row, so every item keeps
  an uncovered corner `stack.spare` times the square's side. Each row holds as
  many items as lets the items be largest while the rows fit under the stack's
  top and reach left no further than the leaning stack ever does, to no less
  than 0.4 of their full size: past that the rows grow longer. As the web
  interface ends the permanents' rows left of its stack (`ROOM.end`), a
  battlefield row whose cards the stack's band of the screen would cover past
  the reachable check's square (both creatures' rows) ends a gap left of the
  furthest the leaning stack ever reaches, however tall the stack is, so no card
  under the stack loses its square and no row moves as the stack grows. A spell
  that is resolving, which the game has taken off its stack while its card stays
  in the stack's zone, stays the top item until it leaves.
- **Cards a prompt holds up lie in the tray.** `table::tray_layout` ports the
  web interface's `laidOut` for one pile (rows about the middle of the space)
  and two (boxes side by side), in the space `desktopTray` gives, placed
  relative to the camera. On the phone the space is the web's `phoneTray`: the
  column over the opponent's rows, with a second band over the viewer's rows for
  the second of two piles, so that the middle band stays clear. Cards there keep
  the touch target's square uncovered wherever they overlap
  (`TraySpace::least_step`), closing up no further than that and lying in
  overlapping rows where one row has no room. Two kinds of split are laid out as
  the web lays them out (`Held::kind`): cards picked out of the library from
  among the rest lie in one fan in the order they were dealt, with no boxes, and
  cards put back from the hand onto the library lie as the library, whose card
  backs the table draws, and the hand fanned beside it. `Table::lift` holds
  cards up, each moving from where it was drawn into its tray place, and
  `Table::let_go` sends them back. Only the table's own resting places put them
  in the tray: scripts are played from their zones (`Table::rests` gives those),
  and the first script to start after the tray lets go moves each card it
  carries elsewhere from where the tray held it (`Show::hand_over_tray`), as the
  web interface's `Hold.keeps` does. While the tray is up only its cards can be
  pressed, as the web's shade over the table takes the pointer.
- **The opponent's scried cards lie beside their library.** They lie face down
  on the table at library size, as in Arena. Choices such as taking cards from
  the top of the library leave its cards in the pile until a script step moves
  them. The viewer's hand, when the opponent looks at it, is held up in the
  middle of the screen. A look that lifts cards fixes their identities as it
  begins. For a look that lifts cards without a `lift` step, the script reads
  the look from the view it leads to.
- **A card picked in a hidden pick lies apart from its hand.** While a
  resolution such as Show and Tell waits for the other player to pick, a hand
  card whose `hidden_pick` is set is held up where the cards of that player's
  hand are held for a look: low in the middle of the screen for the viewer's,
  high for the opponent's. It shows its face to its player and its back to the
  other. The tree marks it `picked`.
- **A merged permanent's other cards peek out from under it.** A card in the
  merged zone (`ZoneName::Merged`) lies in its permanent's row, a step to its
  left and away from the viewer for each card under it, as the web interface
  tucks the cards a permanent holds, and the row leaves room for them. They peek
  away from the viewer on both sides, where the web's peek toward their
  controller: a card nearer the viewer is nearer the camera, which the interface
  checks and the hit test take for in front. Under a land, and under every
  permanent on the phone layout, they stay level instead: the phone's rows stand
  only a few millimetres apart, so a card rising from the back row would reach
  into the creatures' row. The tree places them `merged.<side>.<row>`.
- **A phased-out permanent stays where it lay, faded** to 40% opacity
  (`card.phased_out_opacity`), and the tree marks it `phased-out`. **A copy**
  shows the copied card's face and name, and the tree names the card it really
  is, as `copy("Phantasmal Image")`.
- **A decision made for another player says so.** While the viewer controls the
  opponent's turn, the prompt bar says "For opponent:" before what a decision
  asks, and a priority says only "For opponent".
- **A revealed card has a second card element.** A card held up to be read
  (`showcase`) lies where it was, and its face joins the Revealed Cards pile at
  the top left of the screen, placed relative to the camera. The face's element
  id is the card's slot plus `REVEALED_FACE` (2^21), which the "no duplicates"
  check counts apart from cards. The pile keeps its faces until a script with a
  reset, or until another cause reveals cards, or until the player dismisses it.
  While it holds a face, a "Revealed Cards" pill stands over it with a cross
  (`floats::REVEALED`, laid out once ahead, the first time a script reveals).
  The pill opens a viewer of the pile's faces, drawn flat, and the cross ends
  the pile's spans at once.
- **A press on a pile opens its viewer.** A pile's element catches presses, as
  the web interface's button over each pile does, and opens the viewer of that
  pile's cards (`Press::View`). It writes no element while a prompt holds its
  top card up in the tray or once that card has left the pile, and a card in
  flight passes over it in the hit test.
- **Marks are separate quads.** What a card's own record says (tapped, a power
  and toughness plaque, counters, summoning sickness) and what a prompt marks on
  it are drawn as a few quads placed on the card's face, so they change without
  touching the face's texture. The marks of a permanent's own record (a moon for
  summoning sickness, a token's coin, a stolen permanent's violet outline, an
  attacker's wedge and a blocker's crescent) need no span and no step: the table
  draws them from the record it holds, in `beryl_scene`'s `table::own`. The
  wedge and the crescent face the defending side, on the card's edge that faces
  it most nearly, and fade in from the frame the record first says the creature
  fights.

### Verbs

Each of the 16 verbs has one implementation in the client core, which turns a
script step into segments on the scene objects it concerns. Each verb is a file
of `beryl_scene`'s `verbs` module with one function, `play`, which is given the
step (`StepIn`, with the other steps of its beat) and writes segments, spans,
streaks and notices (`StepOut`).

- **Timing comes from data, through the session.** A verb's duration, delay,
  easing curve and **stagger** (the delay between successive objects when one
  step moves several, such as three cards drawn a third of a second apart) are
  in `data/ui-conventions.json`. The session reads them and sends each step's
  numbers with its script. A change to a duration is one edit that both clients
  pick up.
- **A step may be several segments.** A verb writes one segment for each object
  it moves, or several back to back that together last the step's time. A card
  the viewer draws alone flies to a place beside the stack (`verbs.fly.held`, in
  screen coordinates, placed relative to the camera as the hand is), is held up
  there sinking a little toward the hand, and drops into the hand: three
  segments. Each of several drawn together is one `Swept` segment past that
  place. On the phone, whose table has no room beside the stack, that place is
  over where the card will lie in the hand (`verbs.fly.held.phone`), as the web
  interface's phone holds it, and it is kept inside the safe area.
- **A flying card is drawn in front of what it passes.** The web interface draws
  a flying card above every other. A straight, thrown, held or swept flight
  moves its card toward the eye along its line of sight for most of its way
  (`Lift`, with `verbs.fly.front`), which changes nothing on the window but what
  the card is drawn over, and eases it back as it lands. On the phone this is
  what keeps a flight in sight: the viewer's piles lie partly under the hand,
  and the battlefields lie just above and below the stack.
- **Bows are measured on the screen.** A flight that leaves its straight line
  sideways (`Shape::Bowed`) takes its length across and up the window from its
  two ends' places on the window, in millimetres at the depth of its middle, as
  the web interface's bows are measured in stage pixels. Measured along the
  camera's axes instead, a way from a hand near the eye to a library far off
  reads as going down when it goes up the window.
- **Space comes from data too.** Values only a 3D client needs (how high a
  flight arcs, how far a held card lifts off the table, how far a striking
  creature lunges) are in a file of Beryl 3D's own, `data/beryl3d/space.json`,
  under the names of the verbs and cases in `data/ui-conventions.json`. A check
  fails for a name that is no verb or case. Tuning a 3D value does not touch the
  web interface's data file, so none of the web interface's checks run for it
  (decision C17).
- **Arrows are drawn from their ends on every frame.** `point` only begins and
  ends a span. `light::arrows` draws each open arrow from where its two ends are
  in that frame, on the screen, with the web stage's shapes
  (`ui/src/stage/lines.ts`): a target arrow arcs over what lies between, the
  line to a source sags toward the bottom of the window by `light.sag`, and a
  line of combat runs straight. Its values are under `light` in `space.json`.
- **A step ends when its animation ends.** A script's beats play in order, the
  steps of one beat together, on the time passed to `frame`.
- **A notice outlives its beat.** `announce` moves nothing: it writes a
  `NoticeRun`, which the show keeps in real time from when its script starts
  until it ends, and the client shows it as a float (the turn banner, a scry's
  pill, a coin's flip). A notice takes no press: a press on it reaches the card
  beneath. A notice whose step `passes` goes on standing while the table plays
  on under it, across scripts, and `Show::passed` says when the last such notice
  is over. `dwell` moves nothing either: it begins a `Span::Focus` on its
  subject for the step, so the card is drawn `focused`.
- **Objects come to rest where placement puts them.** When a script has
  finished, every object's pose equals what its placement function gives for the
  new view. A check holds this on every fixture, so animation code cannot leave
  an object slightly out of place.

A verb's entry in `data/ui-conventions.json`, and then its spatial values in
`data/beryl3d/space.json`. Durations are in milliseconds and distances in
millimetres:

```json
{
  "name": "fly",
  "values": { "duration": 420, "curve": [0.2, 0.0, 0.1, 1.0], "stagger": 330 }
}
```

```json
{
  "verbs": { "fly": { "arc": 55 } },
  "pointer": { "rise": 100, "fall": 160, "held_height": 40 }
}
```

### Timing a script

A script's steps say what happens and in what order. How long each takes, which
ones overlap and which effects each begins depend on rules: a spell the viewer
casts is timed differently from one the opponent casts, and a permanent that
taps and leaves in one beat leaves only after it has turned.

Those rules were the web interface's, in `ui/src/stage/timing.ts`. They move
into the session, in `beryl_protocol`'s `script::timing` module, which is the
only place they exist. The session sends each step's timing with its script, and
both clients play what they are sent.

```rust
pub struct Script {
    pub reset: Option<Reset>,
    pub beats: Vec<Vec<Step>>,
    /// One entry for each step, in the order the steps are played: every
    /// step of the first beat, then every step of the second, and so on.
    pub timing: Vec<StepTiming>,
}

/// How one step is played. Times are in milliseconds.
pub struct StepTiming {
    pub case: Option<String>,   // the verb's case in data/ui-conventions.json
    pub duration: f64,          // how long its motion takes
    pub delay: f64,             // how long after its turn comes it begins
    pub stagger: f64,           // how long the next step waits for its turn
    pub curve: Curve,           // Linear, EaseIn, EaseOut or EaseInOut
    pub each: Option<f64>,      // for a step that moves several cards one
                                // after another, how long each one moves
    pub with: bool,             // it takes the turn of the step before it
    pub after: Vec<Subject>,    // it waits until the steps of its beat that
                                // move these have ended
    pub hold: Option<f64>,      // it ends a span, and waits until the span
                                // has stood whole this long
    pub leads: bool,            // what follows it waits until it is over
    pub passes: bool,           // its beat does not wait for it
    pub alone: bool,            // it waits until nothing passes over the table
    pub opens: bool,            // its act is applied as its delay begins
    pub ahead: bool,            // its act is applied with the step before's
    pub closes: Option<f64>,    // how long what it left room for takes to
                                // close up after it; when anything does,
                                // its beat lasts until it has
    pub room: Room,             // how what it makes room among goes to its
                                // new place
    pub phases: Vec<Phase>,     // where each named phase of its motion
                                // begins (a strike's poise and hit)
    pub effects: Vec<StepEffect>,
}

/// How what a step makes room among goes to its new place.
pub enum Room {
    Step,                       // over the step
    For { ms: f64 },            // the cards that stay in a hand regroup in
                                // `ms` from when its act is applied
    From { share: f64 },        // from this share of the step to its end
                                // (a tap's row waits until it has turned)
}

/// One named phase of a step's motion.
pub struct Phase {
    pub name: String,           // "poise" or "hit"
    pub at: f64,                // where it begins, as a share of the step
}

/// An effect a step begins.
pub struct StepEffect {
    pub name: String,           // the effect's name in the data
    pub when: Edge,             // Start or End of the step
    pub on: Option<Subject>,    // what it is on, where that is not the
                                // step's own subject
    pub into: Option<f64>,      // how far through the step it begins, from
                                // 0 to 1, where that is not an edge of it
    pub duration: Option<f64>,
    pub delay: f64,
    pub curve: Curve,
}
```

These fields are the web interface's `Timing` record (`ui/src/stage/tracks.ts`)
and its `Effect` record, with the same meanings.

What the session does and what a client still does:

- **The session chooses the case and the waits.** For each step it looks at the
  step's act, the table as the step finds it, the step before it and the other
  steps of its beat, and fills in a `StepTiming`. This is what `caseOf`,
  `timingOf` and `effectsOf` did in `timing.ts`.
- **A client lays a beat out in time.** Given the timings of a beat's steps and
  the time the beat starts, a small function works out when each step begins and
  ends. It is arithmetic, and it cannot move to the session, because two of its
  inputs are known only to the client: how long a span has already been on
  screen (a player may have looked at an arrow for five seconds before
  answering), and whether a banner is still passing over the table. The web
  interface's is `laid` in `ui/src/stage/beat.ts`. Beryl 3D's is a port of that
  one function, in `beryl_scene`'s `beat` module. Beryl 3D lays a script out
  when its update arrives, before both are known, so a script with a step that
  holds a span (`hold`) or waits for the table to itself (`alone`) is laid out
  again as it starts (`play::relay`). Every segment and record of the script
  then moves by how much later the step it keeps to plays. No pose is worked out
  again, which is exact because the steps that wait move nothing.
- **A client makes a few steps itself.** While a prompt is open, the client
  draws its marks, its arrows and the cards it holds up as steps of its own,
  made from the prompt's state (`ui/src/stage/prompt.ts`). Their timing is a
  short fixed table, because it does not depend on the game: `beryl_protocol`'s
  `own_timings` function returns it (`script/timing/own.rs`). Beryl 3D calls
  that function, and the web interface asks for the table once at startup.
  `prompts::own` plays each change of prompt as one beat (`prompts::Steps`), as
  the web's `Prompts.#mark` does: the ends first, then the tray, then the
  beginnings. A step is played for each mark as the session sent it, whatever
  the table draws for it: a payment's `suggested` marks look like `eligible`
  ones until the pointer is on Auto Pay, and a question's are not drawn until
  the pointer is on its yes, as the web's styles draw them, and where only that
  look changes no step plays. A new decision ends the last prompt's marks and
  begins its own, even where they are the same marks. A beat with a tray step
  has the client move the tray's cards from places it worked out as the prompt
  changed, so that the frame lays nothing out. The steps that end a closed
  prompt's marks are written under the view that closed it. The tray of a closed
  prompt lets its cards go as a step of the answer's script, the first script
  given after the prompt closed, as the web's `lowered` has it: first in the
  script's first beat with a step about a card the tray held (or its first
  beat), at that beat's turn on the script's clock (`Show::place_let_go`). Until
  then the cards stay where the tray held them.
- **A client may play scripts faster to catch up.** When many updates arrive at
  once, as when a recording or a reconnection delivers a dozen views together,
  Beryl 3D plays each script that starts with more than `catch_up.double`
  updates waiting at twice real time, and with more than `catch_up.quadruple`
  waiting at four times (`data/beryl3d/space.json`, `play::Show::frame`). The
  web interface has no catch-up. Holds are measured in script time, as the web
  interface measures them: `light::Lit` keeps a script clock that runs at each
  script's rate while it plays and at real time between scripts, and a span
  records when it stood whole on that clock. So an arrow that stood whole while
  a script played at twice real time has stood, when a later script ends it, the
  script time that passed, not the shorter real time.
- **Segments are made from the laid-out beat.** Each step becomes segments, at
  absolute times, on the scene objects it concerns.
- **Reduced motion** is a client's choice: it plays every step with no duration,
  whatever it was sent.

The move is made in three changes, in this order, so that the web interface
never plays a timing nobody has checked:

1. The rules are written in Rust beside the web interface's, and a check,
   `just timing-parity`, runs both over every recording and requires every
   step's `StepTiming` to be equal. Nothing plays the Rust result yet.
2. The session attaches the Rust result to every script it sends.
3. The web interface plays what it is sent, its own rules are deleted, and the
   check of step 1 is deleted with them.

## The flat layer

Everything that is not an object on the table is drawn in a flat layer over it:
prompts, buttons, life totals, the turn banner, menus, the frame around a card
browser, the start screen, settings and the bug report form. It is a tree of
named elements with CSS-style layout.

### Elements

An **element** is a node with a role, a name, a style and children. Screens are
written as plain functions that return elements. There are no macros and no
markup language.

```rust
fn prompt_bar(prompt: &PromptView) -> Element {
    row()
        .name("prompt")
        .gap(space::M)
        .padding(space::L)
        .child(text(&prompt.title).style(type_style::TITLE).grow(1.0))
        .children(prompt.buttons.iter().map(|button_view| {
            button(&button_view.label)
                .enabled(button_view.enabled)
                .on_press(Press::Button(button_view.id))
        }))
}
```

- **A press is a value, not a closure.** `on_press` takes a `Press`: a small
  closed enum saying what the press means (a prompt's button, a setting, a
  screen to open). The client turns a `Press` into a `SessionCommand`, adding
  the open decision's id and revision, which a screen function does not know. A
  value can be compared, so an unchanged element is recognised as unchanged, and
  it can be printed in a test's failure.
- **Layout is flexbox and grid**, computed by `taffy`. Style methods carry the
  CSS property names an agent already knows: `gap`, `padding`, `grow`,
  `align_items`, `grid_template_columns`.
- **The tree is retained.** A screen function runs again only when one of its
  inputs changes (a session update, a prompt reply, the pointer entering an
  element, a window size). Its result is matched against the existing tree by
  name and position, so unchanged nodes keep their layout results and their
  animations. A frame with no input rebuilds and lays out nothing.
- **Every interactive element has a role and a name.** A check fails the build
  for one that does not. Roles and names are what locators find, what the tree
  printout shows, and what an accessibility tree would be built from.
- **Styles come from design tokens.** Colors, type styles, spacing and radii are
  named constants generated from one data file, `data/design-tokens.json`, which
  also generates the web interface's CSS variables (`just design-tokens`), so
  the two clients cannot drift apart in color or type.
- **An element can show a scene object.** The card browser and the mulligan
  screen lay out boxes with `taffy` and place real 3D cards in them, so a card
  looks and animates the same wherever it appears. After each layout or scroll
  the client hands the table every such box (`Layer::shown_objects`,
  `Table::show_in`). The table draws the card in its box over the flat layer
  (`Band::LayerCards`) without changing where it rests, so scripts still play
  from the card's zone. A card new to a box moves there from where it was drawn,
  and one no box shows any more moves back, with the tray's own-step timings
  (`TrayLifts` and `TrayLetGo`). A box scrolled partly out of the element that
  clips it shows nothing, since a 3D card cannot be cut to a clip. While a box
  shows a pile's top card, the pile draws no side and no element.
- **The viewer** (`inspect::viewer`) lists a pile's cards, the top one first, in
  the overlay shell every choice screen uses (`overlay`): a title and an
  instruction over a grid of up to five columns that scrolls past two rows, and
  a Close button that Escape also presses. While it is open no card of the table
  takes a press.
- **An element can show a face flat** (`Shown::Face`): the inspector draws the
  card under the pointer this way, at full size, beside the table. It is no
  scene object, so a game object still has exactly one. Until the full-size face
  arrives, the board-size face is drawn enlarged.

### Widgets

Screens build themselves from the widgets in `beryl_scene::widgets`, one
function each, so a widget looks and behaves the same everywhere. Besides
buttons, icon buttons, toggles and sliders there are:

- **`list(name, rows)`**: a column with role `List` that scrolls when its rows
  pass its box. While it or one of its rows has the focus, up and down highlight
  the next or previous row, going round, and Return presses it. The highlighted
  row is the focused element, so it is drawn by its `FOCUSED` state. A row
  scrolled out of the box takes no presses.
- **`menu(id, title, groups)`** and **`context_menu(at, menu)`**: the look and
  placement of the web interface's `Menu.tsx`. A menu is a list whose rows are
  `MenuItem` elements named by their labels, in groups under a line, each with a
  check mark and the key that does the same (`element::describe` writes it as
  `shortcuts.ts` does). A row sends `Press::Menu(id, n)`, counting the menu's
  rows from 0 over all its groups and then its submenus' rows. A context menu
  (`Anchor::Menu`) opens at a point and is kept `flat.menu_edge` points inside
  the window, highlighting its first row. A submenu (`Anchor::Submenu`) opens
  beside its row on the pointer or the right arrow, on the menu's left when
  there is no room on its right. A press outside the menu, or Escape, sends
  `Press::Dismiss(Dismiss::Menu)`.
- **`tooltip(text, of)`**: a dark label (`Anchor::Tip`) that stands centred
  above the element named `of`, or below it where there is no room above, once
  the pointer has rested on that element for `flat.tooltip_delay` milliseconds.
  The client asks for a frame at that time. It never takes a press.

### Floats

A **float** is a small element placed at a point of the window instead of in a
screen's layout: the turn banner, the pill above a player who is looking at
cards, a life total and the number that floats off it, the count on a pile, the
Revealed Cards pill, count of cards scrolled off the end of a row, the
inspector, the step banner and the ring under the opponent's orb while they
think. An element whose style `passes` lets the pointer through, as CSS's
`pointer-events: none` does, so the card under the inspector stays hovered.
`Layer::float(id, element)` shows or removes one, and its ids are in
`element::floats`.

- **Each float is a taffy tree of its own**, laid out at its content's size and
  matched against the element it had, as a screen is. It is outside the root's
  flex flow, so a float never moves the prompt bar.
- **Its look is not layout.** `Layer::set_float_look` sets a `FloatLook`: the
  `point` in logical points, the `anchor` that says which point of the float's
  box stands there (from (0, 0), its top left, to (1, 1)), its `opacity` and an
  optional `tint` for its text. Setting a look lays nothing out and allocates
  nothing, so the client may move or fade a float on every frame.
- **A float whose text changes during a script is laid out ahead.**
  `Layer::prepare(id, key, element)` lays an element out in `Client::handle`
  under a key the caller chooses, and `Layer::show_prepared(id, key)` swaps it
  in during a frame with no layout and no allocation. The life totals, the
  amount a total changed by and the mana pools are prepared this way, for every
  value the scripts still to play will set (`hud::counts`), so a total changes
  as its `count` step begins without the frame laying anything out. The notices
  of `announce` steps are prepared the same way, one for each notice of each
  script to play, keyed by its script and step (`hud::notices`). The step banner
  ("Combat" over "Blockers declared") is prepared for each view that calls for
  one, and shown for `hud.banner` milliseconds of the show's clock from the
  first frame whose view at rest calls for it; the thinking ring hides while it
  is up, as the web's does (`hud::banner`).

The game log (`hud::log`) is part of the game screen, not a float: its lines are
made in `Client::handle` as each view arrives, by a port of the web's
`logLines`, and it is rebuilt with the screen and scrolled to its foot. While a
permanent where the latest view will rest reaches into its column, it folds to
an icon button at the window's top left.

- **Floats are drawn whether a prompt is open or not**, after the screen and in
  order of their ids. The top element of a float is drawn on a panel, as a
  screen's top element is. A float with a press takes presses at any time.

Anchoring a float to an object on the table, transitions, grid layout and
scrolling are milestone 3's.

### Sizes

The flat layer is laid out in logical points, and the same code serves every
window.

- **Scale factor.** A logical point is one pixel on a standard display and two
  or three on a high-density one. Text and icons are rasterized at the display's
  real pixel size, so they are sharp at any density.
- **Two layouts.** The desktop layout and the phone layout are separate screen
  functions sharing their widgets, as the web interface's two layouts share its
  drawing code. The phone layout is used when the window's shorter side is under
  500 points, and the desktop layout otherwise. One selector decides it,
  `agent::Layout::for_size`, which the screens, the table and the interface
  checks all ask. Each screen's public function calls its portrait alternate
  itself, so the game screen and the others change apart.
- **Portrait screens.** The start screen and the testing screen are one column
  over the safe area, the gutter in from its sides and foot (`flat.phone` in
  `data/beryl3d/space.json`): the opponents' list takes the room the rest
  leaves, and the whole column scrolls when even that is too little, as while
  the system keyboard is up, so every action stays in reach. The opening lays
  the hand out in rows of four, as large as fit between the coin and the dock.
  The result's button spans a dock at the safe area's foot. The bug report's
  description is a sheet along the window's foot. The screens lay a window the
  keyboard shortens out as any shorter window, and every resize builds the
  screen again for the new size.
- **Safe area.** The host reports the insets for notches, rounded corners and
  system bars. The root element pads itself by them, and a check holds every
  interactive element inside.
- **Minimum sizes.** On the phone layout a check holds every touch target to at
  least 44 by 44 points and every line of text to at least 11 points.
- **The phone's game screen** (`screens::game::game_phone`) places the desktop's
  pieces in the table's bands (`table::phone_bands`), with sizes from
  `hud.phone` in `space.json`:
  - Each orb is centred in its player's strip, drawn at half the desktop's size.
    When the viewer's hand needs more than one row, its back row stands in the
    viewer's strip, and a total in the strip's middle would split that row's
    uncovered part into pieces too narrow to press. The table then raises the
    viewer's total and orb over the hand's top, as the dock rises, but never
    onto the viewer's creatures (`table::phone::your_life`). Streaks and strikes
    at the viewer aim where the total stands.
  - A menu button at the right end of the opponent's strip opens the settings,
    which hold the Log and Report bug buttons. The log opens as a sheet below
    the opponent's strip. The phone shows no gear, log column or step stops.
  - The phase icon stands on the middle band's line, and full control's mark
    stands at the band's right end.
  - The open prompt's primary button and its pass-on button stand in a dock at
    the right of the viewer's strip. The dock rises above the hand when a hand
    in two rows reaches into the strip. The prompt's other buttons are stacked
    over the dock. Where the raised dock or the stacked buttons would stand over
    a permanent of the viewer's back row, they stand at the right end of the
    middle band instead, when they fit there and no stack item lies there, and
    the pill beside them is narrowed to the band's left
    (`hud::phone::clear_at_right`). What the prompt asks is a pill on the middle
    band's line, or at the nearest height clear of every permanent and stack
    item (`hud::phone::asked_top`). While the table is shaded, the pill stands
    over the strip instead. The client measures where cards rest
    (`hud::phone::Occupied`) when it builds the screen.
  - Life totals that no prompt can pick, and the count labels, let the pointer
    through to the cards behind them. On the manual table a life total still
    takes the pointer, since its commands act on the total under it.

### Text

Text is laid out by `parley` in the client core and rasterized by
`beryl_assets`.

- **Layout** covers shaping, line breaking, alignment, styled spans inside one
  paragraph, and inline boxes. Mana symbols and other icons in rules text are
  inline boxes, so `{1}{U}` flows and wraps with the words around it. A symbol's
  box is as wide as its line's em. It is drawn as a disc in the symbol's color
  (`flat.mana` in `data/design-tokens.json`, from the web interface's
  `mana-symbols.css`) with the symbol's glyph from the Mana font on it.

- **Layout results are cached** by text, style, spans and width. A prompt's text
  is laid out once, in `handle`, when the prompt arrives.

- **Glyphs are drawn from an atlas.** Each glyph is rasterized once at the pixel
  size it is shown at, into a single-channel texture page, and each glyph on
  screen is one quad. The Latin glyphs of every type style are rasterized at
  startup. Card names and rules text are known when a match loads and are
  rasterized then.

- **A character no bundled font has** is drawn as the font's missing-character
  box. Nothing is fetched from the system.

- **A glyph at a new size is not waited for.** When the display's scale changes
  (a window dragged to another monitor), glyphs are rasterized again on a
  worker. Until a page is uploaded, text is drawn from the old size, scaled: a
  glyph missing at its size is drawn from the nearest size a page holds. Layout
  does not depend on rasterization, so nothing moves when the new page arrives.

- **Text on a card is part of the card's face**, baked into its texture. See the
  [Card faces](#card-faces) section below.

## Rendering

`beryl_render` draws a frame's draw list. It is deliberately small: a card table
is a few hundred textured shapes, and the design's performance comes from doing
very little per frame.

### The Baseline tier

The Baseline tier is the look that must hold 60 frames a second on the floor. It
uses only what an OpenGL ES 3.0 GPU provides, and it is arranged around the one
thing that limits such a GPU: how many pixels it can shade.

Every phone GPU of the floor's era can skip opaque pixels that end up hidden.
The iPhone 6s's GPU removes them before shading, and the Adreno and Mali GPUs of
2015 Android phones reject them early when nearer things are drawn first. None
of them can skip anything once blending is on. The Baseline tier therefore keeps
as many pixels opaque as it can.

- **An opaque pass with a depth buffer.** Cards are drawn nearest first, then
  the table. No shader in this pass discards a pixel or blends.
- **A card is a mesh, not a quad.** Each card is a rounded rectangle of about 40
  vertices, so its corners are geometry and its pixels are all opaque. A quad
  with corners cut out in the shader would switch hidden-pixel removal off on
  some of these GPUs.
- **Soft edges are a thin blended strip.** A one-pixel-wide band of triangles
  around each card fades its edge, drawn in the blended pass. The Baseline tier
  needs no multisampling.
- **A blended pass, back to front**, for the bands of the draw list: shadows,
  card edges, marks, glows and the flat layer. Colors are premultiplied by their
  opacity, so ordinary and additive shapes share one blend setting.
- **Two pipelines and four meshes.** One opaque pipeline and one blended, each a
  single shader that picks its branch from the record's `material`. A pipeline's
  depth test is fixed when it is created, so the blended pipeline is made twice,
  tested against the depth buffer for the table's bands and untested for the
  flat layer's, with the same shader: three pipeline objects, all made at
  startup. The card mesh, its edge strip, a unit quad and the ring of that quad
  that the card-shaped outline sprite covers are the only meshes. The ring
  leaves out the outline's empty middle and sides, three quarters of the quad,
  which every outline glow and mark would otherwise shade for nothing. The
  80-byte records are uploaded as the instance buffer unchanged. Each run of
  records is bound from its offset in that buffer and drawn from instance 0,
  because OpenGL ES 3.0 has no first instance and no base vertex.
- **Colors are blended as stored.** Faces, tints and the target hold sRGB
  values, and the shader and blending work on those values directly, as a
  browser blends CSS colors. The target format is therefore not an sRGB one.
- **The edge strip is widened in the vertex shader.** Each vertex of the strip
  carries the outward direction of the card's outline at that point. The vertex
  shader projects the inner ring as the card's outline and moves the outer ring
  one physical pixel outward on screen, using the target's size in pixels, which
  is a uniform. The strip is then one pixel wide at any distance and any tilt. A
  card's strip is a second record, `Draw::edge` of the card's record, in the
  band of soft edges.
- **Clips are scissor rectangles.** A run of records that share a clip (bits 4
  to 7 of `flags`) is drawn with the scissor set to that entry of the frame's
  `clips`. A clip therefore ends a draw call, and a scrolling list adds two
  calls to a frame.
- **The face is sampled before the branch.** WGSL allows `textureSample` only in
  uniform control flow, and a record's material varies from one instance to the
  next, so the shader samples the face array before it branches on the material
  (as the prototype did). Every table and shadow pixel then pays for one unused
  texture fetch. On the floor that is the cost to watch; the alternative is
  `textureSampleLevel` inside the card branch, which needs no derivatives and so
  may branch, at the price of choosing the mipmap level by hand.
- **Card faces in a texture array.** All the small faces of a match are layers
  of one array texture, so every card on the table can be in one call. Cards
  shown at full size use a second array and add a call. Both arrays are made in
  the first compressed format the device samples (`Renderer::formats`). Until a
  face's layer is uploaded, the renderer fills it with a placeholder of its own:
  a flat color, frame and stripe chosen by the card's name, laid out on the 4 by
  4 pixel grid of a compressed block so that each block is one solid color.
  Every compressed format can write a solid block in a few fixed bits, so the
  renderer needs no texture encoder.
- **Shadows are single quads.** A card above the table (in flight, held up, in a
  hand) gets one pre-blurred shadow quad beneath it, drawn just above the cards
  lying on the table, which spreads and fades with the card's height. Cards
  lying on the table get none. A shadow drawn at one fixed height is hidden
  wherever the cards beneath it stand taller, as they do in a library pile or on
  a stress board whose overlapping rows rise 40 millimetres: the prototype
  raised its shadows on the stress boards to clear them. The client places each
  shadow just above the highest card lying at rest under it, in a pile or a row,
  and casts the card onto that plane along the web interface's light, so the
  shadow falls toward the viewer and a little right, the farther the higher the
  card.
- **No per-pixel lighting.** A face's color is its texture times a tint.

What that buys, measured on 2026-11-15 by `just b3d gate --gpu`'s fragment count
(Mesa 26.0.8's llvmpipe Vulkan driver in `beryl3d-bench`, the same on every
run), in screens of pixels shaded per frame. The recorded board is the
`verbs-zones` fixture from the first seat in a 1920 by 1080 window, 47 and 48
records with 45 of them opaque; it has no flat layer, card faces, marks or glows
yet. "All blended" is the same draw list with every record of the table drawn
blended from back to front and nothing writing the depth buffer (beryl_render's
`fragments` example, `--recorded verbs-zones --at <ms>`).

| Frame                       | All blended | Opaque with depth |
| --------------------------- | ----------- | ----------------- |
| `verbs-zones` at 1,500 ms   | 1.429       | 1.018             |
| `verbs-zones` at 3,000 ms   | 1.419       | 1.016             |
| `stress-255`, `stress-1024` | not counted | 1.000             |

The fragments counted are 2,962,738 and 2,111,552 at 1,500 ms, and 2,942,928 and
2,106,496 at 3,000 ms, of 2,073,600 pixels. The stress boards' 8 ranked frames
each shade exactly one screen at 2880 by 1800 (5,184,000 fragments): every
record is opaque and drawn nearest first, so the depth test rejects every hidden
pixel before it is shaded. The design's estimate for a typical board was 1.90
screens all blended and 1.45 with depth, of which the flat layer was 0.30.

The limits the Baseline tier is held to on every gated fixture. **Fragments**
are pixels actually shaded, counted as the
[Performance gates](#performance-gates) section describes:

```json
{
  "tiers": {
    "baseline": {
      "draw_calls": 16,
      "fragments": 1.75,
      "blended_area": 0.75,
      "upload_bytes": 327680
    }
  }
}
```

Each is a limit on one frame. `fragments` and `blended_area` are in screens, and
`upload_bytes` is instance data, with room for 4,096 records. The limits are in
`data/beryl3d/gates.json`.

The fragment limit is the floor's fill rate turned into a budget. A 2015
mid-range phone GPU shades roughly 300 to 500 million pixels a second, and a
1080p screen, the largest on the floor, at 60 frames a second is 124 million
pixels for each full screen. The hardware therefore affords 2.4 to 4 screens,
and the limit keeps well inside the low end.

The renderer is built against the floor's feature set everywhere, not only on
old devices:

- **Clamped limits in the automated checks.** A check creates the GPU device
  with `wgpu`'s downlevel limits (the OpenGL ES 3.0 and WebGL 2 feature set) and
  draws every gated fixture into a texture in the Baseline tier. A validation
  error fails the check, so a shader or buffer that the floor could not run
  cannot land.
- **Backends.** Metal on Apple platforms. Vulkan on Android where the device has
  it, and OpenGL ES 3.0 where it does not. DirectX 12 on Windows, with OpenGL
  3.3 for PCs that lack it.

### Effects

An **effect** is light, or a change of surface, on one card or one life total. A
step begins it as the step starts, as it ends or as its blow lands. There are
14, listed in `data/ui-conventions.json` with their durations, and each verb's
entry there says which effects its steps begin. Both tiers show every effect.
The Baseline tier draws each one with the records below, and the Enhanced tier
adds to some of them.

| Effect     | Baseline tier                                                                                               | Enhanced tier adds |
| ---------- | ----------------------------------------------------------------------------------------------------------- | ------------------ |
| `glow`     | A `glow` record just over the card: a soft outline round it that spreads and fades.                         | Bloom              |
| `settle`   | The card's `whiten` rises to 1 and falls back, under a burst with rays one and a half cards wide.           | Bloom              |
| `dissolve` | The card darkens through its tint, then its `dissolve` runs from 0 to 1 and it breaks up from the top down. | Ash particles      |
| `dim`      | The card's tint darkens and its opacity falls to 0.                                                         | Nothing            |
| `blanch`   | The card's `whiten` rises to 1, then its opacity falls to 0.                                                | Vortex particles   |
| `cover`    | The card's `whiten` rises to 1, its face layer changes while it is white, and `whiten` falls to 0.          | Nothing            |
| `impact`   | A burst over what was struck.                                                                               | Bloom              |
| `shake`    | The picture of the table slides side to side and a little up and down, twice, the second swing smaller.     | Nothing            |
| `trail`    | A `streak` record behind the dashing card, along its path.                                                  | Bloom              |
| `flash`    | A burst with rays over the spell.                                                                           | Bloom              |
| `beam`     | A cone of light from the side of the ability toward the middle of the table.                                | Bloom              |
| `pulse`    | A ring round the card's frame.                                                                              | Nothing            |
| `loss`     | The life total's text is tinted red, and a flat-layer text element showing the amount sinks away and fades. | Nothing            |
| `gain`     | The life total's text is tinted green, and a flat-layer text element showing the amount rises and fades.    | Nothing            |

- **When an effect plays.** As a script is compiled, each `StepEffect` of a
  step's timing becomes an `EffectRun` on the script's clock, timed as
  `Effects::begin` in `ui/src/stage/effects.ts` times it: it begins at the
  step's start or end, plus `into` times the step's length, plus `delay`, and
  lasts its `duration` or the step's length. It is on `on`, or else on the
  step's subject. As the script starts, the show takes its runs in real time and
  holds each until it ends, past the end of its script, without holding up the
  next script or a prompt. A later run of the same effect on the same thing cuts
  the earlier one short, and a script with a reset drops them all.
- **The shapes of light** (a soft disc, a burst with rays, a ring, a cone, a
  comet, a ribbon, a card outline, an arrowhead, a wedge, a crescent, a tap
  mark, a moon, a plaque and a coin) are sprites that `beryl_assets::light`
  draws in code into the atlas's page 0, in the cells of
  `beryl_scene::frame::sprites`. No image file holds them.
- **The broken edge of a dissolve** is computed in the shader from the card's
  texture coordinates, with no second texture. A dissolving card is among the
  blended records, so no pixel is discarded in the opaque pass.
- **`shake` moves the picture, not the table.** The client core writes an offset
  in physical pixels into the frame's `shake`. The renderer adds it to the
  screen position of every record drawn with the depth test (the table and what
  is on it) and to nothing in the flat layer. Resolved elements are reported
  without the offset and hit tests ignore it, so no check and no click is
  affected.
- **An effect looks as the web interface's does.** Its shape, colors and timing
  are those of `ui/src/stage/effects.ts`, `flare.ts`, `glow.ts` and
  `effects.css`, with its colors in the design tokens file.
- **An effect never changes where anything is**, so the rule below, that the
  tiers share every position, holds for effects too.

### The Enhanced tier

The Enhanced tier adds lighting and effects on devices with fill rate to spare.
It changes only how pixels are shaded.

- **A shadow map.** One key light above the table. A depth-only pass draws the
  cards from the light's view, and the main pass samples it, so every card casts
  a shadow of its true shape on the table and on the cards below it. A card
  casts what is shown of it: one less than half opaque casts nothing, and a
  dissolving card casts only the rows below its breaking edge. The Baseline
  tier's shadow quads are not drawn.
- **Per-pixel lighting.** Cards and the table are lit by the key light and a
  soft fill, with a faint sheen that moves across a card's face as it tilts.
- **Bloom** on glows and streaks, from a half-resolution blur.
- **Particles** for the looks of `vanish` (a destroyed permanent falling to ash,
  an exiled one drawn into a vortex), drawn as instanced quads with seeded
  motion. `effects::particles` writes them: ash leaves each row of a dissolving
  card as its breaking edge passes and falls down the card's face, and the
  vortex spirals from the face of a blanching card into its centre. Each
  particle is a pure function of its run, its index and the frame's time, its
  randomness an integer hash of the object's id and the step, and a frame holds
  at most `PARTICLES` of them, shared equally among its runs. Their counts,
  lives and sizes are `enhanced.particles` in `data/beryl3d/space.json`.
- **Multisampling**, four samples, with a depth buffer.

The rule that keeps the tiers in step: **a tier never changes where anything is,
how large it is or when it moves.**

- The client core is told the tier, through the `Tier` input, for one purpose
  only: to leave out records the tier does not draw. Shadow quads are left out
  of Enhanced, and particles out of Baseline.
- A check plays every gated fixture in both tiers and requires the resolved
  elements to be identical, and every draw record present in both to be
  identical.
- Every layout check and every motion check therefore holds for both tiers at
  once.

The Enhanced tier has its own limits in the gate file, and the automated checks
draw every gated fixture in it on a device with full limits:

```json
{
  "tiers": {
    "enhanced": { "draw_calls": 32, "fragments": 4.0, "particles": 2000 }
  }
}
```

How the gates hold them:

- **Fragments.** `just b3d gate --gpu` draws each gated board's 8 ranked frames
  in both tiers and counts the fragments of every render pass of a frame, one
  pipeline statistics query per pass, summed. An Enhanced frame has up to seven
  passes: the shadow map's (depth only, so it shades none), the multisampled
  main pass, bloom's four and the flat layer's. The four-sample main pass shades
  a pixel once unless a triangle's edge crosses it, and the count is what the
  driver reports, not divided by the samples. Each board's worst frame is held
  to its own measured limit under `tiers.enhanced.fragments.<board>`, which only
  goes down, and a board over the 4.0-screen target fails whatever its limit.
- **Draw calls.** The same run fails on any ranked frame that takes more than
  `tiers.enhanced.draw_calls` calls over all its passes.
- **Particles.** `just b3d gate` plays every board in Enhanced too, and fails on
  a frame with more than `tiers.enhanced.particles` particles, or with a
  different number of resolved elements from the Baseline playback's frame. The
  Enhanced boards have no work limits of their own under `frames`: the client's
  work is the same in both tiers except for particles.

Measured on 2026-11-16 by `just b3d gate --gpu` (llvmpipe, LLVM 21.1.8, in
`beryl3d-bench`, the same on two runs) and `just b3d gate`, the most in any of a
board's 8 ranked frames, in screens of 2880 by 1800. Particles are the most in
any frame of the board's playback.

| Board          | Baseline fragments | Enhanced fragments | Enhanced draw calls | Particles |
| -------------- | -----------------: | -----------------: | ------------------: | --------: |
| `stress-255`   |              1.038 |              1.067 |                   4 |         0 |
| `stress-1024`  |              1.038 |              1.067 |                   4 |         0 |
| `stress-verbs` |              1.316 |              2.825 |                  23 |       513 |
| `stress-hand`  |              1.017 |              1.035 |                   3 |         0 |
| `stress-stack` |              1.111 |              2.688 |                   8 |         0 |
| `verbs-zones`  |              1.012 |              2.538 |                  10 |       127 |
| `pay-default`  |              1.044 |              2.529 |                  10 |         0 |
| Target         |               1.75 |                4.0 |                  32 |     2,000 |

A board with glows or streaks costs about 1.5 screens more in Enhanced than in
Baseline, nearly all of it bloom's: its blur and composite passes each shade
every pixel of a half-size or full-size target. The stress boards with no glows
record no bloom passes, so their Enhanced cost is the Baseline cost and the
multisampled edges.

### Choosing a tier

The tier is chosen once at startup, can only go down during play, and can be
fixed by a setting.

- **At startup the tier is Baseline.** Before the first frame, the renderer
  draws the `stress-255` board in the Enhanced tier into an offscreen texture of
  the window's size, once unmeasured and then 30 times, waiting for the GPU
  after each. If the slowest of those is under a third of the display's refresh
  interval, the first frame is already Enhanced. The first draw is not counted
  because it makes the multisampled buffers and first uses each pipeline, which
  took ten times an ordinary frame. On this Mac the measurement adds about 50
  milliseconds before the first frame. A floor device fails this test and never
  shows an Enhanced frame. The frame log's `tier` line gives the setting, the
  slowest frame, the interval and the tier chosen.
- **During play** the host's rate rule (see [Hosts](#hosts)) watches the time
  between presented frames against the chosen rate's interval (16.7 milliseconds
  at 60 Hz). If 3 of any 60 consecutive animating frames arrive more than half
  an interval late, it switches to Baseline for the rest of the session, before
  it lowers the rate, and the frame log records the switch. This covers a device
  that passed at startup and then heated up, or was moved to a larger display.
- **A setting** offers Automatic, Baseline and Enhanced. Automatic, the default,
  is the two rules above. Baseline and Enhanced fix the tier: a player who
  chooses Enhanced gets it, and it does not step down.

A step down takes effect between two frames and moves nothing, because the tiers
share every position.

What this means for the requirement that visuals never cost a frame:

- **On the floor**, the Baseline limits above are what keep every frame on time,
  and the Enhanced tier is never tried in front of the player.
- **On a faster device that later cannot sustain the Enhanced tier**, at most a
  few frames arrive late, once, before the step down.

## Card faces

A **card face** is the picture on one side of a card: the Scryfall scan when one
has been downloaded, or a frame Beryl draws itself (name bar, art, type line,
rules text, power and toughness) when it has not. Faces are prepared off the
drawing thread and drawn as one mesh each.

### Sizes and memory

This section sizes the textures a match needs. A match shows at most about 150
distinct faces: two 75-card lists in which every card is different, with their
second sides and the tokens they can create. Real lists repeat cards, so 60 to
90 is typical. Decklists are public, so the set is known before the first turn.

Compressed GPU formats at 8 bits a pixel are a quarter the size of uncompressed
color. With mipmaps, which add a third:

| How a face is held       | One face | 150 faces |
| ------------------------ | -------- | --------- |
| 672 by 936, uncompressed | 3.35 MB  | 503 MB    |
| 672 by 936, compressed   | 0.84 MB  | 126 MB    |
| 256 by 360, compressed   | 0.12 MB  | 18 MB     |

- **Board size, 256 by 360**, for every face in the match: at most 18 MB.
  Compressed formats store blocks of 4 by 4 pixels, so both dimensions are
  multiples of 4. A battlefield card is at most about 270 physical pixels tall,
  on a high-density desktop display, so 360 texels is enough.
- **Full size, 672 by 936**, only for cards being read: the card under the
  pointer, the inspector, the top of the stack, a card held up after a draw, the
  opening hand. At most 16 are kept, least recently used first out: 13 MB. A
  screen that shows more than 16 cards large draws the rest from their
  board-size faces. Until a full-size face is ready the board-size face is drawn
  in its place, enlarged.
- **Card backs** are one more layer of each array.
- **In total at most about 32 MB** of face textures for a match.
- **Compressed formats.** ASTC where the GPU has it, then ETC2, which every
  OpenGL ES 3.0 device has, then BC7 on desktop GPUs. Uncompressed color is the
  last resort and is logged, since it quadruples the memory.

### Preparation

No image is ever decoded, composed or compressed on the frame thread, and the
uploads that happen during play are small and limited.

Before a match's opening hand is shown:

1. The client lists every face of both decklists and their tokens, from the card
   store's records that the host gives it (`Input::Cards`). The host reads the
   list (`Client::faces`) and hands it to the workers, so a face's `FaceId` is
   the same on both sides.
2. For each face, a worker looks for the compressed texture in the disk cache,
   keyed by the image's id, the size, the format and the encoder's version. A
   hit is a file read.
3. On a miss, the worker decodes the scan, or draws Beryl's frame, scales it to
   both sizes, compresses it and writes the result to the disk cache.
4. The board-size layers are uploaded as they arrive. The desktop, windowless
   and phone hosts share one part for this (`beryl_host`'s `Faces`): it makes
   the renderer's face arrays whenever the client's face list changes, hands
   every finished face to `Renderer::hold`, and uploads one board-size face in
   each frame in which nothing animates (`upload_waiting`), waking the host
   while faces wait. The session thread gives the client the records of both
   decklists and their tokens when a game starts or resumes, and a replayed
   recording gives its recorded ones. The component gallery's list is prepared
   the same way, and its faces arrive as `AssetReady::GalleryFace`, so they
   never overwrite where the match's faces are. The headless host behind
   `just b3d png` uploads every face before its first picture
   (`Renderer::upload_layer`), but never downloads a scan, so a picture shows
   only what the scan cache holds.

Three crates do that work: `tiny-skia` 0.11.4 (a CPU rasterizer for 2D shapes)
and `swash` 0.2.10 (a glyph rasterizer) draw Beryl's frame, and `ctt` 0.6.1 (a
wrapper over several texture encoders) compresses.

- **`ctt` is built with default features off** and three encoders on:
  `encoder-astcenc` (ASTC, for iOS and Apple silicon), `encoder-etcpak` (ETC2,
  for Android) and `encoder-bc7f` (BC7, for Windows). Its default features also
  enable two encoders that link prebuilt ISPC kernels, and there are no prebuilt
  kernels for iOS or Android, so the default build fails on both. The three
  encoders above are C++ that the `cc` crate compiles with each platform's own
  clang, so no ISPC compiler is needed anywhere.
- **On Android the app bundles `libc++_shared.so`** from the NDK, because those
  encoders link against it.
- **Windows needs the MSVC build tools** (`cl.exe`, `lib.exe`) for the same C++
  and for `zstd-sys`, so it cannot be cross-checked from the Mac.

During play, two kinds of upload remain. The host hands each to the renderer
(`Renderer::hold`), which uploads what a frame allows (`upload_waiting`) and
returns the `AssetReady` of each upload for the host to give the client.

- **A full-size face**, when a card is first read. A worker reads or builds it
  and the frame thread uploads it, at most one face (0.84 MB) per frame. Until
  it lands, the board-size face is drawn in its place, enlarged.
- **A scan that had never been downloaded.** The download queue keeps Scryfall's
  limit of one image every 100 milliseconds, so a deck seen for the first time
  takes 15 seconds or more to fetch, and the match does not wait for it. Such a
  face starts as Beryl's drawn frame. The scan replaces it only in a frame with
  no animation playing, one layer per frame, so a card never changes its picture
  in the middle of a flight. One thread of `beryl_assets`' own waits for those
  downloads in the order they were queued and sends each face again made from
  its scan, under the same layer; the renderer holds it until the client is
  still (`Renderer::hold`).

**Beryl's drawn frame** (`beryl_assets`' `frame` module) is the web interface's
`Card` frame drawn into the texture: the battlefield frame (`card-field`) at
board size and the full frame (`card-full`) at full size, with the name, the
mana cost as Mana font symbols, a stand-in painting, and on the full frame the
type line and the rules text shrunk to fit its box, and the printed power and
toughness or loyalty. A token's body is pewter. `tiny-skia` draws the shapes,
parley sets the text and `swash` rasterizes it.

**An ability on the stack** shows a face of its own, listed after the cards and
tokens: its source's picture (scan or drawn frame) with the web interface's
`AbilityPlate` over the lower half, saying the ability. The client lists one
plate face for each paragraph of a face's rules text that says a triggered or
activated ability (`FaceList::plate`, by the kind and `nth` that
`StackItem::Ability` gives), and draws the source's own face for an ability
whose words are not one of those paragraphs. Plate faces add to the 150 faces
above.

The download queue and disk cache already exist in the Tauri shell. They move,
unchanged in behavior, into `beryl_art`, a small crate of the main workspace
that the Tauri shell and `beryl_assets` both use. They are not part of
`beryl_assets`, because the Tauri shell must not link Beryl 3D's texture
encoders (decision C17).

## Threads

The requirement is that nothing the app does in the background can delay a
frame. The design meets it by giving the drawing thread almost no work and no
reason to wait on another thread.

- **The frame thread** is the host's main thread. Each frame it drains its input
  channels into `handle`, calls `prepare` and `frame`, and submits the draw
  list. A script is compiled over several frames on this thread rather than in
  the `handle` call that receives it (`play::part`): compiling a long script
  whole was the costliest frame of most gated boards. `prepare` compiles every
  step that begins by the frame's time, every step that could change a segment
  under way then, and at least `STEPS_PER_ADVANCE` (4) steps, so the later beats
  of a long script are compiled well before they begin. A script that arrives
  with nothing playing or waiting begins compiling in the `handle` call that
  receives it. One that arrives behind another waits uncompiled, and begins in
  the `prepare` of the frame in which it starts, once every script before it is
  whole, since it is played from the table the one before it ends with. A script
  that needs every step as it starts is compiled whole then: one laid out again
  as it starts (`relay`), or handed a card the pointer or a prompt's tray holds.
  A new window or a scrolled row compiles every script whole. It holds no lock
  that another of Beryl's threads can hold, and it does no file or network I/O.
  The only thing it waits for is the display.
- **The session thread** runs the game, as today.
- **Search threads** run the AI, as today, on every core. They run at a lower
  scheduling priority than the frame thread, so the operating system preempts
  one when the frame thread wakes. No core is reserved. The app sets their
  scheduling class itself, and the frame thread's (user-interactive, or the
  highest priority on Windows). The search threads are in the default class
  (normal priority on Windows), the class the web app's session searches in, so
  that the AI searches as fast as it does there. The session thread puts itself
  in that class and the search threads inherit it. A thread otherwise inherits
  the class of the thread that made it, and in the prototype a game started from
  the frame thread searched at drawing's priority.
- **Asset workers**, two to four, decode, compose, compress and rasterize. Each
  puts itself in the user-initiated class as it starts (above-normal priority on
  Windows), below drawing and above the AI, since a worker started from the
  frame thread would otherwise share its class.
- **The frame log's writer** runs in the utility class, below everything else,
  since nothing waits for it.
- **The audio thread** takes the frame thread's sound messages and owns the
  output device, and the device's own callback thread mixes what plays. Workers
  decode the sound files beside it.

Communication is by channels only:

```rust
// Session thread to frame thread: typed values, no serialization.
updates: Receiver<GameUpdate>,
// Frame thread to session thread: commands. The session thread calls the
// function of `Sessions` each names and sends back its result as a `Reply`.
commands: Sender<SessionCommand>,
replies: Receiver<Reply>,
// Workers to frame thread: finished textures and glyph pages.
assets: Receiver<AssetReady>,
```

- **A click is asynchronous.** The frame thread takes the command from
  `Client::next_command`, sends it and draws the next frame. The session's reply
  (new marks, button states) arrives as an `Input::Reply`. The frame thread
  never calls a function of `Sessions` itself, because one can wait on a lock.
  The session computes a reply to a click in microseconds, so in practice it is
  drawn in the very next frame.
- **A message wakes the frame thread.** A thread that sends an update or a
  finished asset also wakes the host's event loop, so a still table responds at
  once.
- **The scheduling priority** of the search threads is chosen per platform by
  measuring both sides: with the table still, search throughput must stay within
  2% of what the current app achieves, and the app's log of late frames must
  show none while the AI thinks. While something animates, the AI loses what
  drawing uses and no more.
- **Why the default class.** The budget compares the search with the web app's,
  and the web app's session searches in the default class. Running Beryl 3D's
  search in the same class meets the throughput half of the budget by
  construction, on every platform, whatever the machine's other load. The
  utility class was measured too, over 20 pairs of whole games on the
  development Mac: it searched faster than default in 10 of them and slower in
  10, a median 0.99 of default's rate, and late frames while the AI searched
  were as many with either class ([Measurements](#measurements)). At those loads
  the run-to-run spread was far wider than 2%, so the measurement cannot show
  the budget met or missed; the choice of class can. Utility also lets the
  system run a thread on slower cores, which costs a phone's search most. The
  "no late frame while the AI thinks" half is not met at those loads with either
  class.

Why no core is reserved:

- **What drawing needs.** The worst frame is limited to 2 million instructions
  (see [Performance gates](#performance-gates)). On the floor's slowest CPU,
  which executes about 1.2 billion instructions a second, that is 1.7
  milliseconds: a tenth of a 16.7 millisecond frame. Most frames need far less.
- **What reserving costs on a recent iPhone.** It has two fast cores and four
  slow ones, and a fast core does about the work of three slow ones. Reserving
  one fast core removes about 30% of the phone's search throughput.
- **What reserving costs on the floor.** An iPhone 6s has two cores in all.
  Reserving one halves the AI.
- **What priorities cost.** The AI loses only the time drawing actually uses.

## The agent interface

Beryl 3D is built to be worked on by a coding agent from a terminal. Everything
an agent needs to know about what the client shows is available as text, as a
picture or as a test assertion, with no window and no browser.

The tool is held to these rules, because an agent meets it with no memory of the
last time it used it, and several agents often run it at once.

- **One entry point.** Every Beryl 3D command is `just b3d <command>`.
  `just b3d` alone lists the commands with one line each, and
  `just b3d <command> --help` gives that command's options with two examples.
- **Commands are words, not options.** `tree`, `png` and `check` are commands,
  so no combination of them is left undefined.
- **Nothing has to be guessed.** `list` names every recording, and `info` names
  every moment of one. An unknown name fails with the five nearest names.
- **Output can be pasted back.** Every printout begins with the command that
  prints it again. A line of the tree is a locator. A failing test's name is an
  argument.
- **One way to write a time, a moment, a locator and an action**, in every
  command, in test code and in a script that drives the app.
- **Output is bounded.** A printout that would be long is cut, says how much was
  cut, and names the option that shows the rest.
- **Two runs never share a file.** Every file a command writes is named for that
  run, and the command prints its path.

### Commands

```bash
just b3d list counterspell
just b3d info pay-default
just b3d tree pay-default --at prompt:1
just b3d png counterspell/cast --layout phone --size 393x759 --at 1250ms --tree
just b3d film verbs-zones --from 0ms --to 3s --frames 12 --crop 'card "Island" #7'
just b3d diff verbs-zones --from 800ms --to 1250ms
just b3d check pay-default
just b3d test b3d:counterspell
```

| Command                          | What it prints                                                                                                                                  |
| -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `list [prefix]`                  | Every recording whose name starts with the prefix: fixtures, card spec moments and stress boards, each with its length and its number of views. |
| `info <recording>`               | The recording's moments: each view with its time, each script's steps with their start and end, and each prompt with its buttons.               |
| `tree <recording>`               | The resolved elements at one moment, as an indented tree.                                                                                       |
| `png <recording>`                | The frame at one moment, drawn by the real renderer into a file. With `--tree`, the tree of the same frame as well.                             |
| `film <recording>`               | Frames across a span of time, as one picture laid out as a grid, with one line of text for each frame.                                          |
| `motion-film <verb> [case]`      | A verb's cases from both seats and layouts, filmed at 15 frames a second (`film --rate 15 --frame-width 760`) beside Arena's.                   |
| `diff <recording>`               | What changed between two moments: the elements that appeared, left, moved or changed state.                                                     |
| `check <recording>`              | Every interface check on every tick of the recording, and the faults.                                                                           |
| `test [prefix]`                  | Every interface test whose name starts with the prefix (see [Test speed](#test-speed)). `--every-tick` checks every tick.                       |
| `timeline <recording>`           | Each script's timeline, one line per step (see [Timeline parity](#timeline-parity)).                                                            |
| `gate`                           | The frame gates (see [Performance gates](#performance-gates)).                                                                                  |
| `app`, `smoke`, `frames`, `live` | The running app (see [Driving the running app](#driving-the-running-app)).                                                                      |
| `sweep`                          | Everything Beryl 3D checks after a push (see [Checks and isolation](#checks-and-isolation)).                                                    |

Every command that takes a recording also takes `--seat you|opponent`,
`--layout desktop|phone`, `--size <width>x<height>`, `--scale <number>`,
`--tier baseline|enhanced`, `--at <moment>`, `--do <action>` (which may be
repeated) and `--json`. Left out, they are `you`, `desktop`, 1600 by 900 (393 by
759 with `--layout phone`), 1, `baseline` and the recording's start, with no
action. A test's name, `b3d:<recording>/<seat>/<width>x<height>[@<scale>]`, also
names a recording and sets its seat, window and scale. The scale reaches the
client, which asks for its glyphs and icons at it and gives its clips in
physical pixels, so a picture at `--scale 2` shows what a Retina display does.
`diff` runs its actions after `--from`, so `--from` and `--to` at the same
moment show what an action changes.

`just b3d` is two programs behind one command. The commands that print text are
`beryl_inspect`'s program, `b3d`, which contains no GPU code. `png` and `film`
are a second program, `b3d-render`, in `beryl_pictures`, which has the renderer.
`just b3d` runs whichever the command needs, and both read their options with
the same code.

`stress-255`, used in examples below, is a stress board: a hand-built position
at the engine's capacity, defined in the [Performance gates](#performance-gates)
section.

### Edit-to-output time

Agents work in a loop of one edit and one command, so the time from a one-line
edit in `beryl_scene` to the last line of
`just b3d tree verbs-zones --at 1500ms` is held to a target of 3 seconds. It was
measured on 2026-11-15 on the 18-core Mac, five runs each, with the machine's
usual load beside each median:

| Dev profile                                                  | Median | Runs (s)                 | Load |
| ------------------------------------------------------------ | -----: | ------------------------ | ---: |
| Cargo's defaults                                             | 11.9 s | 11.6 11.1 12.9 16.7 11.9 |   43 |
| `debug = "line-tables-only"`, `split-debuginfo = "unpacked"` |  8.5 s | 7.5 8.5 8.7 7.7 8.6      |   54 |
| The above, and `opt-level = 1` for dependencies (not kept)   |  8.5 s | 8.2 8.1 16.0 9.6 8.5     |   70 |

The second row is kept. With no edit at all the command took about 4.5 s, made
of these parts:

- About 1 s starting `uv` and the tools package.
- About 0.5 s for cargo to find nothing to build.
- 3 to 4 s for the first run of the `b3d` binary. macOS checks every newly
  written executable before it first runs, in time that grows with its size and
  with the machine's load: 0.2 s for an empty program, 3.5 s for `b3d`'s 108 MB.
  Cargo copies the binary into `target/debug` again on every build, even one
  that changed nothing, so every run paid this.
- 0.4 s for the command itself.

`b3d` was that large because it linked the engine, the rules and every card: it
reached them through `beryl_session`, whose protocol types it needs, and through
the recorder that builds the recordings pack. Three changes followed:

- The protocol types moved into `beryl_protocol`, a crate of the main workspace
  that depends on `beryl_card`, `beryl_kernel` and `beryl_table` and on no
  rules, engine or card crate. `beryl_session` re-exports every type at its old
  path.
- The recorder became a program of its own, `record` in `beryl_record`. `b3d`
  and `b3d-render` exit with status 3 when the pack for the present sources is
  missing, and `just b3d` then runs `record` and the command again.
  `just b3d cargo-test` and `just b3d gate` run `record` first.
- `just b3d` runs the binary the linker wrote in `target/debug/deps`, whose file
  only changes when it is linked again, rather than cargo's fresh copy, so the
  macOS check is paid once per link instead of once per command.

`b3d` is now 11 MB. Measured again on 2026-11-15 by the same method, five runs
each, with a different edit in every run (macOS may remember its check of a
binary it has seen before):

| Case                    | Median | Runs (s)            |  Load |
| ----------------------- | -----: | ------------------- | ----: |
| A one-line edit, before |  8.5 s | 7.5 8.5 8.7 7.7 8.6 |    54 |
| A one-line edit, after  |  2.5 s | 3.2 2.3 2.4 2.5 2.6 | 43-47 |
| No edit, after          |  1.1 s | 1.1 0.8 1.1 1.7 0.8 | 32-33 |

The target is met. Most of what remains after an edit is the compiler:
rebuilding `beryl_scene`, then `beryl_inspect`, which depends on it, and linking
`b3d`. The first edit after a long pause took 11 to 15 s in two of the loops,
and later edits in the same loops took 3 to 4 s.

`just b3d png verbs-zones --at 1500ms` has a target of 6 seconds after the same
one-line edit. It was first measured on 2026-11-15 with `b3d-render` in
`beryl_3d`, a 134 MB executable that linked the engine through `beryl_session`:

| Run | Time (s) | Load (1 min) |
| --: | -------: | -----------: |
|   1 |     21.2 |           36 |
|   2 |     32.0 |           37 |
|   3 |     46.6 |           34 |
|   4 |     34.2 |           25 |
|   5 |     21.6 |           22 |

The median was 32 s: about 13 s of build after the edit, and macOS checking the
newly linked executable before its first run, which took 28 s at load 20 to 50.
Three changes followed, each measured by the same method on the same day:

- `b3d-render` moved into a crate of its own, `beryl_pictures`, a program with
  no library that depends on `beryl_inspect`, the renderer and the windowless
  host. `beryl_assets` reached `beryl_session` for one function,
  `decklist_faces`, which lists a match's faces from decklists through the card
  records; it is now behind the `decklists` feature, which only `beryl_assets`'
  own tests turn on. The executable is 74 MB.
- That left 2.6 CPU-seconds in the program itself, against 0.06 for `b3d tree`.
  1.2 to 1.6 s of it was the faces: a face whose scan is missing from the scan
  cache gets a placeholder, which is drawn and compressed on every run, since
  placeholders are never cached. 11 of the recording's 23 faces had no scan, and
  the encoder (`ctt` and the crates it uses) was unoptimized. The encoder and
  its scaler now build at opt-level 3, and the program takes 0.67 CPU-seconds.
- Every dependency builds at opt-level 1 in the dev profile. None is rebuilt
  after an edit to Beryl 3D's crates, and the executable shrinks to 28 MB, which
  links faster, passes macOS's check faster and runs in 0.37 CPU-seconds. A cold
  build of `b3d-render`'s dependencies took 2 minutes 18 seconds (584
  CPU-seconds) at load 70, paid once per change to a dependency or the profile.

| Case, one-line edit each run | Median | Runs (s)                                |  Load |
| ---------------------------- | -----: | --------------------------------------- | ----: |
| Before                       |   32 s | 21.2 32.0 46.6 34.2 21.6                | 22-37 |
| Own crate, no engine         | 17.1 s | 16.7 31.1 21.8 16.1 17.1                | 74-88 |
| And the encoder optimized    |  5.5 s | 10.8 59.2 5.5 4.3 4.3                   | 56-68 |
| The same, again              |  7.5 s | 11.0 7.5 10.1 7.4 5.9                   | 42-45 |
| And dependencies at opt 1    |  6.4 s | 6.6 7.2 9.4 6.1 6.2 6.1 5.1 6.1 7.9 7.6 | 52-68 |
| The same, later              | 12.4 s | 18.0 12.4 10.1 9.0 12.7                 | 22-25 |

The target is met only some of the time. The same change measured an hour apart
gave medians of 6.4 s and 12.4 s, at a lower load the second time: every crate
compiled about twice as slowly, so the load average does not capture all of the
machine's state. With no edit the command takes 1.1 to 1.3 s.

What remains after an edit, from `cargo build --timings` and separate runs:

- The build, 4 to 11 s. `beryl_scene` takes 1.3 to 4 s at opt-level 2 (kept for
  the interface tests' speed). Then `beryl_assets` and `beryl_inspect`,
  `beryl_render`, `beryl_host` and `beryl_pictures` rebuild in series, because
  each depends on the one before, at 0.5 to 2.5 s each. The link takes 0.5 to
  1.5 s of `beryl_pictures`' step.
- The first run, 1.5 to 7 s: about 1 s more than a second run, which is macOS's
  check of the new 28 MB executable, and 0.8 to 1.4 s for the program itself.

A faster loop would need fewer crates in that series, or the dependencies that
never change linked as a dynamic library, so that each edit links and macOS
checks a small executable. Neither is done.

Both commands were measured again on 2026-11-16, after milestone 2, by the same
method: five runs each, a different one-line edit to `beryl_scene` in every run,
in a worktree that had built once. `beryl_scene` had grown from about 11,000
lines to about 48,000 since the first measurement.

| Command, one-line edit each run | Median | Runs (s)               |  Load |
| ------------------------------- | -----: | ---------------------- | ----: |
| `just b3d tree`                 |  6.0 s | 6.0 8.9 5.4 4.0 17.7   | 16-18 |
| `just b3d png`                  |  7.3 s | 6.2 6.3 7.3 119.2 21.2 | 15-16 |

Neither target is met. The first `tree` and the first `png` in the fresh
worktree each took about 92 s, rebuilding from the cached build directory.

Timing each part of `tree` showed that most of the time went to waiting, not
compiling. Five runs, at load 14 to 22, with the build's and the program's waits
for CPU tokens measured inside the commands:

| Run | Total (s) | Build's wait (s) | Build (s) | Program's wait (s) |
| --: | --------: | ---------------: | --------: | -----------------: |
|   1 |       5.7 |              0.0 |       4.7 |                0.0 |
|   2 |      10.7 |              5.2 |       4.0 |                0.6 |
|   3 |      31.3 |             23.8 |       5.5 |                0.9 |
|   4 |      23.8 |             17.9 |       4.8 |                0.0 |
|   5 |      25.7 |              3.6 |       5.2 |                8.0 |

- **The token pool made the command wait.** An agent's lease gets no token while
  a push or the merge queue waits for tokens, or while agents and slow CI hold
  their three quarters of the pool. The build waited up to 24 s for its first
  token, and the program, which wants one token and runs for 0.4 s, up to 8 s.
  Now `just b3d tree`, `png` and `film` build with the cargo shim's floor: cargo
  takes the tokens free at that moment without waiting, and when none is free it
  starts on two job slots and grows as tokens come free
  (`cargo_pool.FLOOR_ENV`). Their programs, which size no threads by their
  lease, run at once as if inside a lease of one token. An incremental build
  uses under two cores for a few seconds, too little to slow the checks.
- **Half the compile was ThinLTO.** At opt-level 2 rustc runs ThinLTO across a
  crate's own codegen units, and in `beryl_scene` that pass took 1.8 to 2.1 s of
  the crate's 3.2 to 3.5 s. The `dev` profile now sets `lto = "off"`, and
  rebuilding `b3d` after an edit takes 1.6 to 2.5 s instead of 4.1 to 4.5 s.
  Without that pass every interface test took 195 CPU-seconds instead of 148, so
  the `test` profile, which `just b3d test` and the fast check use, keeps it.
- **Some builds stalled on the disk.** In one stretch of about ten minutes,
  builds with no pool at all took 20 to 33 s for 7 CPU-seconds of work, while
  the machine's disk served about 27,000 operations a second. Each incremental
  build of `beryl_scene` writes its dependency graph and query cache again,
  about 155 MB. No change here addresses that.

Measured again the same day after both changes, by the same method. Each edit
changes one line inside `beryl_scene::table::place`, a function every `tree`
runs:

| Command, one-line edit each run | Median | Runs (s)            |  Load |
| ------------------------------- | -----: | ------------------- | ----: |
| `just b3d tree`                 |  3.0 s | 3.9 3.2 2.4 3.0 2.9 | 13-15 |
| `just b3d tree`, again          |  2.4 s | 2.8 2.4 3.0 2.3 2.4 | 12-15 |
| `just b3d png`                  |  4.5 s | 4.2 3.9 4.6 4.5 4.5 | 10-13 |
| `just b3d tree`, after `png`    |  3.2 s | 3.1 3.1 3.5 3.4 3.2 | 12-14 |
| `just b3d tree`, again          |  2.9 s | 2.9 3.2 2.3 2.9 3.1 | 14-15 |

Both targets are met, `tree`'s narrowly: the median of all 20 runs is 3.0 s.
What remains of `tree` is about 0.6 s of starting `uv` and cargo, 1.3 to 2.1 s
of `beryl_scene`'s compile, mostly rustc reading its incremental state for a
48,000-line crate, 0.4 to 0.7 s of `beryl_inspect` and the link, and 0.1 s of
the program. A faster loop would need `beryl_scene` split into smaller crates.
`png` builds its own `beryl_scene`, with other features, so the first `tree`
after a `png` compiles the edit again.

### Times, moments, locators and actions

These four are written one way everywhere.

- **A duration** is a number and a unit: `850ms` or `1.25s`. Every command reads
  both units and prints milliseconds.
- **A moment** is what `--at`, `--from` and `--to` take. It is a duration since
  the recording's start, or a name that `info` prints: `start`, `end`, `view:12`
  (the first frame in which the twelfth view is still), `prompt:1` (the first
  prompt, open and still) or `step:12.0.1` (the middle of the second step of the
  first beat of view 12's script). A name may be followed by an offset, as in
  `view:12+250ms`.
- **A recording plays on a clock of its own.** Each recorded reply is given 400
  milliseconds after the table comes to rest in the view it answers. Every
  moment therefore has the same time on every run.
- **A locator** is the start of a tree line: `button "Auto Pay"`,
  `card "Island"`, `card "Island" #8` or `card #8`. `>` looks inside another
  element, as in `region "prompt" > button "Cancel"`. A name matches whole and
  exactly. A locator that matches nothing, or more than one element, fails, and
  the failure lists the elements with the nearest names and says how to narrow
  it.
- **An action** is one line: `press <locator>`, `hover <locator>`,
  `menu <locator>` (a right click), `drag <locator> to <locator>`,
  `hold <locator> over <locator>` (a drag that is not let go, to see what a drag
  shows while it is over something), `type <locator> "<text>"`, `key <name>`,
  `move <x>,<y>`, `wheel <locator> <x>,<y>` (turn the mouse wheel over an
  element by that many points; a negative `y` scrolls toward the end),
  `step <duration>` or `still` (run the clock until nothing moves). A point
  `<x>,<y>` of the window may stand in place of any locator, so
  `drag 700,500 to card #8` starts a drag on the empty table. `--do` takes one
  action. A drive script is a file of them. The running app's socket reads them.
  The test driver's methods have the same names.

### What is printed

The tree's first line is the command that prints it again, and its second says
where in the recording it is. Every line after that has the same fields in the
same order: role, name, id, place, the top left corner and the size of the
element's box on screen in logical points, and then its states.

```text
just b3d tree pay-default --seat you --size 1600x900 --at 1250ms
view 12 of 31, t=1250ms, animating, 143 elements
game
  table
    card "Counterspell" #41 stack 1180,402 116x162 moving(fly 62%)
    card "Island" #7 battlefield.yours.lands 412,640 96x134 tapped
    card "Island" #8 battlefield.yours.lands 516,640 96x134 interactive eligible
    ... 22 more in battlefield.yours.lands (--within battlefield.yours.lands)
  layer
    region "prompt" #100 520,820 560x64
      text "Pay {1}{U}" #101 536,838 180x28
      button "Auto Pay" #102 880,832 96x40 interactive
      button "Cancel" #103 984,832 80x40 interactive
```

- **Every line names its element's id**, so every line, pasted back, locates
  exactly its own element. A card's place is its zone, then `yours` or `theirs`
  (except on the stack), then its battlefield row. A revealed card's face in the
  Revealed Cards pile is in the place `revealed`. An element of the flat layer
  has no place. A name with a quote or a backslash in it is escaped with a
  backslash.
- **States** are words from a closed list: `interactive`, `tapped`, `face-down`,
  `moving(<verb> <percent>)` (`moving` alone until an element carries its verb),
  `hovered`, `held`, `eligible`, `selected`, `target`, `disabled`, `focused`,
  `phased-out`, `picked`, `primary` (the button a prompt expects, drawn amber),
  `pile(<count>)` (on the top card of a battlefield pile), `copy("<name>")` (the
  card a copy really is), `covered(<percent>)` and `faded(<percent>)`.
- **`covered(<percent>)`** is how much of the element other elements hide, and
  **`faded(<percent>)`** how far its opacity is below 1. Neither is printed
  under 1%.
- **A place holding more than 12 cards** prints its first three and a count.
  `--within <place or locator>` prints one part whole, `--role <role>` prints
  one role, and `--full` prints everything.
- **A film** is at most 24 frames, in a grid no wider than 2,000 pixels, each
  frame labelled with its time. `--crop <locator>` follows one element, so a
  flight can be read at full size. With the file's path, `film` prints one line
  for each frame: its time and what was moving.
- **`--json`** prints the same content as JSON, for a script.

A fault names the rule, the elements, the moment and the command that shows it.
The same fault found in several seats or windows is printed once, with a count:

```text
FAULT overlap  b3d:pay-default/you/375x647 t=850ms  (and 3 more tests)
  button "Auto Pay" #12 (188,598 96x44) overlaps button "Cancel" #13 (270,598 80x44)
  show: just b3d tree pay-default --seat you --layout phone --size 375x647 --at 850ms
```

`check` and `test` print at most 20 faults, then a count of the rest, then one
closing line with the tests run, the faults found and the time taken. They exit
with 1 when there is a fault and with 2 when the command itself was wrong.

### The test driver

Tests drive the client inside the test process with an API shaped like
Playwright's, which the current browser checks already use. It needs no page, no
server and no waiting, because the test owns the clock.

```rust
let mut page = Page::fixture("pay-default", Side::You, Layout::Desktop);
page.still();
page.press("button \"Auto Pay\"");
page.still();
assert_eq!(page.find("card \"Counterspell\"").zone(), Some(ZoneName::Stack));
page.check();

// Several cards share a name: narrow the locator.
page.hover("card \"Island\" #8");
```

- **Locators and actions** are the ones defined above. `press`, `hover`, `drag`,
  `type_text`, `key`, `step` and `still` are methods, and `page.run(script)`
  takes the same lines as text. An action enters through `handle` as pointer and
  key inputs at a point where the element is uncovered, the same path a real
  click takes.
- **Nothing waits by itself.** This is the one difference from Playwright. An
  action happens at the clock's present time, and the clock moves only when the
  test calls `step` or `still`. A failed locator or assertion therefore says
  where the clock is and what was still moving, so a missing `still` shows in
  the failure.
- **A tick** is one step of a test's clock, a sixtieth of a second unless the
  test says otherwise.
- **Where an action lands.** `press`, `hover`, `drag` and `type_text` act at the
  centre of the element's largest uncovered rectangle. A press is a pointer down
  and up at one point. A drag is a down, eight moves evenly spaced along the
  straight line with a 16 millisecond `step` after each, and an up.
- **The recording answers itself unless the test answers first.** 400
  milliseconds after the table comes to rest, the clock gives the next recorded
  view, as `step` and `seek` run. A press that sends a command is answered from
  the recording's responses by the web interface's fixture rule: each command
  has a queue of responses, the first answers it and is used up unless it is the
  last, and one recorded with its request answers that request alone. A
  `choose`, or a `click` whose reply says it answered, plays the recording on.
  The driver does not rewind a recording, so `undo` and `cancel` fail.
- **Moments.** `page.moments()` lists the recording's named moments with their
  times, and `page.seek(moment)` goes to one, starting over when it lies in the
  past. Views that the game plays on through (thinking or playing) arrive
  together and share one moment. `page.steps()` lists each script step with its
  start and end, and a `step:` moment is the middle of its step.
- **`check()`** runs every interface check on the current frame.
- **Where it lives.** `Page` is in `beryl_scene`, behind a Cargo feature named
  `driver` that the app does not enable. `Page::new` takes a recording, so
  `beryl_scene` never runs the engine. `Page::fixture` and `Page::spec` are in
  `beryl_inspect`, which finds a recording by name in the file of recordings
  (see [Recordings](#recordings)).

### Driving the running app

An agent starts the app, drives it and reads what it did, with no person
involved (decision C16).

```bash
just b3d app --fixture verbs-zones --for 10s
just b3d app --drive hover-hand --load 8:utility --window background --for 20s
just b3d app --agent expert --for 600s
just b3d frames
just b3d live tree
just b3d live do 'press button "Auto Pay"'
just b3d smoke
```

- **`app`** runs the app. `--fixture <recording>` replays a recording instead of
  a game. `--pause-at <moment>` stops its clock there. `--drive <script>` plays
  a file of actions on the display's clock. `--load <threads>[:<class>]` starts
  busy threads, to imitate a loaded machine. `--slow-gpu <duration>` holds each
  finished frame back, to imitate a slow GPU.
  `--tier automatic|baseline|enhanced` sets the graphics setting for the run,
  and `--probe pass|fail` replaces the startup measurement's verdict, so that
  `just b3d tiers` does not depend on the machine's load. `--for <duration>`
  ends the run.
- **`--agent <name>`** starts a fresh game in which that agent of the session's
  answers for the viewer, so the AI searches with nobody at the table, and ends
  the run when the game is over. The opponent is `mcts-hidden` unless
  `--opponent` names another, and the seed is fixed unless `--seed` gives one.
  `--search-class <class>` puts the session thread and the AI's search threads
  in another scheduling class than default, to compare them (see
  [Threads](#threads)). At the end the app prints the rollouts and engine steps
  a second of the opponent's searches, and the frame log's summary counts the
  frames drawn while the AI searched: how many, how many late, and the worst
  client and encode times among them. The game keeps no replay, so neither app
  resumes it.
- **`--offer <agent>`** adds an agent to the start screen's opponents after the
  session's own list, such as `expert`, which that list no longer offers.
- **`--window`** says what is opened. `none` is the windowless host, which
  measures everything up to presentation, and is what an agent gets when it
  gives no `--window`. `background` is a window that never takes focus and opens
  behind the others. `foreground` is an ordinary window. `fullscreen` is what a
  person gets when they give no `--window` (decision C15). A run that an agent
  starts must give `--for`, so that it always ends: a windowless run has no
  window to close.
- **A drive script** is a file of actions, one a line, in `rust/beryl3d/drive`
  and named `<name>.drive`; `--drive` takes the name or a path. A line may begin
  `at <duration>`, the time since the host started at which it is given; without
  one it follows the line before it. `step` waits, `still` waits until nothing
  moves, and lines beginning `#` are comments. The host gives each action's
  inputs on the first display tick at or after its time, through the frame loop
  as it gives a person's, finds a locator in that tick's frame, and ends the run
  after the last action. A locator that finds nothing ends the run with an error
  that names the line. `hover-hand` sweeps the pointer across the hand for 20
  seconds.
- **Every run writes a frame log of its own** and prints the log's path with a
  summary: frames drawn, the rate the host asked for and its steps down and up,
  late frames counted against the rate asked for, frames held to 60 Hz when it
  asked for more, and how long frames waited for a drawable (the median, the
  95th percentile, the worst and the total). Its first line records the window,
  the display's rate, the load average and any drive script, busy threads and
  slowed frames. The `tier` line says how the tier was chosen at startup. Each
  pointer, key, text, surface and lifecycle input has a line of its own.
  `just b3d frames [path]` prints a log's summary and its worst frames. With no
  path it reads the newest log of a run started from the same checkout.
- **`live`** speaks to a running debug build over a local socket,
  `$TMPDIR/beryl3d-<process id>.sock`, which the app announces on its first
  lines: `live tree`, `live png`, `live do '<action>'` and `live quit`. When
  several apps are running it takes `--app <process id>`, and with none it says
  how to start one; a socket whose app has died is deleted and not counted. Each
  request has 10 seconds. The frame loop answers on the frame thread: `tree`
  prints the frame as `just b3d tree` does, after a first line with the app's
  clock (`t=1500ms`), `png` prints the path of a picture of the frame under
  `$BERYL_CACHE/b3d/out/live-<pid>-<n>.png`, drawn again offscreen at the
  window's size and read back without waiting on the frame thread, and `do`
  gives the action's inputs on the next display ticks, as a drive script's line
  is given, and answers when they are done or a locator found nothing.
  `just b3d release-socket`, a step of the sweep, builds release and fails if
  its binary holds the socket's file name, `beryl3d-{pid}.sock`.
- **`smoke`** starts the app with no window under a profile of its own, waits
  for its first presented frame and for `live tree` to show the start screen (a
  region named `start`), and quits it.
- **`stand-in`** plays a whole game against `expert` (offered with `--offer`) in
  the windowless app, answering every prompt through `live` by a fixed rule: the
  primary button when it is enabled, else the first enabled button, else the
  first eligible element. After 20 prompts it quits the app, starts it again and
  checks that the game resumed with the same board, then plays to the game over
  screen and prints the prompts answered, the outcome and the sounds the frame
  logs counted. It fails when a prompt stays open for 60 seconds of the app's
  clock, a card rests in `nowhere` (where a card goes that has left every zone)
  for a second of the app's clock with a prompt open, a `live` request fails or
  the game never ends.
- **`play --for <duration>`** starts the windowless app on a fresh game against
  `--opponent` (`mcts-hidden` unless another agent is named, which it offers
  with `--offer` and chooses on the start screen), waits until `live tree` shows
  the `game` region, and returns while the app runs. It prints the process id,
  the socket and the `live` commands to go on with. `--for` is required and at
  most one hour, because the app then ends itself. It is how the `ui-review`
  skill starts a player.

Release builds contain neither the socket nor the options that replay, drive,
load, slow or play the app for the viewer.

## Interface checks

An **interface check** is a rule about a frame's resolved elements that must
hold on every frame. The checks are generic: they know nothing about any card or
screen, so they cover every card spec that is ever generated.

- **On screen.** Every interactive element lies inside the window's safe area. A
  card needs only a square of 24 by 24 points (44 by 44 on the phone layout)
  inside it, because a hand is held partly past the bottom edge, as Arena's and
  the web interface's are.
- **No overlap.** No two interactive elements of the flat layer intersect,
  unless one contains the other in the tree. Cards are exempt, because rows and
  hands overlap them on purpose.
- **Text overlaps nothing.** No laid-out line of text crosses a line of another
  element's text or an interactive element of the flat layer, unless one of the
  two holds the other in the tree, as a button holds its label. Floats such as
  life totals and pools are laid out as text, so they count. Each line is judged
  where its clip shows it, a faded element (no more opaque than the audit's
  `transparent`) is not seen, and boxes may touch by a point, since a line's box
  includes its leading. The opponent's life total once sat on "On the draw" at
  the opening and no check saw it, because "no overlap" compares only
  interactive elements.
- **Reachable.** A hit test at the centre of the visible part of an interactive
  flat-layer element (its box cut to its clip, so a list row scrolled partly out
  of view is pressed where it shows) returns that element or one inside it, and
  no later element of the flat layer that draws a fill or a rim
  (`StateBits::PAINTED`) covers that centre. A scrim the pointer goes through
  leaves a button pressable, but a button drawn under it looks disabled. Every
  interactive card at rest has an uncovered area of at least 24 by 24 points (44
  by 44 on the phone layout) where a hit test returns it. The check judges cards
  only in frames where no card is moving, and a card that its step's own verb
  holds still between two of its moves (a strike's creature where its blow
  lands) counts as moving (`Track::paused`).
- **No duplicates.** No two elements share an id, a game object has exactly one
  scene object, and no two sibling elements of the flat layer share a role and a
  name. Arrows are left out of that last rule, since two arrows of one kind from
  one spell are named alike and told apart by their ids.
- **Text fits.** No laid-out line is wider or taller than its element's box.
- **Named.** Every interactive element has a role and a non-empty name.
- **Clips fit.** The frame asks for no more than 15 clip rectangles. A clip
  asked for past the last entry is dropped, and the records that wanted it are
  drawn whole.
- **Layer drawn.** A frame whose flat layer has records draws at least one of
  them inside the window. The layer is placed for the table's camera, so a frame
  that was not given that camera, as the start screen once was when no game was
  going on, draws its whole layer off screen and shows a black window.
- **Marks on their things.** Each mark's outline is centred on the card, ability
  or life total it rings, within 2 points, and the line it is drawn along goes
  round that thing's box on screen, falling inside it by no more than 2 points.
  The outline is a quad in the table's space, so the check projects it as the
  renderer draws it (`Frame::rings`), where the other checks read only the
  elements. Outlines drawn at seven tenths of their card's width passed every
  other check before this one.
- **Phone sizes.** On the phone layout, touch targets are at least 44 by 44
  points and text at least 11 points.
- **No jumps.** Where one of an object's segments ends and the next begins, the
  two poses are equal. Only `appear`, `vanish` and a script that starts the
  table over may place an object without a path.
- **Rest equals placement.** When a script has finished, every object is where
  its placement function puts it.
- **Verb values.** Each verb's measured duration, delay and stagger equal its
  data entry.
- **Every step shown.** Each step of each script is played, and what it concerns
  is on screen while it plays: inside the window (its centre, or one of the
  points a fifth of the way in from its corners, as the web interface measures
  it), at least half uncovered and not transparent. A step whose verb gives its
  subject time to travel moves it at least 4 logical points over at least 150
  milliseconds; a subject that jumps instead is a fault. A card that moves
  within its zone between two places out of view is drawn in no frame, and is
  not judged. Neither is a step that brings its subject back to where it started
  on the window, its start and end less than 4 logical points apart
  (`steps::rests_in_place`): the viewer has no change of place to follow. On the
  phone, a card flown from a graveyard into exile does this, because exile lies
  unseen under its owner's graveyard.
- **Not crowded.** No more than 3 different moves begin at once in one beat.
  Taps and the steps of a turn do not count.
- **No blank card.** Every card drawn face up, whose name the viewer knows, has
  a face layer of its own.
- **Lines hold.** An arrow or line whose two ends are at rest does not change
  shape: between two frames in which it is not moving, neither control point of
  its curve moves 4 points or more while its two ends stay within 0.1. It runs
  on each pair of consecutive frames the checks evaluate, which keep the arrows
  of the frame before (`agent::checks::keep_arrows`), and `just b3d check` says
  in its closing line on how many pairs it ran.

The last four are the web interface's stage audits (`ui/scripts/stage-audit.mjs`
and `audit.mjs`), with the same thresholds. The thresholds are in
`data/beryl3d/gates.json`, under `audit`.

What the checks run over:

- **Every fixture, every card spec, every stress board**, from both seats. The
  one exception is the `stress-verbs` board, which starts every verb's acts on
  100 objects together in one beat so that the frame gates draw every verb in
  the same frames. No game plays a beat like that: its steps crowd and cover
  each other on purpose. So "not crowded" and "every step shown" do not judge it
  (`stress::audited` in `beryl_inspect`), and every other check does.
- **A matrix of windows.** In the desktop layout: 1440 by 900, 1600 by 900, 1920
  by 1080 and 2560 by 1080. In the phone layout: 393 by 759 and 375 by 647, the
  two sizes the web interface's phone checks use, and 430 by 932.

How they are run fast enough to gate every push is the subject of the
[Test speed](#test-speed) section below.

A fix for an interface bug adds the broken state as a fixture, so the generic
checks cover it from then on. A scripted check for one screen is written only
when no generic check can express the rule, as for the web interface.

A small set of **renderer tests** covers what numbers cannot: they draw known
scenes into a texture and sample a few single pixels with a tolerance, to catch
a wrong texture, a blending mistake or a missing glyph.

## Test speed

Interface checks are only worth what they catch before a change lands. This
section specifies how Beryl 3D runs all of them before every push, in seconds.
The `beryl3d-interface` lane's `test` step (`just b3d test`) and the sweep's
`every-tick` step (`just b3d test --every-tick`) run them.

### What a test is

An **interface test** is one recording played from one seat in one window, with
every interface check applied. It is a pure function: the same three inputs give
the same faults, with no browser, no GPU, no engine and no clock.

```text
b3d:counterspell/cast/you/1600x900
b3d:counterspell/cast/opponent/375x647
b3d:verbs-zones/you/2560x1080
b3d:stress-255/you/393x759
```

`just b3d test` runs every test, and `just b3d test <start of a name>` runs
those that match. A test's name is also an argument to the other commands:
`just b3d tree b3d:counterspell/cast/you/1600x900` prints that test's recording
from its seat at its size.

Before its closing line, `just b3d test` prints how many frames and elements
each rule judged (`judged`; scripts instead of frames for the checks over a
script's segments), so a rule that passes because it judged nothing shows a
zero. 'Phone sizes' judges only the phone windows, so it shows a zero on the
desktop ones.

A window narrower than 500 points on its shorter side is shown in the phone
layout (`Layout::for_size`), so a test's size also picks its layout. Every phone
test is played with the same safe area, 44 points at the top and 34 at the
bottom (`pages::PHONE_SAFE_AREA` in `beryl_inspect`). These are test geometry,
fixed so that a test finds the same faults on every machine; the phone hosts
pass the device's own insets. The client lays the window out inside the safe
area and the checks hold its elements to the same insets, and `tree` and `check`
open a phone test's name with the same layout and insets.

While the phone layout is built, `just b3d test` and the sweep run the desktop
windows only, and `just b3d test --phone-only` runs the phone windows only. A
phone run is held to `data/beryl3d/test-faults.json` like any other, but it
never writes the list (`--update` does not go with it) and never counts a
desktop entry as stale, since it is never the whole run.

How many there are, as measured on 2026-11-15:

- 1,904 recordings from a seat: each card spec moment and fixture in the pack
  from each seat it is recorded for, and the two stress boards.
- Four desktop window sizes each, and three phone sizes under `--phone-only`.
- 7,616 tests in all, about 13,300 with the phone sizes.

### The target

These numbers are the targets on the development machine under its usual load,
and what `just b3d test` measured in the debug build `just b3d` uses: medians of
five runs each at load 86 to 101 on 2026-11-15.

| Measure                                | Target           | Measured               |
| -------------------------------------- | ---------------- | ---------------------- |
| CPU time for one test                  | 2 milliseconds   | 10.2 ms                |
| Tests per CPU-second                   |                  | 98                     |
| All the tests at their check times     | under 10 seconds | 9.1 s, 78 CPU-s        |
| The same tests checked on every tick   | under 2 minutes  | 9.9 s, 87 CPU-s        |
| Frames a test draws at its check times |                  | median 103, most 1,704 |
| Frames a test draws on every tick      |                  | median 166, most 2,889 |

The slowest tests are the four `stress-1024` tests, at about 1.7 seconds each.
The check times save less than the frame counts suggest: on every tick a test
draws 1.6 times as many frames but costs only 1.1 times as much. One reason is
that every judged step is sampled at every tick in both modes. Another is that
`stress-1024` draws more frames at its check times (152) than on every tick
(121), because each of 1,024 cards adds the start, middle and end of its
segments.

For comparison, the web interface's browser check of one animation takes 36
seconds at the 90th percentile, and the existing check of the session's scripts
for one card spec moment, which is plain Rust, takes 14 milliseconds in an
unoptimized build.

### Measured

`just b3d test` ran 7,552 tests on 2026-11-15: each recording from each seat it
is recorded for (1,888 pairs), in the four desktop windows. The first and last
rows are the median of five runs of the whole set on the 18-core Mac at the load
shown, and the rows between them single runs, all with the debug build
`just b3d` uses.

| Build                                                                                            | CPU time |  Clock | Load       |
| ------------------------------------------------------------------------------------------------ | -------: | -----: | ---------- |
| Before                                                                                           |  5,200 s | 29 min | 30 to 60   |
| Reachable check and fault dedupe sub-quadratic, `beryl_scene` and `beryl_inspect` at opt-level 1 |    270 s |   44 s | 40         |
| The above at opt-level 2                                                                         |    114 s |   17 s | 50         |
| Recordings shared between plays, clear cards skip the grid                                       |     91 s |   18 s | 55         |
| Pages opened without copying the recording or its views                                          |     75 s |   16 s | 300 to 330 |

Before, the four `stress-1024` tests took about 1,740 s each on the clock, three
fifths of all the time, because the "reachable" check measured each card against
every element of the frame and the faults were deduplicated by scanning a list.
They now take about 3.2 s each. With the check change alone, in the unoptimized
build, one `stress-255` test went from 14 s of CPU time to 1.4 s and one
`stress-1024` test from 1,740 s to 10 s on the clock.

A test now takes about 10 ms of CPU time on average, five times the 2 ms target.
A profile of the last row puts the rest in three places, none of them the
checks:

- About two fifths opening the page: inflating and parsing the recording's JSON,
  once for all its windows, and reading its views into the protocol's types,
  once for each of its four windows.
- About a quarter in the client handling each view: compiling its script and
  laying out the flat layer, whose text shaping alone is about an eighth.
- About a tenth building the draw list of each frame (`order::finish`,
  `Table::write`), which the checks do not read.

Skipping the frames between check times while something moves found the same
faults and saved nothing, so the cost is in the frames at the check times
themselves, about 100 a test.

### How a test costs a millisecond

Four things make a test this cheap. The first is the largest.

- **It checks the moments at which something can go wrong, not every tick.**
  Positions are functions of time with a known form, so a fault can only begin
  where something starts, ends, peaks or changes layout. A ten-second moment is
  600 ticks and about 20 such times.
- **It never runs the engine.** Recordings are made once and read from a file.
- **It never touches a GPU.** The checks read the frame's resolved elements.
- **It shares nothing.** Tests run on every core in one process, with no server,
  no page and nothing to wait for.

The **check times** of a test are:

- the frame after each input that changes the element tree or starts a script;
- for each segment: its start, its midpoint and its end, and for a flight the
  top of its arc;
- the middle and end of each transition of the flat layer's screen (its first
  frame needs none: an element fading in is wholly transparent there and cannot
  be pressed, and an element moving still stands where the frame before judged
  it);
- the first frame in which everything is still after each script.

Four checks need no frame at all. "No jumps", "rest equals placement", "verb
values" (so far, each step's duration) and "not crowded" read the segments and
steps directly. "Every step shown" samples the step's subject at every tick
while the step plays, in both modes, and judges the samples together. It judges
the share of frames in which the subject is seen, so a sparser sample judges the
same step differently; the differential test below found that. Those frames are
drawn and sampled, but the frame checks do not run on them.

Checking every tick remains, as the second line:

- **In the sweep**, after a push, the same tests run again with the checks
  applied on every tick (its `every-tick` step, `just b3d test --every-tick`),
  held to the same `data/beryl3d/test-faults.json`. This is the exhaustive form.
  It is not part of slow CI, whose budget of CPU time it would spend several
  times over (see [Checks and isolation](#checks-and-isolation)).
- **A differential test** holds the two together
  (`beryl_inspect/tests/all/differential.rs`). On a fixed sample of 200 tests,
  drawn with a fixed seed from every test but the stress boards', it plants each
  kind of fault at a seeded random tick inside one of the test's steps, and
  requires every fault the every-tick run finds to be found at the check times
  too. A fault only the every-tick run finds is a missing check time, and is
  fixed by adding one. The 1,000 plantings take about 2 seconds in `cargo test`.

### Recordings

A recording is the stream of session updates for one fixture or card spec
moment. Making one means playing the engine and the session, which costs more
than the test that reads it.

- **Recordings are packed into one file** by `beryl_inspect`, which calls the
  code that records fixtures and card spec moments today (`beryl_cli`'s
  `fixture` and `specs` modules, made a library for it). The file is under
  `$BERYL_CACHE/b3d`, which every worktree on the machine shares.
- **The file is cached** under a hash of everything a recording depends on: the
  engine, rules, session and card sources, the fixture definitions and the verb
  data.
- **A change to client code alone leaves the hash unchanged**, so the tests read
  the file and run no engine.
- **A change to the engine, a card or the session changes the hash**, and the
  whole file is built again, once. Of several sessions working at one staging
  commit, one builds it and the rest read it. Building it costs about a minute
  of CPU time, is reported apart from the tests' time, and is not part of their
  target. The sweep builds the file for each staging commit it checks, so a
  session that has just rebased usually finds it there.
- **`beryl_inspect::recordings` is the only way in.** `messages(name, seat)`
  gives one recording's messages and `list()` every recording with its length
  and number of views. A fixture is named as in `ui/fixtures.json` and a card
  spec moment as `<card>/<moment>`. The `scene-purity` check fails on any other
  Beryl 3D code that names `beryl_cli`.
- **How the file is laid out.** It is `recordings-<hash>.bin`, in postcard: each
  recording's name, seat and number of views, and its messages as deflated JSON
  (the views of a recording repeat, so deflating makes the file about 30 times
  smaller), or why it could not be recorded. The builder holds
  `recordings-<hash>.lock`, writes under a temporary name and renames. Building
  or reading a pack sets its modification time, and a pack used in the last six
  hours is kept, as are the newest three, up to 48 packs (`pack::prune`). When
  only the newest three were kept, the worktrees building packs for their own
  sources deleted a fast check's pack while its tests still read it.
- **Card spec moments and fixtures are recorded from both seats.** A fixture's
  definition names the seat its stop is seen from. From the other seat it is the
  same game, with the same stand-in answers and clicks and the same stop, with
  the session drawing that seat's view of each update as well
  (`Sessions::with_across`). That seat never decides in it. A manual table
  fixture has one viewer, and a prop stands only in its human's view, so those
  are recorded from their own seat alone.

Measured on 2026-11-15, each the median of five runs at load 40 to 50 on the
development Mac, in an unoptimized build: building the file from nothing takes
23 s on the clock and about 64 s of CPU time, with a peak of 3.3 GB of memory,
for 1,532 recordings; the file is 9.1 MB; and opening it and reading one
recording takes 0.19 s, most of it hashing the sources.

### What keeps it fast

- **A counted limit.** The number of frames a test evaluates is deterministic.
  Its median and its maximum over all tests have limits in the gate file, so a
  change that makes tests evaluate more frames fails before it makes them slow.
- **A measured report.** Each run reports tests per CPU-second. A run over its
  time target files a task to speed it up, as the project's other checks do. No
  target is raised to make a run pass.
- **A small test program.** The program that runs the tests depends on the
  client core and on no GPU crate, so rebuilding it after an edit is quick.
  Rebuilding is expected to take longer than running the tests.

### At 50,000 cards

The engine's goal is 50,000 cards. At about three moments a card that is 2.1
million tests over the full matrix, or roughly 50 minutes of CPU time.

- **Before a push** the tests run at one desktop size and one phone size:
  600,000 tests, about 15 minutes of CPU time, about a minute across the
  development machine's 18 cores.
- **In the sweep** they run over the full matrix.
- **Nothing is chosen by guessing what a change affects.** A change to the
  client core can break any card's moment, so all of them run.

## Performance gates

The performance gates turn "never drops a frame" into a property that a check
holds on every change, without a device and without timing noise. They count the
client core's work, which is identical on every run.

### What is counted

Two kinds of work are counted: what the client core does on the CPU, and what
the draw list asks of the GPU.

On the CPU:

- **Instructions per frame**, under Callgrind in a Linux virtual machine of
  Beryl 3D's own, `beryl3d-bench`, made the way the engine's `beryl-bench` is. A
  second machine keeps the engine's counts from moving when Beryl 3D installs a
  graphics driver, and keeps either gate from waiting for the other.
  `just b3d vm` creates and provisions it
  (`tools/src/beryl_tools/beryl3d/vm.py`), with Mesa's Vulkan drivers added to
  what `beryl-bench` installs. The counted boundary is one call to `frame` plus
  the `handle` calls since the previous frame.
- **Allocations in `frame`**, from a counting global allocator in a native test.
  The limit is zero once a game has started. `handle` may allocate, and its
  allocations are counted and limited per fixture.
- **Peak heap bytes** of the client core over a fixture.

On the GPU, time cannot be counted, but work can:

- **Fragments shaded per frame.** The renderer draws the frame in the virtual
  machine on a software Vulkan driver and reads the driver's own count of
  fragment shader runs (a pipeline statistics query). The count is exact and the
  same on every run, and it leaves out pixels the depth buffer rejected.
- **Cost per fragment, per material.** Each material's shading is a WGSL
  function of its own, named `shade_` and the material. A test parses the
  shaders with `naga`, the shader compiler `wgpu` uses, and counts each such
  function's texture samples and its expressions. Each count has a limit, and no
  Baseline material may take more than one texture sample per fragment.
- **Fragments times expressions**, summed over materials, is the frame's
  estimated GPU cost. It is the nearest equivalent of an instruction count.
- **Arm's offline shader compiler is not used.** It would give the cycles a
  fragment costs on a real Mali GPU. It is not installed on the development
  machine, and its installer needs a person to accept a licence, so no check
  depends on it.
- **Draw calls, instances, upload bytes and blended area**, which the client
  core computes as it builds the draw list. These need no renderer, so they are
  checked on every tick.

The driver's count was measured before the gate was built (2026-11-15, Mesa
26.0.8's llvmpipe Vulkan driver in `beryl3d-bench`, `wgpu` 30.0.2). One opaque
quad of 4,096 pixels drawn in front of one of 16,384, with a depth buffer, gave
20,480 fragment shader runs drawn far first and 16,384 drawn near first, on
three runs out of three, with default limits and with
`Limits::downlevel_webgl2_defaults()`. So the count is repeatable and leaves out
the fragments the depth buffer rejects, and the gate reads it from the driver.

`just b3d gate --gpu` runs both GPU counts
(`tools/src/beryl_tools/beryl3d/gpu.py`) and is a step of the sweep.
beryl_render's `shader_cost` test counts each material and holds it to
`materials.<name>` in `data/beryl3d/gates.json`; it needs no GPU, so the lane's
tests run it too. beryl_render's `fragments` example draws the 8 frames of each
gated board that `rank` picks in the virtual machine, twice in each tier, and
the gate fails if the two counts differ or the worst frame's count, in screens,
is over `tiers.<tier>.fragments.<board>` (during the build-out, only over the
tier's target, 1.75 screens for Baseline and 4.0 for Enhanced; see "During the
build-out" and "The Enhanced tier"). A frame's count is the sum over its render
passes, one query each. Only Vulkan and DirectX 12 offer the pipeline statistics
query in `wgpu`, so the count cannot be taken on Metal.

### What it is held to

The gate holds each fixture's **worst single frame** as well as its total. A
total is the right measure for search throughput. For frames it is not: one 30
millisecond frame vanishes in an average over 5,000.

The gated fixtures are of two kinds.

- **Recorded games**, the same recordings the interface checks play. At least
  one of them opens prompts (`pay-default`), so that the worst frame includes
  one in which `handle` builds a screen and lays out its text.
- **Stress boards**: positions built by hand at the engine's capacity, where a
  frame is most expensive. The engine keeps a game in a compact form that holds
  255 objects, and moves a game that outgrows it to a larger form that holds
  1,024. The stress boards are:
  - `stress-255`, a battlefield of 255 objects;
  - `stress-1024`, a battlefield of 1,024 objects;
  - a 60-card hand, fanned, with the pointer moving across it;
  - every verb playing at once on 100 objects;
  - a stack of 50 spells and abilities with arrows to their targets.

The limits live in one file of Beryl 3D's own, `data/beryl3d/gates.json`. The
first board below is played for 1,200 frames, and the tolerance is explained
after it:

```json
{
  "frames": {
    "stress-255": {
      "worst_frame_instructions": 2000000,
      "total_instructions": 1450000000,
      "frame_allocations": 0
    },
    "stress-1024": { "worst_frame_instructions": 4000000 }
  }
}
```

Where the worst-frame limits come from:

- **The floor's slowest CPU** is the 2015 Android phone's: a Cortex-A53 class
  core at about 1.4 GHz, executing about 1.2 billion instructions a second. The
  iPhone 6s and the 2013 laptop are several times faster.
- **The floor's displays refresh 60 times a second.** Every device with a 120 Hz
  display has a CPU more than five times faster than this one, so the 60 Hz
  budget on the slowest CPU is the binding one.
- **Half a frame** at 60 frames a second is 8.3 milliseconds, which is about 10
  million instructions. The other half is left for the renderer and the GPU
  driver.
- **`stress-255` is limited to 2 million**, a fifth of that, leaving a factor of
  five for cache misses, which instruction counts do not see.
- **`stress-1024` is limited to 4 million**, leaving a factor of two and a half.
  A game reaches the larger form only when a board outgrows 255 objects.

Counts are exact, so the tolerance is not for noise. It absorbs the small shifts
that happen when a change to unrelated code alters how the compiler inlines and
lays out this code, as the engine's gate does.

The numbers above are targets. The first measurement of the real client replaces
them, and after that the rule is the engine count gate's:

- **Limits go down.** A limit is lowered whenever a change lowers the count.
- **A limit rises for two reasons only**, named in the commit with the old and
  new counts: the work itself changed (a new verb, a new element on every
  screen), or a compiler or dependency upgrade moved the count with no change to
  Beryl's code.
- **No limit waits for anyone.** Neither kind of rise needs the owner's
  approval. The commit that raises a limit states its reason, and that is the
  record.
- **The change that moves a count moves its limit.** The native gate runs in a
  lane of its own, `beryl3d-gate`, in every fast check of a Beryl 3D change, so
  a change that moves a work counter, the load or an allocation count fails its
  own push until it lowers or raises that limit in the same commit. A change
  that raises a work counter's limit because it added work to every board (a new
  element on every screen, a verb a gated board plays) also runs
  `just b3d gate --counts` and moves the instruction limits in the same commit,
  since that work moves them too. Otherwise the sweep finds the instruction
  counts past their limits an hour later and files a bead for another session to
  re-pin them. On 2026-11-16 nine such beads were filed in one day, most of them
  because the native gate, behind the workspace's tests in one lane, had given a
  result in only 100 of 327 fast checks.

#### During the build-out

While Beryl 3D gains a verb or a screen every hour or so, a 1% tolerance cannot
tell new work from a regression, so every verb failed the sweep. From 14 to 16
November 2026 the sweep filed 21 failure tasks for Beryl 3D; the `gpu` step
failed all 11 of its runs, each settled by raising a limit, and
`data/beryl3d/gates.json` had 43 commits, most of them raises for a new verb. In
one failure, text and layout added by new verbs raised the counts by 5 to 66%,
while a search over every card for every moving card, quadratic in the board,
raised them several times more. Both looked alike, and the commit that
recalibrated the limits took the quadratic search in with the new verbs:
`stress-1024`'s total went from 80.7 to 761.5 million instructions and
`stress-255`'s from 39.7 to 225.5 million.

So, until `BUILD_OUT` in `tools/src/beryl_tools/beryl3d/build_out.py` is set to
`False`, the sweep's count gates fail only on what a feature cannot explain:

- **Instruction counts may pass their limits by 20%**, one constant for every
  board, and a count below its limit passes, or every cut would file a task too.
  `--counts --update` still lowers the limits.
- **Fragments over a board's limit are a warning.** Only a frame over the
  Baseline target of 1.75 screens fails.
- **The scaling check holds at every velocity.** `stress-1024` is `stress-255`
  with four times the cards, so the ratio of its instruction counts to
  `stress-255`'s, under `scaling` in `gates.json`, may not pass a bound set 15%
  over the ratio measured once the work was linear. Because both boards share a
  fixed cost (the interface, text and layout), linear work gives a ratio well
  under four. The quadratic search above gave 3.4 for the totals where the bound
  is 2.4, so it fails however many verbs land beside it. A verb that adds work
  for each card raises the ratio a little; its commit may raise the bound with
  the old and new ratios, and the native `elements` and `placed` counters, which
  grow with the cards, show whether the new work is per card.
- **The native gate is unchanged.** Its work counters are exact and cheap, and
  the commit that raises one explains it.

`BUILD_OUT` goes back to `False` when feature work slows down, and at the latest
when the owner switches the default client to Beryl 3D (C11). The same commit
lowers every limit with `--counts --update` and `--gpu --update`, so that the 1%
tolerance and the fragment limits start from what is measured.

### Finding the worst frame

Callgrind counts a whole run. Counting every frame separately would mean
thousands of measurements, so the gate finds its candidates first.

1. The client core keeps cheap **work counters** as it runs: elements visited,
   segments evaluated, layout nodes computed, records written, objects placed.
   They are deterministic.
2. A native pass plays the fixture and ranks its frames by those counters.
3. One run of `b3d-frame-counts` under Callgrind plays the fixture again, with
   every frame sent through a wrapper function, `frame_counted`, which makes its
   `handle` calls and its `frame` call. Around each call the program asks
   Callgrind, through Valgrind's client requests, to collect and then stop, and
   after it to write the counts out and zero them, so dump `n` holds frame
   `n - 1`. (Callgrind's own `--dump-after` option dumped only once on arm64,
   because Callgrind lost track of the wrapper's returns.) The four fixtures run
   at once. An earlier version ran one Valgrind process per counted frame
   through Gungraun, each replaying the fixture up to its frame: about 100
   processes and 25 minutes, during which every other session waited for the
   machine.
4. The gate's worst frame is the largest frame count, and its total is their
   sum.

- **A check holds the counters to their purpose.** The frame with the most
  instructions must be among the 8 the work counters rank highest, so that the
  native gate, which reads the counters in seconds, keeps seeing the work that
  costs instructions. When it does not, the fix is a counter that sees that
  work.
- **Only the Beryl 3D sweep runs the count gate.** No change waits for it before
  pushing, except one that adds work to every board, which moves the instruction
  limits itself (see above). A session checks a change's frame cost on the Mac
  with the native gate and the `frame_abba` timing, and a sweep that finds a
  count past its limit files a bead like any other sweep failure.
- **The work counters are also limited natively**, on every tick, in the check
  that runs before each push. That catches most regressions without the virtual
  machine.

The native gate is `just b3d gate`, the step of the `beryl3d-gate` lane
(`tools/src/beryl_tools/beryl3d/gate.py`). The `gate` binary of `beryl_inspect`
plays each gated board as a host would at 60 frames a second
(`beryl_inspect::gate::Playback`). The first view is handed over before the
first frame, and each later one once 30 frames have passed since the one before
and the table has nothing left to animate, so every view's script plays out. A
recorded view that the session follows unasked arrives with the views after it,
as the test driver hands them over. The gated boards are the seven of
`beryl_inspect::gate::BOARDS`: the five stress boards (`stress-255`,
`stress-1024`, `stress-verbs`, `stress-hand` and `stress-stack`) and the
recordings `verbs-zones` and `pay-default`. `stress-hand` is played with the
pointer moving over the hand, and `stress-stack` waits for the table to rest
before its last view. It prints every frame's work counters, its load and the
allocations made inside `Client::frame`, which a counting global allocator sees,
and the allocations made in `Client::handle` over the board. The gate holds them
to these limits under `frames.<board>` in `data/beryl3d/gates.json`:

- `worst_frame_<counter>` and `total_<counter>` for each work counter
  (`elements`, `segments`, `records`, `layout_nodes`, `text_runs`, `placed`,
  `steps`);
- `worst_frame_draw_calls`, `worst_frame_upload_bytes` and
  `worst_frame_blended_area` for the Baseline tier's load;
- `handle_allocations` over the board, and `frame_allocations`, which is 0.

The counters are exact, so they have no tolerance: one over its limit fails, and
the report names the board, the frame and the count. `just b3d gate --update`
lowers each limit to what was measured and never raises one. The allocation gate
is also a test of `beryl_inspect` (`tests/all/frame_allocation.rs`), which
prints the backtrace of the first allocation inside a frame.
`beryl_inspect::gate::rank` gives the 8 frames that the counters rank highest,
for the instruction-count gate.

An element is visited once for each object slot of the table and each node of
the flat layer that a frame looks at, whether or not it draws it, so a second
walk over the elements shows in the count. A segment is evaluated once for each
object whose pose comes from a track.

### What counts do not show

Counted work predicts a frame's cost. It does not measure a real device.

- **No floor device is part of the checks** (decision C13). Nothing confirms
  that a count under its limit means 60 frames a second on a 2015 phone. A
  software Vulkan driver does not model how a phone GPU removes hidden pixels,
  its memory bandwidth or its driver.
- **Where a count and a real device disagree, the device is right.** The count
  is then corrected or dropped. A counted number is a stand-in for device
  performance, and no design choice is made to improve a count at the expense of
  the device.
- **The frame log is the evidence from real devices.** The running app records
  every animation run in which a frame arrived late: which verbs were playing,
  how late, which tier was active, which GPU, and whether the AI was searching.
  A bug report carries the log, as it carries the web interface's today.
- **Frame time on the Mac, before and after a change**, is measured but not
  gated. Two builds of the client core, the one before a change and the one
  after, are timed in the same process in the order old, new, new, old, so that
  drift in the machine's load cancels. [Benchmarking](benchmarking.md) describes
  the method.

### Measurements

Every number here was measured from Beryl 3D itself on the development machine,
an Apple M5 Max with 18 cores, on 2026-11-15, with the machine's load average
beside each. The load came from other sessions' builds and tests. The prototype
that measured the design before Beryl 3D existed has been deleted.

The boards are the four gated boards, played as `beryl_inspect::gate::Playback`
plays them: a host's frames at 60 a second, each later view handed over once the
table has rested for 30 frames.

- **`stress-255`** and **`stress-1024`**: battlefields of 255 and 1,024 cards
  that each get a second view in which 40 of them fly to the end of the row. 127
  frames.
- **`verbs-zones`**: the recorded game whose scripts move cards between every
  zone, from the first seat. 1,093 frames.
- **`pay-default`**: the recorded game that opens prompts, from the first seat.
  84 frames.

**Instructions per frame**, counted under Callgrind in `beryl3d-bench` by
`just b3d gate --counts`. A frame's count is its `handle` calls and its `frame`
call. Three runs gave identical counts.

| Board         | Worst frame           | Total, every frame | Target for the worst frame |
| ------------- | --------------------- | -----------------: | -------------------------- |
| `stress-255`  | 3,055,119 (frame 30)  |         37,895,318 | 2,000,000: not met         |
| `stress-1024` | 11,210,755 (frame 30) |         79,392,870 | 4,000,000: not met         |
| `verbs-zones` | 2,085,813 (frame 894) |        100,894,992 | 2,000,000: not met         |
| `pay-default` | 347,285 (frame 30)    |          4,620,323 | 2,000,000: met             |

- **`verbs-zones`' worst frame was missed until every frame was counted.** The
  gate used to count only the 8 frames the work counters rank highest, and
  reported 878,544 instructions (frame 30). Frame 894 takes a view whose script
  compiles in `play::compile` and re-places rows in `table::place`, work no
  counter sees, so it was never ranked or counted. The gate's ranking check now
  fails on it until a counter sees that work.
- **The worst frame is the one whose `handle` takes a view with a script.**
  Compiling the script still re-places part of a battlefield row for each step
  that moves a card in it. by-q44s.22 continues that work. Every other frame is
  within its target: a flight frame of `stress-1024` takes under 2 million
  instructions.
- **`pay-default`'s worst frame lays out a prompt.** Its frame 30 takes a view
  that opens one, and of its 357,443 instructions, counted as each function's
  own, taffy's are 7,000 (2.0%) and parley's 56,508 (15.8%), or 129,618 (36.3%)
  with the font crates parley calls (harfrust, read-fonts and fontique). Most of
  the rest is the script compile and its allocations.
- **The 2 million target** is the one the doc gives `stress-255` and the
  'Threads' section gives every frame. The recorded boards have no limit of
  their own beyond their measured counts.
- **The limits have risen since.** By 2026-11-16, after milestone 2's verbs and
  milestone 3's step banner and log, `frames.<board>.worst_frame_instructions`
  in `data/beryl3d/gates.json` held `verbs-zones` at 4,816,173, `stress-255` at
  3,267,119, `stress-1024` at 9,283,180, `stress-verbs` at 2,638,123 and
  `pay-default` at 1,257,448. The 2 million target is met by `pay-default`,
  `stress-hand` and `stress-stack` only. `verbs-zones`' worst frame is now frame
  980 of 1,183, whose `handle` takes a view with a script of 34 steps and places
  738 objects while compiling it. The worst frames of `verbs-zones`,
  `stress-255`, `stress-1024` and `stress-verbs` are each a script compiled
  whole in the `handle` call that receives it.
- **Compiling a script over frames halved the stress boards' worst frame.** With
  `Client::prepare` compiling a script that arrives with nothing playing a few
  steps ahead of each frame (see [Threads](#threads)), the frame that receives
  the stress boards' 40-step script compiles 14 of them, and their worst frames
  fell to 1,966,154 instructions for `stress-255` (now its frame 0, which places
  the board) and 6,424,479 for `stress-1024`. `stress-verbs`' script, one beat
  of 106 steps that all begin together, still compiles whole in its first frame.
  `verbs-zones`' worst frame, 4,838,458, takes eleven updates in one `handle`; a
  script that arrives behind another is still compiled whole.
- **Compiling a waiting script as it starts cut `verbs-zones`' worst frame from
  34 steps to 12.** Its frame 980 took eleven updates in one `handle` and
  compiled every one of their 34 steps. Each script that arrives behind another
  now waits uncompiled until the frame in which it starts. Each such script
  places its view once as it arrives, for the frames to read where its cards
  will rest, so the board places 2,260 objects in all instead of 1,827. A script
  laid out again as it starts (any `hold` or `alone` timing) is still compiled
  whole as it starts.

**Time per frame, natively**, by beryl_render's `frame_times` example in a
release build, paced at 60 frames a second, five runs. Each figure is the median
of the five runs, with the worst of the five in brackets. `client` is the
`handle` calls and `Client::frame`; `encode` is `Renderer::render`, which writes
the records, encodes the frame and submits it; `done` is the wall-clock time
from the submission to the GPU finishing, because GPU timestamps do not work on
this Mac. The target is the gates' 2880 by 1800 pixels, with no card faces
uploaded.

| Board                   | Load  | `client` mean | `client` worst | `encode` mean | `encode` worst | `done` mean    |
| ----------------------- | ----- | ------------- | -------------- | ------------- | -------------- | -------------- |
| `stress-255`            | 26-27 | 56 µs (91)    | 784 µs (4,878) | 342 µs (460)  | 1.98 ms (5.59) | 2.37 ms (3.34) |
| `stress-1024`           | 26-27 | 195 µs (642)  | 3.73 ms (13.7) | 419 µs (535)  | 4.29 ms (7.35) | 2.21 ms (3.64) |
| `verbs-zones`           | 23-29 | 27 µs (89)    | 1.74 ms (47.9) | 334 µs (417)  | 7.11 ms (14.3) | 2.20 ms (2.66) |
| `pay-default`           | 20    | 14 µs (19)    | 166 µs (356)   | 223 µs (244)  | 456 µs (2,494) | 2.36 ms (2.50) |
| `stress-255`, 1920x1080 | 19    | 35 µs (39)    | 472 µs (784)   | 181 µs (223)  | 441 µs (1,429) | 1.39 ms (2.09) |

The runs shared the machine with Callgrind and a build in `beryl3d-bench`, which
the load average of the Mac does not show, so the worst times are high. They
were taken before commit 417c9da07 halved the instructions of the stress boards'
frame 30, so that frame's `client` times are now lower than shown.

- **The worst `client` frame is the same frame 30**, the script compile: 3.7 ms
  on this machine for `stress-1024`, a fifth of a frame on a CPU many times
  faster than the floor's. by-q44s.22 continues the fix.
- **A paced frame costs more than a back-to-back one.** `frame_abba`, which
  plays frames back to back with each board's setup spread over its frames,
  measured 22 µs a frame for `stress-255` and 96 µs for `stress-1024`, against
  56 and 195 µs paced: the processor lowers its clocks while the thread sleeps
  between frames. Frame times are therefore taken paced, and the gates count
  instructions, which clocks do not change.
- **The worst times are single interruptions** by the machine's load: the
  medians of the means are steady, and one run's worst in a board is often many
  times the others'.

**The draw list's load**, from `just b3d gate`, the most in any frame, and the
fragments shaded from `just b3d gate --gpu`, the most in any of a board's 8
ranked frames, in screens of 2880 by 1800 (Mesa's llvmpipe Vulkan driver in
`beryl3d-bench`, the same on two runs).

| Board          | Draw calls | Instances | Upload bytes | Blended area | Fragments |
| -------------- | ---------: | --------: | -----------: | -----------: | --------: |
| `stress-255`   |          1 |       256 |       20,480 |            0 |     1.000 |
| `stress-1024`  |          1 |     1,025 |       82,000 |            0 |     1.000 |
| `verbs-zones`  |          2 |        48 |        3,840 |       0.0414 |     1.000 |
| `pay-default`  |          2 |        27 |        2,160 |       0.0287 |     1.029 |
| Baseline limit |         16 |           |      327,680 |         0.75 |      1.75 |

- **Every board is within the Baseline tier's limits.** The stress boards draw
  no shadows or edges yet, so they have no blended area, and the recorded
  boards' blended area is the flat layer's prompt bar and text.
- **No board has card faces, marks, glows or shadows yet**, so these are far
  below what the tier will carry once milestone 2's verbs and effects land.
- **Every gated board draws within the floor's limits.** beryl_render's
  `downlevel` test draws each board's 8 ranked frames on a device made with
  wgpu's downlevel limits, in a 1920 by 1080 target, the largest those limits
  allow, and a validation error fails it. It runs in the lane's tests.

**The app**, `beryl3d`:

| Measure                              | Result                               | Load  |
| ------------------------------------ | ------------------------------------ | ----- |
| Release executable, stripped         | 38.2 MB                              |       |
| Memory once started, a game resumed  | 37 MB, of which 24 MB is the GPU's   | 57    |
| Peak memory                          | 360 MB, of which 344 MB is the GPU's | 33-71 |
| Memory while frames are drawn        | 257 MB, of which 199 MB is the GPU's | 24-28 |
| Launch to the first frame, no window | 26 ms median (343, 43, 25, 26, 23)   | 33-71 |

- **Most of the executable is the game.** Linking `beryl_session` brings the
  engine, the cards, the card store and the search.
- **The GPU memory is Apple's Metal driver's, not Beryl's.** Beryl's own buffers
  and textures take under 10 MB. The driver allocates a fixed pool the first
  time a process encodes each kind of command encoder, and frees it about a
  second after the last encoder of that kind; `footprint` counts the pools as
  "Owned physical footprint (unmapped) (graphics)". A bare Swift program shows
  it: a first render pass costs about 185 MB whatever its size (16 by 16 or 1920
  by 1080), and a first blit encoder about 112 MB more. The 344 MB peak is
  mostly these two pools. The idle run above draws one frame, so the driver
  frees both pools a second later, leaving 24 MB.
- **Frames encode no blit, so only the render pass's pool stays while frames are
  drawn.** `Queue::write_buffer` copies through a blit, and the renderer used it
  every frame for the uniforms and the instance records, which kept the blit
  pool for as long as frames were drawn. On Metal the renderer now writes them
  into a ring of three buffers the CPU maps directly
  (`MAPPABLE_PRIMARY_BUFFERS`, `beryl_render`'s `slots` module), and a blit
  happens only when a glyph page or card face is uploaded. Elsewhere it still
  uses `Queue::write_buffer`. Measured with `footprint --sample 0.25` on the
  release build replaying `verbs-zones` with no window on the M5 Max, five runs
  of each, the footprint while frames were drawn fell from 400 MB (344 MB the
  GPU's) to 257 MB (199 MB the GPU's) in every run, with no late frames in
  either. The first second still shows the blit pool, from the startup uploads.
  A phone's driver may keep pools of other sizes; this was measured on the Mac
  only.
- **The game in these runs waited for the player.** The app resumes the newest
  unfinished game, and nothing answers for the player in a run with no window.
  The search threads were measured against the frame thread in whole games that
  `--agent` plays, under "The search threads against the frame thread" below.
- **The first frame is the windowless host's**, read from the app's own
  `beryl3d: first frame shown` line. The first run paid for loading the
  executable from disk.

**What earlier beads measured**, given in full in their own sections:

| Measure                                            | Result                                                      | Budget       |
| -------------------------------------------------- | ----------------------------------------------------------- | ------------ |
| `just b3d tree` after a one-line edit              | 2.5 s median on 2026-11-15; 6.0 s on 2026-11-16, load 16-18 | 3 s: not met |
| `just b3d png` after a one-line edit               | 6.4 s and 12.4 s on 2026-11-15; 7.3 s on 2026-11-16         | 6 s: not met |
| 127 board faces cold, ASTC                         | 1.9 s, load 55-57                                           |              |
| 127 board faces from the compressed cache          | 8 ms                                                        |              |
| 150 board faces, compressed                        | 18.5 MB                                                     | 18 MB, about |
| Frame delivery, background window, 16 busy threads | 0 late frames of 1,302                                      |              |
| Frame delivery, simple fullscreen, settled         | 1 late frame of 1,287                                       |              |

The edit-to-output times are under [Edit-to-output time](#edit-to-output-time),
and the card faces and frame delivery below.

What carries over from the prototype, which measured the design on 2026-11-14:

- **The opaque pass with a depth buffer is cheaper.** The same frame drawn all
  blended, back to front, took 1.2 to 1.6 times as long to finish on this GPU,
  and the fragment counts above show the depth test rejecting every hidden pixel
  of the stress boards.
- **GPU timestamps do not work on this Mac.** `wgpu` 30 offers
  `Features::TIMESTAMP_QUERY` on Metal, but a render pass's end timestamp came
  back earlier than its start, so frame times here use the wall clock.
- **Paced frames cost more**, as measured above, and the window must be driven
  by a display link, which the desktop host now is (decision C18).

Frame delivery was then measured by Beryl 3D itself, on 2026-11-15, by an agent
with no person at the window (decision C16): the `hover-hand` drive script over
`verbs-zones` for 20 seconds, on the same Mac's 120 Hz display, at a machine
load average of 51 to 56 from other sessions' builds and tests. `--load 16`
added 16 busy threads in the default class. Waits are for a drawable, in
milliseconds.

| Window     | Busy threads | Frames | Late (held to 60 Hz / missed 60 Hz) | Worst gap | Wait: median, 95th, worst |
| ---------- | -----------: | -----: | ----------------------------------- | --------: | ------------------------- |
| background |            0 |  1,246 | 8 (3 / 5)                           |  154.3 ms | 0.1, 0.6, 153.9           |
| background |           16 |  1,302 | 0                                   |    8.6 ms | 0.1, 0.1, 1.8             |
| fullscreen |           16 |  1,126 | 94 (93 / 1)                         |   46.4 ms | 0.1, 8.0, 45.9            |

- **The late frames are the window server's, not the app's.** In every run the
  client took under 0.1 ms and the encode under 1 ms in each late frame. The
  background window's worst gap was one wait of 154 ms for a drawable, in the
  run with no added load, so a single run does not show that the added threads
  matter.
- **Fullscreen was not held to 60 Hz: its display link skipped.** The log
  counted 93 of its 94 late frames as held to 60 Hz because they came within one
  and a half sixtieths of a second, but 75 of them came exactly 21.3 ms after
  the frame before, not 16.7 ms. The cause was found the same day, below.

Why fullscreen was late was then measured, on 2026-11-15, the same way at a load
average of 50 to 62, with the rate rule of decision C18 in place.

| Window                                 | Frames | Late (held / missed 60 Hz) | Rate asked for at the end | Worst gap |
| -------------------------------------- | -----: | -------------------------- | ------------------------- | --------: |
| fullscreen Space, first run            |    578 | 22 (5 / 17)                | 30 Hz                     |   63.0 ms |
| fullscreen Space, second run           |    484 | 22 (2 / 20)                | 30 Hz                     |   63.0 ms |
| simple fullscreen, before settling     |  1,266 | 3 (2 / 1)                  | 120 Hz                    |   25.2 ms |
| simple fullscreen, before settling     |    723 | 4 (3 / 1)                  | 60 Hz                     |   25.7 ms |
| simple fullscreen, settling, first run |  1,282 | 3 (2 / 1)                  | 120 Hz                    |   30.1 ms |
| simple fullscreen, settling, second    |  1,287 | 1 (1 / 0)                  | 120 Hz                    |   21.3 ms |
| foreground window                      |  1,346 | 2 (2 / 0)                  | 120 Hz                    |   20.3 ms |
| background window                      |  1,301 | 0                          | 120 Hz                    |    8.5 ms |

- **A fullscreen Space skips display refreshes.** winit's
  `Fullscreen::Borderless` makes a fullscreen Space on macOS, where the system
  shows the app's surface directly on the ProMotion display. There the display
  link's own timestamps, read in each tick, skipped about twice a second: 20.8
  ms between ticks at 120 Hz, 29.2 ms at 60 Hz and 45.8 ms at 40 Hz, each a
  whole number of 4.17 ms steps. The app's work stayed under 0.4 ms a frame. The
  rate rule stepped down from 120 to 30 Hz in each run, and frames still came
  late at 30 Hz, so no rate helps. Making the link's range exactly the rate
  asked for, rather than 75% of it to the rate, did not change this.
- **A composed window over the whole display holds 120 Hz.** winit's "simple"
  fullscreen grows an ordinary window over the display and hides the menu bar
  and the Dock, and the window server composes it as it does any window. It
  showed as few late frames as the foreground and background windows, so the
  desktop app's fullscreen on macOS is now this (decision C15). One run before
  settling stepped down to 60 Hz 1.3 seconds in, after a cluster of late frames
  as the window opened, and stayed there for want of 600 frames in a row with
  room, which is why the rule now lets 120 frames settle first.
- **The step down works with no window.** The windowless host at 60 Hz with
  `--slow-gpu 24ms` stepped down to 30 Hz once its frames settled and had no
  late frame after.

**The search threads against the frame thread** (decision C9, the budget in
[Threads](#threads)), measured 2026-11-15 by Beryl 3D itself on the same Mac,
from a release build with `tools`: `beryl3d --window none --agent expert`. Each
run is one whole game, the WU Control mirror from seed 20261114 between the
`mcts-hidden` opponent, at the web app's budget, and the `expert` agent
answering for the viewer, ended when the game is over. Every run played the same
game: 124 searched decisions, 408,036 rollouts and 157.5 million engine steps.
`--search-class` put the session thread and the search threads in utility, as
Beryl 3D then did, or in default, the class the web app's session runs its
search in and the one Beryl 3D uses now. `--load 16:default` added 16 busy
threads. Five rounds ran each of the four configurations once, in a rotated
order, at machine load averages of 41 to 77 from other sessions' builds and
tests. Rates are the opponent's rollouts per second of decision time, from its
own search counters; the windowless host ticked at 60 Hz. Each figure is the
median of five runs, with the worst beside it.

| Search class          | Busy threads | Rollouts/s      | Frames while searching | Late among them | Worst client | Worst encode | Worst gap   |
| --------------------- | -----------: | --------------- | ---------------------: | --------------- | ------------ | ------------ | ----------- |
| default (the web app) |            0 | 17,437 (12,180) |                    772 | 7 (12)          | 10.4 (43.5)  | 6.1 (13.9)   | 44.4 (55.2) |
| utility               |            0 | 13,879 (10,910) |                    800 | 6 (11)          | 12.3 (53.7)  | 9.3 (20.6)   | 46.6 (54.1) |
| default               |           16 | 16,587 (13,467) |                    807 | 4 (6)           | 4.3 (77.2)   | 7.4 (19.0)   | 36.0 (82.9) |
| utility               |           16 | 14,367 (11,028) |                    852 | 6 (10)          | 12.3 (31.0)  | 10.4 (18.1)  | 43.9 (49.2) |

Times are in milliseconds: the worst `Client::frame` and encode of a frame drawn
while the AI searched, and the longest gap between two frames in the run.

- **The throughput budget is not met.** Paired by round, utility's rate was a
  median 0.88 of default's (0.56 to 1.14 over the ten pairs), and lower in 8 of
  the 10. The machine's load moved the rates more than the class did, from
  10,910 to 27,948 rollouts a second across runs, so the pairing is what shows
  the class's cost. On a busy machine, threads in the utility class yield to
  every default-class process, which is what the other sessions run in. The
  second set of runs below did not repeat this.
- **Utility bought drawing nothing measurable.** Late frames while the AI
  searched were as many with utility as with default, with and without the busy
  threads. Every run had some, so the "no late frame while the AI thinks" half
  of the budget is not met either, at these loads, whatever the class.
- **The late frames are the frame thread waiting, not working.** Of the 117 late
  frames drawn while the AI searched, the client took under 0.3 ms in all but
  those of three runs, and the encode under 1 ms in 98; the gaps were 25 to 83
  ms. The worst client times of 30 to 77 ms were most likely frames the system
  interrupted, since that work takes under a millisecond in most frames. The
  machine ran three to four times as many busy threads as its 18 cores, and the
  frame thread's user-interactive class did not keep it from waiting. The rate
  rule stepped the windowless host down to 30 Hz in 14 of the 20 runs.
- **A live game first crashed the client.** Before these runs could finish, a
  script naming an ability by its number panicked the client core, which builds
  its own table from each view and so knows no ability's number. It now places
  such a step whole; by-q44s.1.59 keeps the table across scripts so that the
  step animates.

**The search threads' class, measured again** the same day with the same driver,
after the session thread moved to the default class and the asset workers to
user-initiated: five more rounds of the four configurations, from a release
build with `tools`, at machine load averages of 23 to 99. Every run again played
the same game of 124 searched decisions and 408,036 rollouts, and the rate rule
stepped the windowless host down to 30 Hz in all 20.

| Search class       | Busy threads | Rollouts/s      | Frames while searching | Late among them | Worst client | Worst encode | Worst gap   |
| ------------------ | -----------: | --------------- | ---------------------: | --------------- | ------------ | ------------ | ----------- |
| default (Beryl 3D) |            0 | 12,555 (11,088) |                    831 | 10 (14)         | 9.9 (33.1)   | 7.9 (11.8)   | 51.9 (77.7) |
| utility            |            0 | 14,838 (12,516) |                    739 | 7 (9)           | 17.9 (31.0)  | 6.5 (15.4)   | 48.5 (50.2) |
| default (Beryl 3D) |           16 | 10,278 (8,413)  |                    984 | 7 (12)          | 11.0 (21.7)  | 9.0 (28.5)   | 47.8 (49.0) |
| utility            |           16 | 11,494 (9,253)  |                    847 | 8 (12)          | 18.1 (31.4)  | 5.4 (7.5)    | 49.2 (53.9) |

As in the first table, each figure is the median of five runs with the worst
beside it, and times are in milliseconds.

- **This time utility searched faster.** Paired by round, default's rate was a
  median 0.89 of utility's (0.68 to 1.25 over the ten pairs), and lower in 8 of
  the 10: the reverse of the first set. Over both sets' 20 pairs, default was
  faster in 10 and the median ratio was 0.99. Rates across runs went from 8,413
  to 21,252 rollouts a second, so on this machine the class's effect on the
  search is smaller than what the other sessions' load does to it, and neither
  set can resolve the 2% budget. The default class meets it by being the web
  app's class.
- **Late frames are unchanged.** Every run had some while the AI searched, 162
  in all, with either class. In 18 of the 20 runs the worst late frame's client
  work was under 1 ms and in 17 its encode was under 2 ms, so as before these
  are the frame thread waiting for the machine, not doing work.

**Card face preparation**, measured 2026-11-15 on the same machine at a load
average of 55 to 57, by `beryl_assets` itself in a release build. The faces are
real Scryfall scans at 672 by 936. The 127 faces are those of the implemented
cards, each given one of 49 real scans, prepared at board size on 4 workers. The
times are inflated by the load, perhaps two to three times.

| Measure                                  | ASTC 4x4 | ETC2 RGBA | BC7     | RGBA   |
| ---------------------------------------- | -------- | --------- | ------- | ------ |
| Compress one board face, with mipmaps    | 77 ms    | 3 ms      | 19 ms   | 4 ms   |
| Compress one full-size face              | 388 ms   | 18 ms     | 44 ms   | 13 ms  |
| 127 faces cold (decode, compress, cache) | 1.9 s    | 0.95 s    | 1.2 s   |        |
| 127 faces from the compressed cache      | 8 ms     | 6 ms      | 6 ms    |        |
| One board face with mipmaps              | 123 KB   | 123 KB    | 123 KB  | 491 KB |
| 150 board faces                          | 18.5 MB  | 18.5 MB   | 18.5 MB | 74 MB  |

- **Decoding and scaling a scan** to board size took 14 to 24 ms a face.
- **ASTC uses astcenc's fastest preset.** Its quality-35 preset took 190 ms a
  board face and its `fast` preset 110 ms. Faces are compressed once per
  machine, so the cold time is paid only for a deck seen for the first time.
- **Full-size faces are made only on request**, not during the cold start, so
  the cold start does not pay ASTC's 388 ms a face.

## Checks and isolation

Beryl 3D is a separate program, and until the switch it is not the app. Its
checks are therefore its own (decision C17): they run for a change to Beryl 3D
and for no other change, and no other change builds Beryl 3D's code.

What keeps the two apart:

- **A workspace of its own.** `rust/beryl3d` is a Cargo workspace with its own
  manifest, lock file and build directory. The main workspace's Clippy run and
  its tests never see it. Its dependencies (`wgpu`, `winit`, `taffy`, `parley`
  and the rest) never enter the main lock file, or the build directory that
  every worktree copies.
- **Paths of its own.** Beryl 3D's files are under `rust/beryl3d/`,
  `data/beryl3d/` and `tools/src/beryl_tools/beryl3d/`. A change under the first
  two selects one lane of the checks, `beryl3d`, and no other lane except the
  two that check formatting and source rules (`repo` and `names`). A test in the
  tools holds both directions: a list of Beryl 3D paths selects no other lane,
  and a list with none of them never selects `beryl3d`.
- **One change in the main workspace.** `beryl_protocol`'s `neutral.rs` has
  `ObjectView::neutral` and `Script::empty`. Beryl 3D builds every test view as
  `..ObjectView::neutral(..)` or `..Script::empty()`, in `beryl_scene::testing`,
  so a field added to those types with a default does not break its build. They
  change no wire type.
- **Code both shells use.** What the Tauri shell and Beryl 3D both need and that
  names no Tauri type moves out of the Tauri shell into a crate of the main
  workspace: the image download queue and disk cache in `beryl_art`, and
  profiles, data folders, bug report storage and resuming in `beryl_shell`.
- **Tools of its own.** Everything behind `just b3d` is one Python package,
  which the main tools import in one place. Adding a check to Beryl 3D edits
  that package and nothing else, so no other lane's record of its own tools
  changes and slow CI runs no other lane again. The package is the one exception
  to the paths above: it is Python under `tools/`, so a change to it also runs
  the main `python` lane (Black, Pyre and the tools' tests), and its own tests,
  `tools/tests/test_beryl3d*.py`, run in that lane for every change to `tools/`.
  They take about 2 seconds, so they stay there rather than in a Python lane of
  Beryl 3D's own.
- **Data of its own.** Limits, thresholds, spatial values and allowlists are in
  `data/beryl3d/`. Beryl 3D adds nothing to `data/ui-conventions.json`, which
  the web interface's checks watch, except an entry under "Not seen in Arena"
  when it builds something Arena does not show.
- **A virtual machine and a name of its own.** Counts are taken in
  `beryl3d-bench`, and every check it logs is named `b3d:` and what it checks.

What runs, and when:

| When                                                                              | What                                                                                                                                                                                                                                    | If it fails                                                             |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Before a push that changes a Beryl 3D path: the `beryl3d` lanes of the fast check | Formatting, Clippy and the tests of the workspace. The source rules (`scene-purity`). Every interface test at its check times. The work counters and the allocation gate. Timeline parity.                                              | The push is refused, as for any lane.                                   |
| After a push, on staging: `just b3d sweep`                                        | The same, then every interface test on every tick, the instruction and fragment counts in the virtual machine, every gated fixture drawn on the floor's feature set, the app's launch test, and a build for each phone and for Windows. | A task is filed under Beryl 3D's epic. The `master` branch is not held. |

- **The sweep is not slow CI.** It is modelled on the browser sweep
  (`just sweep`). It runs at low priority on the staging tip when a Beryl 3D
  path, or a path of the main workspace that Beryl 3D depends on, has changed
  since its last run. Its cost is outside slow CI's budget of CPU time.
- **Quick steps report first.** The sweep runs the lane's steps and the launch
  test first, saves the build directory that new worktrees clone, and files
  their failures. Only then does it run the counts in the virtual machine, which
  can take hours under a lock of their own, so the next sweep's quick steps do
  not wait for them.
- **A step that the fast check cuts off at its time limit** is left to the
  sweep, not to slow CI.

What isolation costs, and who pays:

- **A change elsewhere can break Beryl 3D unseen.** Beryl 3D links the session,
  the fixture recorder, the image cache and the shared shell crate. A change to
  one of them is checked without Beryl 3D, so it can land and leave Beryl 3D not
  compiling, or playing a script wrongly.
- **The sweep finds it, and Beryl 3D's own work fixes it.** The sweep's task
  names the commit that broke it. Whoever is working on Beryl 3D takes that task
  first, and a session whose own check fails on such a break fixes the break in
  its own change.
- **The two timelines can drift.** A change to how the web interface lays a beat
  out in time is not held to Beryl 3D's. The sweep's timeline parity run lists
  the recordings that now differ, and Beryl 3D follows.

The exceptions. Each is a change to the rest of the project, made once and
checked by the rest of the project's checks:

- **Moving step timing into the session** (see
  [Timing a script](#timing-a-script)): three changes to `beryl_session` and the
  web stage.
- **Code both shells use**, moved out of the Tauri shell into crates of the main
  workspace that depend on nothing of Beryl 3D's: `beryl_art` (the image
  download queue and cache), and later the crate for profiles, bug report
  storage and resuming a game.
- **The fixture recorder made a library**, in `beryl_cli`.
- **Recording a fixture from the seat across the table**: `fixture::record_from`
  in `beryl_cli`, and `Sessions::with_across` in `beryl_session`, which sends
  the other seat's view of each update beside the human's.
- **One import of Beryl 3D's tools package**, the `just b3d` recipe, and the
  rule that sorts paths into lanes.
- **Design tokens**: the data file that generates the web interface's CSS
  variables as well as Beryl 3D's constants.

## Sound

Beryl 3D plays the same sounds as the web interface, from the same list:
one-shot sounds tied to script steps and looping ambience.

- **The files and the list are the web interface's**: the files under
  `ui/public/audio` (music as AAC in M4A files under `music/`, effects as WAV
  files under `sfx/`), listed in `ui/src/audio/manifest.json`, with
  `ui/src/audio/steps.ts` saying which step plays which sound. Beryl 3D reads
  the same manifest and ports the rule in `steps.ts`.
- **Playback** uses `rodio` 0.23.0 over `cpal`, with rodio's default features
  off and three on, by the names in rodio's manifest: `wav` (Symphonia's WAV
  reader and PCM decoder, for the effects), `mp4` (Symphonia's MP4 container and
  AAC decoder, for the music) and `playback` (the output device, through
  `cpal`). Android builds `rodio` without `playback`, as described below. The
  audio thread (`beryl_3d`'s `src/audio`) owns the output device, whose own
  callback thread mixes; the frame thread only sends it messages: each sound of
  each frame's `Frame::sounds`, `Request::Music` and `Request::Audio` (the
  levels and mute).
- **Effects are decoded at startup** into memory, by a worker, so playing one is
  a message to the audio thread and never a file read. As in `sound.ts`, each
  copy plays a variant at random, varied up to 3% in pitch and 1 dB in level, at
  the cue's `gain` (4 dB less for the opponent's), and a cue already playing at
  its `voices` copies (4 by default) drops the request.
- **Music is decoded when it is asked for**, on a worker, since every track
  decoded whole would take about half a gigabyte. A loop is cut at its exact
  length in `ui/src/audio/loops.json`, plays twice, and crossfades over 1.5 s
  into the next variant in its rotation; the win and loss stings play once.
- **Settings** are saved by the desktop app as `settings.json` in its data
  folder for each `Request::SaveSettings`, on a thread of their own, and handed
  to the client as `Input::Settings` at startup. A replayed recording or a game
  `--agent` plays starts from the defaults and saves nothing.
- **A run with no window is silent**, so agents' and checks' runs make no noise,
  and an app with no audio device runs silent and says so once on its standard
  error.
- **A sound is triggered by the client core**, which lists it in `Frame::sounds`
  in the first frame whose time is at or after the time its step asks for it
  (`beryl_scene`'s `sound.rs`, a port of `CUES` in `steps.ts`; the list of
  names, `sound::NAMES`, is the manifest's `sounds` keys in file order, and
  `SoundId(n)` is the n-th). Each step has its sounds as the script is compiled,
  so a sound's time is on the script's clock like the step's own. Tests read
  those requests like any other output, so the timing of a sound against its
  animation is checkable.
- **The port is held to `steps.ts` by timeline parity**: both clients print each
  step's sounds in their timelines, and a step whose sounds differ counts as a
  differing step (see [Timeline parity](#timeline-parity)). When `steps.ts`
  changes, the sweep names the recordings whose sounds now differ.
- **On Android the Kotlin host plays the sound.** `cpal`'s Android backend uses
  AAudio, which first shipped in API level 26, and it links `libaaudio.so`
  unconditionally. A library that links it fails to load at all on API 24 and
  25, so the app would not start there; it would not merely be silent. So on
  Android, `rodio` is built without `playback`: the client core mixes with
  `rodio`'s mixer, and the Kotlin host pulls the mixed samples through a C
  function into an `AudioTrack`, which every supported API level has.

## Bug reports

The bug report keeps its place and its contents, and gains two things a WebView
could not give.

- **The picture** is the renderer's own frame, read back from the GPU. It needs
  no screen-recording permission and no platform snapshot code.
- **The element tree** at the moment of the report is saved as text beside the
  picture, so whoever fixes the bug can see every element's position without
  reproducing it.

The frame log and the session's replay file are attached as they are today.

How it works (`beryl_scene`'s `screens::bug_report` and `client/bug.rs`, and
`beryl_3d`'s `bugs.rs`):

- Cmd+B (Cmd+Shift+B on the manual table) freezes the table's clock and sends
  `Request::FreezeFrame`. For two frames nothing of the report is drawn; the
  host reads back the second, and keeps the `just b3d tree` printout of that
  frame beside it.
- The sheet then goes through the web's stages: drag a region (a click, Space,
  Return or the whole-window button takes the whole window), describe the bug
  with the "AI play" box, submit, "Saved", closed. Escape closes it at any
  point.
- Under the sheet the client keeps drawing the frozen table itself, not the
  read-back picture, so the renderer needs no window-sized texture. The describe
  stage has no thumbnail of the region for the same reason.
- Once the report closes, the table's clock runs behind the frame's by the time
  it was frozen, so a script that was playing goes on from where it stopped.
- `Request::SaveBug` writes the report with
  `beryl_session::bugs::write_attached`: on the session thread in a live game,
  and on a thread of its own for a replayed recording, which has no replay or
  view of a game. Beside the web report's files it holds `tree.txt` and
  `frames.jsonl`, and its `ui.json` names Beryl 3D as the client.
- `Request::ExportBugs` writes `beryl-bug-reports.zip` to the Downloads folder
  and prints its path, since winit has no save dialog.
- A drive script's or `just b3d live do`'s `key` action takes modifiers, as in
  `key Cmd+B`.
- A report saved while a recording replays also records, in its `ui.json` log,
  the `recording`, its `seat`, `at` (the milliseconds of the recording at the
  freeze, on the test driver's clock) and `ended` (whether its last script had
  ended by then), so `just b3d tree <recording> --at <at>ms` (or `--at end`)
  shows the frozen frame again with no game.
- `beryl3d --replay <replay.json>` (`just b3d app --replay`) resumes the game of
  a replay file at its latest decision, as launching resumes the newest game
  (`Sessions::resume_replay`), and saves nothing: the file is the report's, and
  a game saved among the replays would be the one the next launch resumes.
- `just b3d bug <report>` prints a report's words, commit, element tree and
  frame log summary, and the commands above that reproduce it.

## Migration and compatibility

Both clients are alive from the first commit of Beryl 3D until the project's
owner switches (decision C1). This section specifies what they share, how
coverage is known and what the switch removes.

### What the two clients share

Shared truth is kept in data and in the session, so that most fixes are one
change there and a small change in each client.

- **The session and its protocol**, including the timing of every step.
- **`data/ui-conventions.json`**: verbs, marks, timing values and the numbered
  exceptions.
- **Design tokens**: one data file generating CSS variables for the web
  interface and constants for Beryl 3D.
- **Fixtures, card specs and the list of sounds.**
- **The image download queue and disk cache**, moved out of the Tauri shell into
  `beryl_art`, a crate of the main workspace.
- **Bug report storage, profiles and resuming the newest game**, moved out of
  the Tauri shell into `beryl_shell`, a crate of the main workspace that both
  shells use. Beryl 3D keeps its replays and games ledger in the web app's data
  folder (`~/Library/Application Support/ca.thurn.beryl`, or
  `%APPDATA%\ca.thurn.beryl` on Windows, under `profiles/<name>` for a named
  `BERYL_PROFILE`), so a game saved by either app resumes in the other. Settings
  are not shared: the web interface keeps its settings in the browser's storage,
  and Beryl 3D in a file of its own in the same data folder.

What they do not share is placement. The web interface draws the table on a 1600
by 900 surface and imitates perspective by shrinking and tilting each card.
Beryl 3D's table is a real scene. Each is fitted to Arena separately.

### Matching the web interface's look

The owner wants Beryl 3D to look nearly pixel for pixel like the web interface,
which he is happy with. `just b3d match` measures every difference between the
two clients at the same moment of a recording, and `data/beryl3d/web-match.json`
holds the counts for every interface test target, which may only go down. The
reference is the web interface as WebKit draws it, at scale 2.

- **Snapshot.** `just b3d match snapshot <target> --at <moment>` writes both
  clients' pictures and element records of the moment to
  `$BERYL_CACHE/b3d/match/<target>/` (`web.png`, `web.json`, `b3d.png`,
  `b3d.json`, `report.txt`). A record holds the element's role, name, object (a
  card's slot in the view), box in points, paint order, parent, the style of a
  fixed list of CSS properties and each line of text with its baseline. Beryl
  3D's records come from `b3d tree --records`, which builds the frame with
  `Frame::looking` set so that it records how each flat-layer element was drawn
  (its panel, text color and type) and each line's words; what `Style` cannot
  say yet (gradients, shadows, filters, masks, transforms) is written as `none`.
  The web side is read by a script that Playwright evaluates in the page, so
  nothing under `ui/` changes, and its roles come from the ARIA role or from
  `data/beryl3d/match/roles.json`. The page runs on Playwright's paused clock: a
  mid-animation moment runs it as long after the view's arrival as Beryl 3D's
  moment lies, and a moment at rest runs it until the stage is idle. The page's
  first frames while it loads make the web side's start of the animation
  approximate, and CSS animations finish rather than stopping at the moment. The
  report warns when a bundled font's `name` table differs from the web's WOFF2
  file of the same face. Neither side draws card scans unless `--art` is given,
  and then both read the app's scan cache.
- **Boxes and styles.** Elements are paired by locator, then by role, name (or
  card) and parent, then by role and the nearest box within 24 points; the pairs
  and what is left over go to the snapshot's `joined.json`. "The same parent"
  means that the nearest ancestors already paired are each other's pair, since
  the web nests many more unnamed elements. `just b3d match boxes` lists, worst
  first, box differences over half a point, text broken into a different number
  of lines (an element's lines include those of its unpaired descendants, as the
  web puts a button's label on a span inside it), the unpaired elements on each
  side, and text overlaps only one side has; `--all` runs every test target.
  Differing style values and web style values `Style` cannot express are each
  listed worst first.
- **Effects.** A conformance suite draws single elements with the box shadows,
  gradients, drop shadows, filters and masks the web interface uses, in WebKit
  and in Beryl 3D's renderer, and holds each within a small tolerance.
- **Pixels.** What is left after boxes and styles agree is compared as pixels,
  with glyph pixels masked, because glyph antialiasing differs between WebKit
  and parley with swash. `just b3d match pixels <target> --at <moment>` takes
  the snapshot, pairs it and runs `b3d-render pixels` on its directory. A pixel
  differs when a color channel differs by more than 8 levels. Each text line of
  both sides' records, shrunk to its ink in that side's picture, is left out.
  Differing pixels that touch form regions, and regions under 4 pixels are
  dropped as antialiasing specks. Each differing pixel is given to the topmost
  element that paints at its point on each side (a fill, image, shadow, border,
  text, card or icon): on the web by paint order and box, in Beryl 3D by the hit
  test (`agent::hit`), or by paint order and box where the element it reaches
  paints nothing. Elements, and pairs of elements counted as one, are printed
  worst first by differing pixels, with a hint when one side has no such element
  or no element paints over most of the pixels (where only the page's background
  or the table's cloth shows, which are not elements). The worst ten get a crop
  in `crops/`: the web, Beryl 3D and their difference amplified four times, side
  by side, magnified up to four times, with glyph pixels in blue; the counts go
  to `pixels.json`. `--only <locator>` on `snapshot` and `pixels` draws one
  Beryl 3D element subtree and its web pair alone, on black, and gives pixels
  only to the elements inside it. On the web an injected style hides everything
  else with `visibility: hidden`, which keeps the layout. In Beryl 3D a looking
  frame records which element each draw record belongs to (`Frame::owners`, the
  element added last before the record was pushed) and `Frame::keep_only` keeps
  the subtree's records. A record the table writes for a card after a later
  element was added would be kept with that element instead; the flat layer
  writes each node's records right after adding it, so its subtrees are exact.
- **Motion.** Element boxes, opacity and transforms are compared frame by frame
  over an animation.
- **Drift.** A web change never runs Beryl 3D's checks (decision C17), so the
  Beryl 3D sweep scores every target and files a bead for each one that drifted.

Styles are not generated from the web interface's CSS. Each Beryl 3D element
keeps its own `Style`, and the tools hold it equal to the web's computed style.
Text is compared as data: string, font, size, weight, color, baseline and box.
Whether the 3D table's card placement should follow the web interface's, instead
of each client being fitted to Arena separately (below), is decided from the
measured difference once the tools exist.

### Timeline parity

The two clients place things differently on purpose, so their pictures cannot be
compared. What they must agree on is what plays and when. The session sends both
the same timing for every step (see [Timing a script](#timing-a-script)), which
leaves two things a client could still get wrong: laying a beat out in time, and
the steps it makes for itself while a prompt is open. The **timeline parity**
check holds both.

Both clients print what they played for a recording in one format: one line of
JSON for each step, with its start and end in milliseconds from the start of its
script, as one seat sees it. A step a client made itself has `"own": true`.

```json
{"view":12,"beat":0,"step":1,"verb":"fly","case":"own spell cast","start":0,"end":370,"effects":["settle@end"],"sounds":[["cast","start",null]]}
```

`sounds` lists the sounds the step asks for, each as `[sound, edge, into]`: the
sound's name in `ui/src/audio/manifest.json`, the edge of the step it is asked
for at, and how far into the step after that edge it sounds, as a share of the
step's time, or null for at the edge. The list is sorted by sound, edge and then
`into`. On the web side it comes from `stepCues` in `ui/src/audio/steps.ts`, and
on Beryl 3D's from `sound::cues`.

- **The web interface's side** is a script, `ui/scripts/timeline.mjs`. It plays
  a recording through the stage's model in Node, with no browser and no drawing
  surface, and records each step as the stage begins and ends it
  (`ScriptStage.follow`).
- **Beryl 3D's side** is `just b3d timeline <recording>`.
- **The check**, `just b3d timeline-parity`, compares the two for every fixture
  and card spec moment, from both seats. Two timelines agree when they hold the
  same steps with the same verb, case, effects and sounds, and every start and
  end is within 1 millisecond.
- **Recordings that do not agree yet are listed** in
  `data/beryl3d/timeline-parity.json`, each with the number of its steps that
  differ. Like every allowlist under `data/`, its counts may only go down.
- **It runs in the `beryl3d-timeline` lane** when a Beryl 3D path changes, and
  in the sweep. It never runs for a change to the web interface (decision C17).
  When the web interface's timeline moves, the sweep lists the recordings that
  now differ, and Beryl 3D follows.

Some beat ends depend on where a client lays cards out, which the two clients do
differently on purpose: a beat lasts until the cards that close up after a card
left are at their new places, but only as long as that close-up is still each
card's latest motion, and on the web interface a strike waits while its way
would cross an earlier strike's. Beryl 3D follows the first rule on its own
table and does not hold strikes back. These differences are accepted by the
comparison itself (`laid_out_later` in
`tools/src/beryl_tools/beryl3d/timeline.py`), which recognises them from the two
timelines alone, rather than listed:

- The first step of a beat, other than a script's first, may begin later on the
  web, if a card vanished in the beat before, the web's beat begins no sooner
  than all of the script's earlier steps on the web have ended, and it waits
  longer after them than Beryl 3D's by no more than the web's close-up after
  that card: 0.4 of its `vanish` step, or for a cut the session's `closes`.
- A strike that is not the first step of its beat may begin a whole number of 25
  ms waits later on the web than the step before it.
- The steps after either are accepted while the web is later by the same amount,
  and every accepted step has the same verb, case, effects, sounds and length on
  both sides.

What the check does not cover is fixed by hand, and found by the parity list: a
screen's layout, a prompt's wording, and how an effect looks.

### The parity list

The **parity list** is a generated report of everything the web interface can
show, with each item's state in Beryl 3D. `just b3d parity` prints it, and the
sweep keeps it current. An example of its output, with invented numbers:

```text
fixtures      118 / 140 play with no fault
card specs    1,388 / 1,402 moments play with no fault, both seats
timelines     2,930 / 3,084 recordings agree with the web interface's
verbs         16 / 16
effects       14 / 14
marks         6 / 6
prompt shapes 12 / 12
screens       9 / 11   missing: testing screen, manual table lobby
conventions   77 / 77 entries confirmed
tools         6 / 8    missing: motion-review, arena-pixel-match
launch tests  macOS pass, Windows, iOS and Android not required
```

- **A fixture or spec moment is done** when Beryl 3D plays it from both seats,
  in both layouts, across the window matrix, with every interface check passing
  and its timelines agreeing with the web interface's. **A verb, effect, mark or
  prompt shape is done** when the recording named for it is done:
  `data/beryl3d/parity.json` names one recording for each verb, effect and mark
  of `data/ui-conventions.json`, and a test fails when the recording named for a
  verb or effect does not play it.
- **A tool is done** when a job that a skill or a `just` command does for the
  web interface can be done for Beryl 3D. The jobs are listed by hand in
  `data/beryl3d/parity.json`, beside a map from every skill to its job (or to
  why it looks at no client): showing a card's moments (the `specs` skill),
  reviewing the interface as a player (`ui-review`), filming a verb beside
  Arena's (`just motion --film`, `motion-review`, `arena-pixel-match`), working
  a bug report (`bugfix`), and measuring frames in the running app
  (`just ui-perf`).
- **A UI Conventions entry is done** when `data/beryl3d/conventions/<kind>.json`
  (one file per section kind of `data/ui-conventions.json`, which Beryl 3D does
  not edit) names a recording, a moment and a tree line that show Beryl 3D
  following it, and that recording is done. An entry it cannot follow without a
  new numbered exception is recorded as open, with the reason, for the owner.
- **A launch test** (only macOS's is required in this epic; Windows, iOS and
  Android are not tested) starts the app on one platform with no window shown
  and passes when the app draws a frame.
- **Prompt shapes** are counted from the `prompt_shapes` of
  `data/beryl3d/screens.json`, which names one fixture for each shape of
  `ui/shapes.json`.
- **Screens** are listed by hand in `data/beryl3d/screens.json`, because nothing
  generates them: the start screen, the opening hand and mulligan, the game,
  game over, settings, the bug report, the card gallery, the testing screen and
  each of the manual table's screens. Each screen marked built names a moment of
  its recording (`at`) and a line of the tree printout there (`shows`), and a
  test fails when the recording does not show it. A built screen counts once its
  recording is done. `parity.json` maps every screen file of the web interface
  to a screen of that list, or to `missing`.
- **New web interface work adds to the list** automatically where it adds a
  fixture or a spec. Adding a web screen file or a skill makes a sweep due, and
  its `parity-list` step fails until `parity.json` maps the new file or skill.

### The switch

A complete parity list is necessary for the switch and does not trigger it. The
project's owner says when Beryl 3D becomes the app, and separately when the old
code is deleted (decision C11).

When the owner switches:

- `just dev`, the release bundle and the phone builds produce Beryl 3D.
- The web interface takes no further changes.

When the owner says to delete, one change removes:

- the web interface's source, its Node dependencies and its build;
- the Tauri shell crate and the Tauri phone projects;
- the browser checks, their scheduling and the three-hourly run of all of them;
- the TypeScript protocol generation in `beryl_session`.

Until then, each client's checks run for its own changes.

### Platforms and versions

Beryl 3D runs on the platforms the web interface does, with a lower floor on
iOS.

| Platform | Minimum                              | Graphics                         |
| -------- | ------------------------------------ | -------------------------------- |
| macOS    | Apple silicon, as today              | Metal                            |
| Windows  | Windows 10                           | DirectX 12, else OpenGL 3.3      |
| iOS      | iOS 15, which reaches the iPhone 6s  | Metal                            |
| Android  | API level 24 (Android 7.0), as today | Vulkan, else OpenGL ES 3.0       |
| Linux    | the `beryl3d-bench` virtual machine  | software Vulkan, for counts only |

- **Beryl 3D's iOS host targets iOS 15**, the last version the floor runs. The
  Tauri app keeps its target of iOS 17 while it exists.
- **`wgpu` releases break its API about every three months.** Only
  `beryl_render` names a `wgpu` type, so an upgrade touches one crate.
- **Dependencies are pinned**, and an upgrade is a change of its own.

## Out of scope

The project's owner ruled these out on 2026-11-14.

- **Screen readers.** Every interactive element still has a role and a name, so
  an accessibility tree can be built later.
- **Keyboard-only play**, beyond the keyboard shortcuts the web interface has.
- **Text in scripts the bundled fonts lack.** A character none of them has is
  drawn as the missing-character box.
- **A floor device.** None is bought or attached (decision C13), and the owner
  has nothing older than a current iPhone.

## Alternatives considered

Each of these was a real option in the design interview. They are recorded so
the reasons are not rediscovered.

- **Each client keeping its own timing rules, held equal by a check.** It would
  have left the session's protocol and the web stage untouched. But the timing
  rules are the part of the web interface that changes most often, and every
  change would have been written twice. The owner chose to move them into the
  session instead.
- **Bevy.** A full engine with shadows, lighting, bloom and audio ready-made. It
  is an entity-component engine that owns the app's main loop through `winit`,
  which conflicts with decisions C3, C7 and C12. Every frame, even one that only
  animates, would run Bevy's scheduler and its copy of the scene into the
  renderer, third-party code that cannot be held to a counted, allocation-free
  budget. In a throwaway prototype of 2026-11-14 that compared the two, it used
  about twenty times the CPU per frame, four times the memory (185 MB) and nine
  times the executable size (41.7 MB) of `wgpu` used directly, with 24 threads
  and a 546-second build. It releases a breaking version every three to four
  months.
- **Porting the web interface's 2D coordinates.** Layout code would port line
  for line, and a check could compare the two clients' positions exactly. But
  perspective, shadows and lighting would stay imitations for good.
- **A CPU reference renderer with stored images.** Bit-identical pictures on
  every machine. It is a second renderer in which every shader is written twice,
  and stored images fail on every intended visual change.
- **`winit` on every platform.** One windowing code path. Its Android text entry
  is an unmerged draft, and it has had no stable release since March 2026.
- **Reserving a core for drawing.** Immune to scheduler behavior, at 30 to 50%
  of search throughput on a phone.
- **Drawing card faces as live layers each frame.** Sharp at any size, at 8 to
  25 quads and three to four blended layers per card, which a full battlefield
  cannot afford on the floor.
- **Drawing the Baseline tier back to front with blending and no depth buffer.**
  One pipeline, quads only, and antialiased edges for free. But blending stops
  every floor GPU from skipping hidden pixels, at an estimated cost of a third
  more pixels shaded.
- **Buying 2015-class phones to confirm the counts.** The only way to measure
  the floor instead of predicting it. Declined by the project's owner (decision
  C13); the frame log from real play is the evidence instead.
- **`miniquad` as the GPU layer.** Much smaller than `wgpu` and aimed at old
  OpenGL ES devices, but with no Vulkan or DirectX 12 and a thinner base for the
  Enhanced tier.
- **Slint for the flat layer.** Mature widgets, text entry and phone ports, but
  a second interface system beside the table with its own language and its own
  element tree, so every check and budget would be enforced twice.

## Manual QA

Automated checks cover layout, motion values and cost. A person checks what they
cannot: that the table feels right, that the phones behave, and that the agent
tools tell the truth about what is on screen.

The commands and debug options used here are specified in
[The agent interface](#the-agent-interface). `verbs-zones` and `pay-default` are
fixtures, and `stress-255` and `stress-1024` are stress boards.

### First assembled check

Do this as soon as a fixture can be drawn, before any broad testing. It crosses
every boundary in the design once: session recording, client core, renderer,
host and agent tool.

1. Run `just b3d tree verbs-zones --at 1500ms` and note the position of one card
   that is mid-flight.
2. Run `just b3d png verbs-zones --at 1500ms` and open the picture. The card is
   where the tree said, mid-air, with its shadow on the table beneath it.
3. Run
   `just b3d app --fixture verbs-zones --pause-at 1500ms --window foreground`.
   The window shows the same picture as the PNG.
4. Run `just b3d live tree` against the app. The tree matches step 1.

### A whole game on the desktop

1. Start a game against `expert`, Beryl's scripted AI opponent, and play it to
   the end.
2. Watch for every verb: a land played, a spell cast and countered, a draw, a
   creature attacking and dying, a token appearing, a life total changing. Each
   matches Arena's order and timing, and nothing jumps.
3. While the AI is thinking, move the pointer across the hand. Cards lift
   without stutter.
4. Resize the window from 1440 by 900 through 21:9 and back during an animation.
   Nothing overlaps, and moving cards arrive at the right places.
5. Open the bug report with Command-B, type a description and save. The saved
   report holds a picture, the element tree, the frame log and the replay.
6. Quit mid-game and relaunch. The game resumes where it was.
7. Quit mid-game, launch the web interface's app, and check that the same game
   resumes there. Then the other way round.

### Old and constrained hardware

1. In settings, force the Baseline tier. The table keeps every position and
   every motion; only lighting, the shape of shadows, bloom and particles
   change.
2. Choose Automatic on a machine that passes the startup measurement, then run
   the app with `--slow-gpu 20ms`. Within a second or two the log records a step
   down to Baseline, and no object moves when it happens.
3. Choose Enhanced and repeat. The tier stays Enhanced.
4. Run `just b3d png stress-255` and `just b3d png stress-1024`. Both boards are
   readable: rows have overlapped and identical permanents have piled up with
   counts.
5. On the oldest phone available, start the app with `--fixture stress-255` and
   read the frame log afterwards. It records no late frame in the Baseline tier.

### Phones

The headless matrix of the exit check (393 by 759, 375 by 647 and 430 by 932,
`just b3d test --phone-only`) is required. On 2026-11-16 the owner removed
verification on the iOS Simulator, the Android emulator and phones from this
epic, so the steps below are a manual checklist for whoever ships the hosts, not
an exit check, and the headless smoke commands (`just b3d ios-smoke`,
`android-smoke`, `ios-device`) are not run by it.

1. Install on an iPhone and on an Android phone, and play a full game on each.
2. Rotate the phone mid-animation, and send the app to the background and bring
   it back. The table returns exactly as it was, with no black frame and no
   reload.
3. Trigger a low-memory warning in the simulator. The app survives, and
   full-size card faces reload when next needed.
4. Open the bug report and type with the system keyboard, including autocorrect
   and dictation. The text arrives in the field correctly.
5. On an iPhone with a 120 Hz display, check that a card's flight is visibly
   smoother than on a 60 Hz phone.
6. Check that nothing interactive sits under the notch, the home indicator or
   Android's system bars.

### Agent tools

1. Introduce a deliberate overlap in a prompt's layout and run
   `just b3d check pay-default`. The fault names both buttons, the window size
   and the moment, and its `show:` command prints the overlap.
2. Make one verb's implementation 50 milliseconds longer than its data. The verb
   values check fails and names the verb.
3. Add an allocation to `frame`. The allocation gate fails and names the
   fixture.
4. With the app running, use `just b3d live do 'press card "Island"'` to play a
   land. The app plays it as if clicked.

### Checking the parity list

1. Run the parity list and pick five items it marks done, including one card
   spec and one manual-table screen.
2. Open each in both clients side by side. Beryl 3D shows everything the web
   interface shows, in the same order.
