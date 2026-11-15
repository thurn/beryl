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
  on a phone.

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
  desktop app therefore starts fullscreen, where macOS and Windows can show its
  surface without composing it with other windows, and a key leaves fullscreen.
  The prototype's one fullscreen run under load had fewer late frames than its
  windowed runs, and Arena also starts fullscreen. The frame log records the
  window mode, so the choice keeps being tested and is reversed if windowed play
  proves no worse.

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

Beryl 3D is six crates and two native programs for phones. Each crate has one
job, and each depends only on crates below it in this list.

The six crates are a Cargo workspace of their own, `rust/beryl3d`, with its own
lock file and build directory (decision C17). They reach the game through path
dependencies on three crates of the main workspace: `beryl_session`, the fixture
recorder in `beryl_cli`, and `beryl_art`, the image download queue and disk
cache that the Tauri shell also uses.

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
  textures and rasterizing glyphs.
- **`beryl_inspect`**, the tools that play recordings with no window and no GPU:
  the file of recordings, the interface test runner, the frame benchmarks, and
  the commands of `just b3d` that print text (see
  [The agent interface](#the-agent-interface)). It depends on the client core
  and on the code that records fixtures, and on no GPU crate, so it rebuilds
  quickly after an edit to the client core. The commands that draw pictures are
  a second program in `beryl_3d`, which has the renderer.
- **`beryl_scene`**, which this document calls **the client core**. It holds the
  3D table, the element tree, animation, layout, hit testing and the interface
  checks. It depends on `taffy`, `parley` and the protocol types of
  `beryl_session` (plain data: views, scripts, commands), and on nothing that
  touches a GPU, a clock, a thread or a file. It defines `Input` and `Request`,
  the types everything above it speaks to it in.

The boundary that matters most is around the client core. It has two entry
points, and everything a frame shows is a function of what has passed through
them:

```rust
impl Client {
    /// Input: a session update, a pointer or key event, a new window size.
    pub fn handle(&mut self, input: Input);
    /// The frame at time `now`, in nanoseconds since the client started.
    pub fn frame(&mut self, now: u64) -> &Frame;
}
```

`handle` and `frame` never read a clock, start a thread, take a lock or touch a
file. Given the same sequence of calls, a client executes the same instructions
and returns the same frames. The [Determinism](#determinism) section below lists
what keeps that true.

The two entry points divide the work by how often it happens.

- **`handle` does what an input changes.** It applies a session update, rebuilds
  a screen whose inputs changed, lays out elements and text, and starts
  animations. This is where `taffy` and `parley` run. It may allocate.
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
    pub sounds: Vec<SoundId>,     // the sounds that begin in this frame
    pub load: Load,               // draw calls, instances, upload bytes, and
                                  // the opaque and blended screen areas
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
    pub opaque_area: f32,     // in screens
    pub blended_area: f32,    // in screens
}

pub struct Work {
    pub elements: u32,        // elements visited
    pub segments: u32,        // segments evaluated
    pub layout_nodes: u32,    // layout nodes computed, in `handle`
    pub text_runs: u32,       // runs of text shaped, in `handle`
    pub records: u32,         // draw records written
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
  (glyph pages, icons, and the shapes of shadows and glows).
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
  farthest first. A **band** is a group of records drawn together. The bands, in
  order:
  1. contact shadows of cards above the table;
  2. the soft edges of cards, and the marks on cards lying on the table;
  3. marks on cards above the table (in flight, in a hand, on the stack, held up
     to be read);
  4. glows and streaks;
  5. the flat layer, in the order of its tree;
  6. cards shown inside flat-layer elements, such as a card browser, with their
     marks;
  7. flat-layer elements that must cover those cards, such as a tooltip.
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
  adds up numbers that predict GPU load: draw calls, instances, bytes to upload,
  and the screen area of its opaque and of its blended records, each divided by
  the screen's area. The [Performance gates](#performance-gates) section below
  holds these to limits, and counts the pixels actually shaded. A record's area
  is the rectangle that bounds its mesh, cut to the view volume. The one-pixel
  edge strip counts as no area, and clip rectangles are not taken into account.
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
  is "now".
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
  counted boundary, or the dependency is replaced.
- **No threads, locks or files.** Work that needs them lives in `beryl_assets`,
  and its results arrive as inputs.
- **No system fonts.** Text is laid out with the fonts bundled in the app
  (Cinzel, Crimson Pro, Open Sans and the mana symbol font), with system font
  fallback switched off, so a line breaks in the same place on every machine.
  The web interface takes these fonts from npm packages, as WOFF2 files that the
  Rust text crates cannot read. Beryl 3D commits the same families as TrueType
  files, with their licences, in `rust/beryl3d/crates/beryl_scene/fonts`, and
  embeds them in the client core at build time.
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
}

fn pose_at(segments: &[Segment], t: u64) -> Pose { /* pure */ }
```

Two cases of changing targets are specified, because they are where animation
code usually goes wrong.

- **A new view arrives while a script is playing.** It waits, and takes effect
  when the current script ends: its script plays then, and its prompt and
  buttons appear then. Scripts never overlap.
- **The window changes shape while a card is moving.** Every resting position is
  recomputed for the new shape. A segment in progress keeps its start time and
  duration and takes the new end pose. Its start pose is recomputed so that the
  object's pose at the current time is unchanged, so the card does not jump and
  arrives at the right place.

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
  pose the card has then. These are ordinary segments.
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
    Surface { size: Size, scale: f32, safe_area: Insets },
    Pointer(Pointer),                        // mouse, touch or pen
    Key(Key),
    Text(TextChange),                        // the edited field's new contents
    Assets(AssetReady),                      // a face or a glyph page is ready
    Lifecycle(Lifecycle),                    // suspended, resumed, low memory
    Tier(Tier),                              // Baseline or Enhanced
    Reply(Reply),                            // the session's answer to a command
}

/// What the client asks of the session: one variant for each function of
/// `beryl_session::Sessions` that a player's action calls. Each carries that
/// function's own argument types.
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
}

/// The session's answer. `Sessions::click` returns the prompt's next state
/// (`ClickReply`); every other function returns nothing or an error.
pub enum Reply {
    Click(Result<ClickReply, SessionError>),
    Done(Result<(), SessionError>),
}

impl Client {
    /// The next command for the session, if any. The host sends it to the
    /// session thread and passes the answer back as `Input::Reply`.
    pub fn next_command(&mut self) -> Option<SessionCommand>;
}

pub enum Request {
    BeginTextEntry { field: ElementId, rect: Rect, text: String },
    EndTextEntry,
    Share { path: PathBuf },
    Haptic(Haptic),
    OpenUrl(String),
}
```

What a worker tells the client core when it finishes. The pixels themselves go
to the renderer, and the client core learns only where they are:

```rust
pub enum AssetReady {
    /// A face is in a layer of the small or of the full-size array.
    Face { face: FaceId, size: FaceSize, layer: u16 },
    /// Glyphs are in a page of the atlas, each at a rectangle in texels.
    Glyphs { page: u16, glyphs: Vec<(GlyphKey, [u16; 4])> },
}

/// An index into the list of faces the client made when the match loaded.
pub struct FaceId(pub u16);
pub enum FaceSize { Board, Full }
/// One glyph of one bundled font at one size in physical pixels.
pub struct GlyphKey { pub font: u8, pub glyph: u16, pub size: u16 }

impl Client {
    /// The glyphs that laid-out text needs and no page holds yet. Each is
    /// returned once. The host hands them to the rasterizer.
    pub fn wanted_glyphs(&mut self) -> impl Iterator<Item = GlyphKey> + '_;
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
  and Windows. It starts fullscreen (decision C15). On macOS it paces frames
  with a display link (`NSView`'s, asking for the display's highest rate) and
  draws only on the link's ticks: an input or a session update reaches the
  client core at once and is drawn on the next tick. In the prototype, `winit`'s
  own redraw requests left a 120 Hz display at 60 Hz, and drawing on every input
  used up macOS's three drawables. On Windows, `winit`'s redraw requests are the
  ticks. F11, and ⌃⌘F on macOS, leave and re-enter fullscreen. The `background`
  window mode makes the app an accessory app on macOS, with no Dock icon, so its
  window never becomes active and never gets the keyboard.
- **The iOS host** is a Swift app of a few hundred lines. It owns a
  `CAMetalLayer` and a `CADisplayLink`, asks for the display's highest refresh
  rate, and calls the Rust library through C functions.
- **The Android host** is a Kotlin app of a few hundred lines. It owns a
  `SurfaceView` and a `Choreographer` frame callback, and calls the same C
  functions through JNI.

The C functions the phone hosts call:

```c
Beryl3d *beryl3d_new(const BerylConfig *config);
void beryl3d_surface(Beryl3d *, void *native_surface,
                          uint32_t width, uint32_t height, float scale,
                          BerylInsets safe_area);
void beryl3d_surface_lost(Beryl3d *);
void beryl3d_pointer(Beryl3d *, BerylPointer event);
void beryl3d_key(Beryl3d *, BerylKey event);
void beryl3d_text(Beryl3d *, const char *utf8);
void beryl3d_lifecycle(Beryl3d *, BerylLifecycle event);
/* Draws one frame. Returns when the next is needed: now, at a time, never. */
BerylWake beryl3d_frame(Beryl3d *, uint64_t now_nanoseconds);
/* The next thing the client asks of the platform, or none. */
bool beryl3d_next_request(Beryl3d *, BerylRequest *out);
```

- **A `Beryl3d` is the whole app**: the session, the client core, the renderer
  and the workers. `beryl_3d` exports these functions.
- **The session runs inside the Rust library.** Session updates and finished
  assets never cross the C boundary. They arrive on channels that
  `beryl3d_frame` drains before it draws.
- **Requests are polled.** After each frame the host calls
  `beryl3d_next_request` until it returns false.

Three behaviors are specified because they differ most between platforms.

- **Text entry.** On phones the host shows a native text field over the element
  being edited, so the system keyboard, autocorrect, dictation and input methods
  all work as the platform intends. The client sends `BeginTextEntry` with the
  field's rectangle and contents, and the host sends a `Text` input for every
  change. On the desktop the field is edited in place, using `parley`'s editing
  support and `winit`'s input-method events.
- **Losing the surface.** When an app is suspended, Android destroys the GPU
  surface and iOS forbids GPU work. The host calls `beryl3d_surface_lost`, the
  renderer drops the surface, and on return the host supplies a new one. Workers
  read the compressed textures back from the disk cache and the renderer uploads
  them before the first frame after the return is shown, which takes
  milliseconds. The client core's state is untouched, so the table reappears
  exactly as it was.
- **Low memory.** On a low-memory `Lifecycle` input the client drops every
  full-size card face, and the renderer switches to the Baseline tier for the
  rest of the session and frees the Enhanced tier's buffers. The small faces are
  kept.

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
- **One camera per layout, with a fixed orientation.** The desktop layout and
  the phone layout each have one perspective camera, defined by its pitch, its
  vertical field of view and the point on the table it looks at. It never turns,
  and it never moves while the window keeps its shape. The one exception is the
  `shake` effect, which slides the picture of the table a few pixels for a sixth
  of a second (see [Effects](#effects)).
- **The camera is fitted to Arena.** Its pitch and field of view are solved from
  the corners of cards measured in Arena's recordings, so the table's
  foreshortening matches the reference. The fitted values are data.

A window of another shape does not turn the camera. It moves the camera along
its own line of sight, nearer or farther:

- The layout names a rectangle of the table that must always be visible (both
  players' rows, the piles, the stack's place).
- The camera's distance is chosen so that rectangle fits in the window, inside
  the safe area and clear of the regions the flat layer has reserved.
- A wider window then shows more table cloth at the sides, and a narrower one
  moves the camera back.

### Objects and placement

Each game object has one **scene object** that lasts as long as the game object
does, keyed by the object's id in the view. A card that moves from a hand to the
stack to the battlefield is one scene object throughout, which is what lets a
flight start exactly where the card was.

- **Placement is a pure function.** For a view and a window shape, a **placement
  function** gives every object the pose it rests in. There is one function per
  zone: battlefield rows, hands, the stack, each pile, and the places cards are
  held up to be read.
- **The battlefield fits what it holds.** A row lays its cards out at full size
  until they would pass the row's width, then overlaps them, then stacks
  identical permanents into piles with a count, as the web interface does. A
  board at the engine's capacity is therefore still a bounded number of quads
  inside a bounded area.
- **An overlapped card keeps a strip of at least 24 by 24 logical points
  uncovered**, 44 by 44 on the phone layout, so it can always be pointed at.
- **A row that cannot give every card its strip scrolls.** After piling
  identical permanents, a row with more cards than fit at the minimum strip
  shows as many as fit and scrolls sideways under a drag or the wheel, with a
  count of the cards off each end. On a phone a board of 255 different
  permanents cannot fit any other way. Arena's recordings show no board this
  large, so UI Conventions lists the behavior among those not seen in Arena.
  Cards scrolled out of view are not drawn.
- **Hands, the stack and cards being read keep their place on screen.** Their
  positions are given relative to the camera, not the table, so they are the
  same size and in the same place at any window shape. They are still 3D
  objects: they tilt, they are lit and they cast shadows on the table below.
- **Marks are separate quads.** What a card's own record says (tapped, a power
  and toughness plaque, counters, summoning sickness) and what a prompt marks on
  it are drawn as a few quads placed on the card's face, so they change without
  touching the face's texture.

### Verbs

Each of the 16 verbs has one implementation in the client core, which turns a
script step into segments on the scene objects it concerns.

- **Timing comes from data, through the session.** A verb's duration, delay,
  easing curve and **stagger** (the delay between successive objects when one
  step moves several, such as three cards drawn a third of a second apart) are
  in `data/ui-conventions.json`. The session reads them and sends each step's
  numbers with its script. A change to a duration is one edit that both clients
  pick up.
- **Space comes from data too.** Values only a 3D client needs (how high a
  flight arcs, how far a held card lifts off the table, how far a striking
  creature lunges) are in a file of Beryl 3D's own, `data/beryl3d/space.json`,
  under the names of the verbs and cases in `data/ui-conventions.json`. A check
  fails for a name that is no verb or case. Tuning a 3D value does not touch the
  web interface's data file, so none of the web interface's checks run for it
  (decision C17).
- **A step ends when its animation ends.** A script's beats play in order, the
  steps of one beat together, on the time passed to `frame`.
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
into the session, in `beryl_session`'s `script::timing` module, which is the
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
                                // close up after it
    pub effects: Vec<StepEffect>,
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
  one function, in `beryl_scene`'s `beat` module.
- **A client makes a few steps itself.** While a prompt is open, the client
  draws its marks, its arrows and the cards it holds up as steps of its own,
  made from the prompt's state (`ui/src/stage/prompt.ts`). Their timing is a
  short fixed table, because it does not depend on the game: `beryl_session`'s
  `own_timings` function returns it. Beryl 3D calls that function, and the web
  interface asks for the table once at startup.
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
  named constants generated from one data file, which also generates the web
  interface's CSS variables, so the two clients cannot drift apart in color or
  type.
- **An element can show a scene object.** The card browser, the mulligan screen
  and the inspector lay out boxes with `taffy` and place real 3D cards in them,
  so a card looks and animates the same wherever it appears.

### Sizes

The flat layer is laid out in logical points, and the same code serves every
window.

- **Scale factor.** A logical point is one pixel on a standard display and two
  or three on a high-density one. Text and icons are rasterized at the display's
  real pixel size, so they are sharp at any density.
- **Two layouts.** The desktop layout and the phone layout are separate screen
  functions sharing their widgets, as the web interface's two layouts share its
  drawing code. The phone layout is used when the window's shorter side is under
  500 points, and the desktop layout otherwise.
- **Safe area.** The host reports the insets for notches, rounded corners and
  system bars. The root element pads itself by them, and a check holds every
  interactive element inside.
- **Minimum sizes.** On the phone layout a check holds every touch target to at
  least 44 by 44 points and every line of text to at least 11 points.

### Text

Text is laid out by `parley` in the client core and rasterized by
`beryl_assets`.

- **Layout** covers shaping, line breaking, alignment, styled spans inside one
  paragraph, and inline boxes. Mana symbols and other icons in rules text are
  inline boxes, so `{1}{U}` flows and wraps with the words around it.

- **Layout results are cached** by text, style and width. A prompt's text is
  laid out once, in `handle`, when the prompt arrives.

- **Glyphs are drawn from an atlas.** Each glyph is rasterized once at the pixel
  size it is shown at, into a single-channel texture page, and each glyph on
  screen is one quad. The Latin glyphs of every type style are rasterized at
  startup. Card names and rules text are known when a match loads and are
  rasterized then.

- **A character no bundled font has** is drawn as the font's missing-character
  box. Nothing is fetched from the system.

- **A glyph at a new size is not waited for.** When the display's scale changes
  (a window dragged to another monitor), glyphs are rasterized again on a
  worker. Until a page is uploaded, text is drawn from the old size, scaled.
  Layout does not depend on rasterization, so nothing moves when the new page
  arrives.

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
- **Two pipelines and three meshes.** One opaque pipeline and one blended, each
  a single shader that picks its branch from the record's `material`. A
  pipeline's depth test is fixed when it is created, so the blended pipeline is
  made twice, tested against the depth buffer for the table's bands and untested
  for the flat layer's, with the same shader: three pipeline objects, all made
  at startup. The card mesh, its edge strip and a unit quad are the only meshes.
  The 80-byte records are uploaded as the instance buffer unchanged. Each run of
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
  (as the prototype does). Every table and shadow pixel then pays for one unused
  texture fetch. On the floor that is the cost to watch; the alternative is
  `textureSampleLevel` inside the card branch, which needs no derivatives and so
  may branch, at the price of choosing the mipmap level by hand.
- **Card faces in a texture array.** All the small faces of a match are layers
  of one array texture, so every card on the table can be in one call. Cards
  shown at full size use a second array and add a call.
- **Shadows are single quads.** A card above the table (in flight, held up, in a
  hand) gets one pre-blurred shadow quad beneath it, drawn just above the cards
  lying on the table, which spreads and fades with the card's height. Cards
  lying on the table get none. A shadow drawn at one fixed height is hidden
  wherever the cards beneath it stand taller, as they do in a library pile or on
  a stress board whose overlapping rows rise 40 millimetres: the prototype
  raised its shadows on the stress boards to clear them. The real client should
  place a shadow at the height of what lies under it, which placement knows.
- **No per-pixel lighting.** A face's color is its texture times a tint.

What that buys, as an estimate for a typical board, in screens of pixels shaded
per frame:

| Drawn                        | All blended | Opaque with depth |
| ---------------------------- | ----------- | ----------------- |
| Table                        | 1.00        | 0.65              |
| Cards, with overlaps         | 0.45        | 0.35              |
| Shadows, edges, marks, glows | 0.15        | 0.15              |
| Flat layer                   | 0.30        | 0.30              |
| Total                        | 1.90        | 1.45              |

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
| `glow`     | A `glow` record under the card: a soft disc that spreads and fades.                                         | Bloom              |
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

- **The shapes of light** (a soft disc, a burst with rays, a ring, a cone, a
  comet) are sprites that `beryl_assets` draws once at startup into the atlas.
  No image file holds them.
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
  a shadow of its true shape on the table and on the cards below it. The
  Baseline tier's shadow quads are not drawn.
- **Per-pixel lighting.** Cards and the table are lit by the key light and a
  soft fill, with a faint sheen that moves across a card's face as it tilts.
- **Bloom** on glows and streaks, from a half-resolution blur.
- **Particles** for the looks of `vanish` (a destroyed permanent falling to ash,
  an exiled one drawn into a vortex), drawn as instanced quads with seeded
  motion.
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

### Choosing a tier

The tier is chosen once at startup, can only go down during play, and can be
fixed by a setting.

- **At startup the tier is Baseline**, and the game's first frames are drawn in
  it. During the loading screen the renderer draws the `stress-255` board in the
  Enhanced tier into a texture 30 times and measures how long the GPU takes. If
  the slowest of those is under a third of the display's refresh interval, the
  tier becomes Enhanced before the first turn. A floor device fails this test
  and never shows an Enhanced frame.
- **During play** the renderer watches the time between presented frames against
  the display's refresh interval (16.7 milliseconds at 60 Hz). If 3 of any 60
  consecutive animating frames arrive more than half an interval late, it
  switches to Baseline for the rest of the session and records the switch in the
  app's log. This covers a device that passed at startup and then heated up, or
  was moved to a larger display.
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

1. The client lists every face of both decklists and their tokens.
2. For each face, a worker looks for the compressed texture in the disk cache,
   keyed by the image's id, the size, the format and the encoder's version. A
   hit is a file read.
3. On a miss, the worker decodes the scan, or draws Beryl's frame, scales it to
   both sizes, compresses it and writes the result to the disk cache.
4. The board-size layers are uploaded during the loading screen.

Three crates do that work: `tiny-skia` (a CPU rasterizer for 2D shapes) and
`swash` (a glyph rasterizer) draw Beryl's frame, and `ctt` (a wrapper over
several texture encoders) compresses.

During play, two kinds of upload remain.

- **A full-size face**, when a card is first read. A worker reads or builds it
  and the frame thread uploads it, at most one face (0.84 MB) per frame. Until
  it lands, the board-size face is drawn in its place, enlarged.
- **A scan that had never been downloaded.** The download queue keeps Scryfall's
  limit of one image every 100 milliseconds, so a deck seen for the first time
  takes 15 seconds or more to fetch, and the match does not wait for it. Such a
  face starts as Beryl's drawn frame. The scan replaces it only in a frame with
  no animation playing, one layer per frame, so a card never changes its picture
  in the middle of a flight.

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
  channels, calls `frame`, and submits the draw list. It holds no lock that
  another of Beryl's threads can hold, and it does no file or network I/O. The
  only thing it waits for is the display.
- **The session thread** runs the game, as today.
- **Search threads** run the AI, as today, on every core. They run at a lower
  scheduling priority than the frame thread, so the operating system preempts
  one when the frame thread wakes. No core is reserved. The app sets their
  scheduling class itself (utility, on Apple platforms), and the frame thread's
  (user-interactive). A thread otherwise inherits the class of the thread that
  made it, and in the prototype a game started from the frame thread searched at
  drawing's priority.
- **Asset workers**, two to four, decode, compose, compress and rasterize.
- **The audio thread** is the audio device's callback.

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
repeated) and `--json`. Left out, they are `you`, `desktop`, 1600 by 900, 1,
`baseline` and the recording's start, with no action.

`just b3d` is two programs behind one command. The commands that print text are
`beryl_inspect`'s program, `b3d`, which contains no GPU code. `png` and `film`
are a second program, `b3d-render`, in `beryl_3d`, which has the renderer.
`just b3d` runs whichever the command needs, and both read their options with
the same code.

`stress-255`, used in examples below, is a stress board: a hand-built position
at the engine's capacity, defined in the [Performance gates](#performance-gates)
section.

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
  `drag <locator> to <locator>`, `type <locator> "<text>"`, `key <name>`,
  `move <x>,<y>`, `step <duration>` or `still` (run the clock until nothing
  moves). `--do` takes one action. A drive script is a file of them. The running
  app's socket reads them. The test driver's methods have the same names.

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
  (except on the stack), then its battlefield row. An element of the flat layer
  has no place. A name with a quote or a backslash in it is escaped with a
  backslash.
- **States** are words from a closed list: `interactive`, `tapped`, `face-down`,
  `moving(<verb> <percent>)` (`moving` alone until an element carries its verb),
  `hovered`, `held`, `eligible`, `selected`, `target`, `disabled`, `focused`,
  `covered(<percent>)` and `faded(<percent>)`.
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
  together and share one moment. `step:` moments need the script clock and fail
  until the client has it.
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
just b3d frames
just b3d live tree
just b3d live do 'press button "Auto Pay"'
just b3d smoke
```

- **`app`** runs the app. `--fixture <recording>` replays a recording instead of
  a game. `--pause-at <moment>` stops its clock there. `--drive <script>` plays
  a file of actions on the display's clock. `--load <threads>[:<class>]` starts
  busy threads, to imitate a loaded machine. `--slow-gpu <duration>` holds each
  finished frame back, to imitate a slow GPU. `--for <duration>` ends the run.
- **`--window`** says what is opened. `none` is the windowless host, which
  measures everything up to presentation, and is what an agent gets when it
  gives no `--window`. `background` is a window that never takes focus and opens
  behind the others. `foreground` is an ordinary window. `fullscreen` is what a
  person gets when they give no `--window` (decision C15). A run that an agent
  starts with a window must give `--for`, so that it always ends.
- **Every run writes a frame log of its own** and prints the log's path with a
  summary: frames drawn, late frames, frames held to 60 Hz on a faster display,
  and how long frames waited for a drawable. `just b3d frames [path]` prints a
  log's summary and its worst frames. With no path it reads the newest log of a
  run started from the same checkout.
- **`live`** speaks to a running debug build over a local socket named for that
  process: `live tree`, `live png` and `live do '<action>'`. When several apps
  are running it takes `--app <process id>`.
- **`smoke`** starts the app with no window, waits for its first presented frame
  and exits.

Release builds contain neither the socket nor the options that replay, drive,
load or slow the app.

## Interface checks

An **interface check** is a rule about a frame's resolved elements that must
hold on every frame. The checks are generic: they know nothing about any card or
screen, so they cover every card spec that is ever generated.

- **On screen.** Every interactive element lies inside the window's safe area.
- **No overlap.** No two interactive elements of the flat layer intersect,
  unless one contains the other in the tree. Cards are exempt, because rows and
  hands overlap them on purpose.
- **Reachable.** A hit test at the centre of an interactive flat-layer element
  returns that element or one inside it. Every interactive card at rest has an
  uncovered area of at least 24 by 24 points (44 by 44 on the phone layout)
  where a hit test returns it.
- **No duplicates.** No two elements share an id, a game object has exactly one
  scene object, and no two sibling elements of the flat layer share a role and a
  name.
- **Text fits.** No laid-out line is wider or taller than its element's box.
- **Named.** Every interactive element has a role and a non-empty name.
- **Clips fit.** The frame asks for no more than 15 clip rectangles. A clip
  asked for past the last entry is dropped, and the records that wanted it are
  drawn whole.
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
  is on screen while it plays: inside the window, at least half uncovered and
  not transparent. A step whose verb gives its subject time to travel moves it
  at least 4 logical points over at least 150 milliseconds; a subject that jumps
  instead is a fault.
- **Not crowded.** No more than 3 different moves begin at once in one beat.
  Taps and the steps of a turn do not count.
- **No blank card.** Every card drawn face up, whose name the viewer knows, has
  a face layer of its own.
- **Lines hold.** An arrow or line whose two ends are at rest does not change
  shape: between two frames in which it is not moving, neither control point of
  its curve moves 4 points or more while its two ends stay within 0.1.

The last four are the web interface's stage audits (`ui/scripts/stage-audit.mjs`
and `audit.mjs`), with the same thresholds. The thresholds are in
`data/beryl3d/gates.json`, under `audit`.

What the checks run over:

- **Every fixture, every card spec, every stress board**, from both seats.
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

How many there are, with the 47 cards the engine knows today:

- About 1,400 card spec moments and about 140 recorded games.
- Two seats and seven window sizes each.
- About 21,500 tests in all.

### The target

These numbers are targets on the development machine under its usual load. The
first measurement of the real client replaces them.

| Measure                                              | Target           |
| ---------------------------------------------------- | ---------------- |
| CPU time for one card spec moment's test             | 2 milliseconds   |
| All 21,500 tests, before a push of a Beryl 3D change | under 10 seconds |
| The same tests checked on every tick, in the sweep   | under 2 minutes  |

For comparison, the web interface's browser check of one animation takes 36
seconds at the 90th percentile, and the existing check of the session's scripts
for one card spec moment, which is plain Rust, takes 14 milliseconds in an
unoptimized build.

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
- the start and end of each transition of a flat-layer element;
- the first frame in which everything is still after each script.

Two checks need no frame at all. "No jumps" and "verb values" read the segments
directly.

Checking every tick remains, as the second line:

- **In the sweep**, after a push, the same tests run again with the checks
  applied on every tick. This is the exhaustive form. It is not part of slow CI,
  whose budget of CPU time it would spend several times over (see
  [Checks and isolation](#checks-and-isolation)).
- **A differential test** holds the two together. On a fixed sample of tests it
  plants each kind of fault at a random tick and requires the check times to
  find it. A fault only the every-tick run finds is a missing check time, and is
  fixed by adding one.

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
  graphics driver, and keeps either gate from waiting for the other. The counted
  boundary is one call to `frame` plus the `handle` calls since the previous
  frame.
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

If the software driver's fragment count proves not to be repeatable, the gate
computes the count itself: test code draws the frame's opaque records, nearest
first, into a depth buffer of its own, counts the pixels that pass, and adds the
pixels of the blended records.

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
      "frame_allocations": 0,
      "tolerance_percent": 1.0
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

### Finding the worst frame

Callgrind counts a whole run. Counting every frame separately would mean
thousands of measurements, so the gate finds its candidates first.

1. The client core keeps cheap **work counters** as it runs: elements visited,
   segments evaluated, layout nodes computed, records written. They are
   deterministic.
2. A native pass plays the fixture and ranks its frames by those counters.
3. The Callgrind benchmark plays the fixture again. The 8 highest-ranked frames
   go through a wrapper function, `frame_counted`, which makes that frame's
   `handle` calls and its `frame` call, and every other frame makes them
   directly. Callgrind's `--toggle-collect` option counts instructions only
   inside the wrapper.
4. The gate's worst frame is the largest of the 8 counts.

- **The same frames are chosen on every run**, because the work counters are
  deterministic.
- **A test checks that the counters rank frames correctly.** On the stress
  boards it counts every frame of a short stretch under Callgrind and requires
  the true worst frame to be among the 8 the counters ranked highest.
- **The work counters are also limited natively**, on every tick, in the check
  that runs before each push. That catches most regressions without the virtual
  machine.

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

These were taken 2026-11-14 on the development machine, an Apple M5 Max, at a
load average of 5 to 8 from other jobs, by the prototype in
`rust/prototypes/beryl3d` (`just proto3d --stats <board>`). The prototype is not
Beryl 3D: it is a throwaway program that keeps the doc's central split (a core
with no GPU, clock or thread that writes draw records (64 bytes each in the
prototype, 80 in the design since), and a `wgpu` renderer that draws them) and
leaves out text, prompts and the flat layer. Its numbers are replaced by the
real client's when it exists.

Each number is the median of five runs, with the worst of the five beside it.
The frames are 1920 by 1080, drawn into a texture.

- **live** is a real game, the WU Control mirror between the `mcts-hidden` agent
  and an `expert` agent answering for the viewer, played by `Sessions` with the
  AI's search threads running, 1,800 frames paced at 60 frames a second.
- **stress-255** and **stress-1024** are battlefields of 255 and 1,024 cards in
  overlapping rows, with 40 cards in flight at any time, 1,200 frames drawn back
  to back.

| Measure                                   | live         | stress-255   | stress-1024  |
| ----------------------------------------- | ------------ | ------------ | ------------ |
| Core time per frame, mean                 | 22 µs (22)   | 12 µs (12)   | 31 µs (32)   |
| Core time, worst single frame             | 241 µs (490) | 22 µs (43)   | 62 µs (66)   |
| Encode and submit per frame, mean         | 279 µs (283) | 36 µs (38)   | 40 µs (43)   |
| Heap allocations per core frame after 1 s | 0            | 0            | 0            |
| Draw records, most in a frame             | 126          | 296          | 1,065        |
| Draw calls, most in a frame               | 2            | 2            | 2            |
| Opaque records' screen area, mean         | 1.68 screens | 3.02 screens | 9.23 screens |
| Blended records' screen area, most        | 0.42 screens | 0.67 screens | 0.73 screens |
| Submit to GPU done, as built              | 439 µs       | 225 µs       | 259 µs       |
| Submit to GPU done, all blended, no depth | 539 µs       | 282 µs       | 423 µs       |

What these show:

- **`frame` allocates nothing**, on every board, including the frames in which a
  new script starts and is turned into segments. The core sizes its arrays at
  startup and plays scripts from small copies of the records it needs.
- **Two draw calls a frame**, because the table and the shadows are the card
  mesh scaled. The doc's limit of 16 is far off.
- **The opaque pass with a depth buffer is faster on this GPU too.** The same
  frame drawn all blended, back to front, with no depth buffer took 1.2 to 1.6
  times as long to finish. The gap grows with overlap: stress-1024's opaque
  records cover 9.2 screens before the depth test, and Apple's GPU shades only
  what is in front. The floor's GPUs do the same for opaque pixels, so this
  confirms the direction of the Baseline table's estimate, not its numbers.
- **Blended area stays inside the 0.75-screen limit**, but stress-1024 comes
  within 3% of it with shadows alone, before any mark, glow or flat layer. The
  real client's stress boards will exceed it, and the limit or the shadows' size
  will need a decision from a measurement.
- **The worst core frame is the one that starts a script.** It walks every
  object slot several times (placement once per beat), which the prototype sized
  for 1,100 objects whatever the board. Walking only the live objects, as the
  real client should, removes most of it.
- **Paced frames cost several times what back-to-back frames do.** live is paced
  like a window; the stress boards are not. Run paced at 60 frames a second
  (`BERYL3D_PACED=1`), stress-255's core time rose from 12 to 42 µs, its encode
  and submit from 36 to 354 µs, and its time to a finished frame from 225 to 433
  µs, at a load of 5 to 7. The processor and the GPU lower their clocks while
  the thread sleeps between frames. Asking for the user-interactive scheduling
  class (`QOS_CLASS_USER_INTERACTIVE`) did not change this: live measured 257 µs
  to encode and submit without it and 279 µs with it, within the runs' spread. A
  frame's cost must therefore be timed paced, and the instruction counts the
  gates use are unaffected by clocks, which is one more reason to gate on them.
- **The AI's search did not lengthen frames.** In live, the longest frame (core
  plus encode) while the AI searched was 0.70 ms (worst run 1.9 ms), and while
  it did not, 1.15 ms (2.6 ms), over about 580 frames drawn during search per
  run. The search threads ran at their default priority. The machine was lightly
  loaded, so this does not yet test decision C9's priorities.
- **In the window, macOS paced frames at 60 Hz on a 120 Hz display.** Run by the
  project's owner on a ProMotion Mac at load 20, about 25 of 31 frames flagged
  late had waited 16.3 ms for the drawable, with the core and encode under 0.3
  ms: the window was simply presented at 60 Hz. macOS keeps a window at its
  highest rate only when the app asks through a display link, and `winit`'s
  on-demand redraws with `wgpu`'s FIFO presentation do not ask. The desktop host
  therefore needs a display link on macOS (`NSView`'s display link with a
  preferred frame rate range), as the iOS host already plans with
  `CADisplayLink`.
- **Frames are drawn only on the display link's ticks.** The prototype first
  drew a frame for every mouse move and every session update as well as on each
  tick, submitting frames faster than the display showed them. It used up
  macOS's three drawables and then waited whole display intervals for the next.
  With frames drawn only on ticks, inputs only reach the core, and its wake
  keeps the link running. The late frames left in the owner's runs, at a load of
  42 to 51, waited 17 to 90 ms for a drawable while the GPU finished each frame
  in about 1 ms and the core and encode took under 1 ms. That points at the
  window server under the machine's load, outside the app. The real client's
  frame log should record the wait for a drawable apart from its own work, so a
  late frame on a player's machine can be attributed the same way.
- **Search threads inherit the frame thread's priority unless started from
  elsewhere.** A macOS thread inherits the scheduling class of the thread that
  creates it, and neither the session nor the search pool sets one. The
  prototype first started its game from the frame thread after putting that
  thread in the user-interactive class, so every search thread ran level with
  drawing, and the same run showed a few stalls of 50 to 230 ms. The game is now
  started from a thread in the utility class. The real client must set the
  search threads' class explicitly rather than rely on where they are started.
- **The floor's limits.** Every board drew on a device created with
  `wgpu::Limits::downlevel_webgl2_defaults()` with no validation error.

| Measure                                   | Result                          |
| ----------------------------------------- | ------------------------------- |
| Release executable, stripped              | 33.7 MB                         |
| Peak memory                               | 109 MB in live, 90 MB in stress |
| Start to first frame, 39 scans decoded    | about 130 ms                    |
| Card faces at 256 by 360, uncompressed    | 41 layers, 15 MB                |
| Release build of the prototype from clean | about 65 seconds                |

- **Most of the executable is the game, not the client.** The earlier `wgpu`
  prototype with no game was 4.6 MB. Linking `beryl_session` brings the engine,
  the cards, the card store and the search.
- **Peak memory includes the session and the AI.** The faces are 15 MB of it.
- **GPU timestamps do not work on this Mac.** `wgpu` 30 offers
  `Features::TIMESTAMP_QUERY` on Metal, but a render pass's end timestamp comes
  back 0.2 to 0.3 ms earlier than its start. The table above therefore gives the
  wall-clock time from submitting a frame to the GPU finishing it, which
  includes the driver's latency. GPU time on the Mac needs Xcode's tools.
- **What earlier prototypes measured.** Two throwaway programs before this one
  timed a frame's inner loop with no GPU (54 µs for 600 cards, with no sorting
  of real poses) and compared `wgpu` used directly with the Bevy engine (Bevy:
  41.7 MB, 185 MB of memory, 24 threads, a 546-second build). Those were the
  numbers that chose `wgpu`; this prototype's replace their frame times.

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
- **Tools of its own.** Everything behind `just b3d` is one Python package,
  which the main tools import in one place. Adding a check to Beryl 3D edits
  that package and nothing else, so no other lane's record of its own tools
  changes and no other lane runs again.
- **Data of its own.** Limits, thresholds, spatial values and allowlists are in
  `data/beryl3d/`. Beryl 3D adds nothing to `data/ui-conventions.json`, which
  the web interface's checks watch, except an entry under "Not seen in Arena"
  when it builds something Arena does not show.
- **A virtual machine and a name of its own.** Counts are taken in
  `beryl3d-bench`, and every check it logs is named `b3d:` and what it checks.

What runs, and when:

| When                                                                             | What                                                                                                                                                                                                                                    | If it fails                                                             |
| -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Before a push that changes a Beryl 3D path: the `beryl3d` lane of the fast check | Formatting, Clippy and the tests of the workspace. The source rules (`scene-purity`). Every interface test at its check times. The work counters and the allocation gate. Timeline parity.                                              | The push is refused, as for any lane.                                   |
| After a push, on staging: `just b3d sweep`                                       | The same, then every interface test on every tick, the instruction and fragment counts in the virtual machine, every gated fixture drawn on the floor's feature set, the app's launch test, and a build for each phone and for Windows. | A task is filed under Beryl 3D's epic. The `master` branch is not held. |

- **The sweep is not slow CI.** It is modelled on the browser sweep
  (`just sweep`). It runs at low priority on the staging tip when a Beryl 3D
  path, or a path of the main workspace that Beryl 3D depends on, has changed
  since its last run. Its cost is outside slow CI's budget of CPU time.
- **A step that the fast check cuts off at its time limit** is left to the
  sweep, not to slow CI.

What isolation costs, and who pays:

- **A change elsewhere can break Beryl 3D unseen.** Beryl 3D links the session,
  the fixture recorder and the image cache. A change to one of them is checked
  without Beryl 3D, so it can land and leave Beryl 3D not compiling, or playing
  a script wrongly.
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
- **One import of Beryl 3D's tools package**, the `just b3d` recipe, and the
  rule that sorts paths into lanes.
- **Design tokens**: the data file that generates the web interface's CSS
  variables as well as Beryl 3D's constants.

## Sound

Beryl 3D plays the same sounds as the web interface, from the same list:
one-shot sounds tied to script steps and looping ambience.

- **The files and the list are the web interface's**: the M4A files under
  `ui/public/audio`, listed in `ui/src/audio/manifest.json`, with
  `ui/src/audio/steps.ts` saying which step plays which sound. Beryl 3D reads
  the same manifest and ports the rule in `steps.ts`.
- **Playback** uses `rodio` over `cpal`, on the audio device's own thread.
- **Sounds are decoded at startup** into memory, so playing one is a message to
  the audio thread and never a file read.
- **A sound is triggered by the client core**, which emits a sound request when
  a step that has one begins. Tests read those requests like any other output,
  so the timing of a sound against its animation is checkable.
- **Android before 8.0 is silent.** `cpal`'s Android backend needs API level 26.
  The game is otherwise complete there.

## Bug reports

The bug report keeps its place and its contents, and gains two things a WebView
could not give.

- **The picture** is the renderer's own frame, read back from the GPU. It needs
  no screen-recording permission and no platform snapshot code.
- **The element tree** at the moment of the report is saved as text beside the
  picture, so whoever fixes the bug can see every element's position without
  reproducing it.

The frame log and the session's replay file are attached as they are today.

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
- **Bug report storage, profiles and settings**, moved out of the Tauri shell
  into a crate both shells use.

What they do not share is placement. The web interface draws the table on a 1600
by 900 surface and imitates perspective by shrinking and tilting each card.
Beryl 3D's table is a real scene. Each is fitted to Arena separately.

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
{"view":12,"beat":0,"step":1,"verb":"fly","case":"own spell cast","start":0,"end":370,"effects":["settle@end"]}
```

- **The web interface's side** is a script, `ui/scripts/timeline.mjs`. It plays
  a recording through the stage's model in Node, with no browser and no drawing
  surface, and records each step as the stage begins and ends it
  (`ScriptStage.follow`).
- **Beryl 3D's side** is `just b3d timeline <recording>`.
- **The check**, `just b3d timeline-parity`, compares the two for every fixture
  and card spec moment, from both seats. Two timelines agree when they hold the
  same steps with the same verb, case and effects, and every start and end is
  within 1 millisecond.
- **Recordings that do not agree yet are listed** in
  `data/beryl3d/timeline-parity.json`, each with the number of its steps that
  differ. Like every allowlist under `data/`, its counts may only go down.
- **It runs in the `beryl3d` lane** when a Beryl 3D path changes, and in the
  sweep. It never runs for a change to the web interface (decision C17). When
  the web interface's timeline moves, the sweep lists the recordings that now
  differ, and Beryl 3D follows.

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
launch tests  macOS pass, Windows pass, iOS pass, Android pass
```

- **A fixture, spec moment, verb, effect, mark or prompt shape is done** when
  Beryl 3D plays it from both seats, in both layouts, across the window matrix,
  with every interface check passing and its timelines agreeing with the web
  interface's.
- **A tool is done** when a job that a skill or a `just` command does for the
  web interface can be done for Beryl 3D. The jobs are listed by hand in the
  same data file as the screens: showing a card's moments (the `specs` skill),
  reviewing the interface as a player (`ui-review`), filming a verb beside
  Arena's (`just motion --film`, `motion-review`, `arena-pixel-match`), working
  a bug report (`bugfix`), and measuring frames in the running app
  (`just ui-perf`).
- **A UI Conventions entry is done** when its data records that Beryl 3D follows
  it, with the fixture that shows it.
- **A launch test** starts the app on one platform with no window shown and
  passes when the app draws a frame.
- **Screens** are listed by hand in one data file, because nothing generates
  them: the start screen, the opening hand and mulligan, the game, game over,
  settings, the bug report, the card gallery, the testing screen and each of the
  manual table's screens.
- **New web interface work adds to the list** automatically where it adds a
  fixture or a spec, and by a line in the screens file otherwise.

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
  budget. In the prototypes under [Measurements](#measurements) it used about
  twenty times the CPU per frame, four times the memory and nine times the
  executable size of `wgpu` used directly. It releases a breaking version every
  three to four months.
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
