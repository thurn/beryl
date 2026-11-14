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
4. **Deterministic interface tests** that enforce layout and motion rules
   quickly and on every state.
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
  show (`beryl_session`). It does not change in this design.
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
  check that runs after, and their parts, into which the new checks fit.
- [Manual multiplayer](manual-multiplayer.md): a rules-free table for two
  people. Its screens are part of what Beryl 3D must reproduce.
- `ui/AGENTS.md` and `rust/AGENTS.md`: how the web interface and the Rust crates
  are tested today.
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
  fragments a frame shades and the cycles each shader costs. No 2015-class phone
  is bought or attached to confirm them. A counted number is a stand-in for
  device performance: where the two are found to disagree, the device is right,
  and no design choice is made to suit a count at the device's expense.

## What does not change

Beryl 3D is a second player of scripts the session already writes. Nothing in
the engine, the rules, the AI or the session changes for it.

- **The session's protocol.** Beryl 3D receives the same `GameUpdate` values (a
  `PlayerView`, which carries its `Script`) and sends the same commands
  (`choose`, `click`, `cancel`, `undo`, `pass_on`, `concede`, `set_stops`). It
  links `beryl_session` directly, so the values cross a channel as Rust types
  with no serialization.
- **Scripts and verbs.** The client plays scripts and infers nothing, as the web
  interface does. A script's steps name verbs from the closed list, and the
  client has one implementation of each.
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

Beryl 3D is five crates and two native programs for phones. Each crate has one
job, and each depends only on crates below it in this list.

- **`beryl_3d`**, the app. It connects the session, the client core, the
  renderer, the background workers and audio. It is built as an executable on
  the desktop and as a library for the phone programs, and it exports the C
  functions those programs call.
- **`beryl_host`**, the platform boundary. It defines the interface between the
  client and a platform (see the [Hosts](#hosts) section below), and contains
  the windowless and desktop implementations.
- **`beryl_render`**, the renderer. It draws a frame with `wgpu`. It is the only
  crate that names a `wgpu` type.
- **`beryl_assets`**, everything prepared off the drawing thread: downloading
  and decoding card images, composing card faces, compressing textures and
  rasterizing glyphs.
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
    pub role: Role,            // Card, Button, Text, Region, Player, ...
    pub name: NameRef,         // "Counterspell", "Auto Pay"
    pub corners: [Vec2; 4],    // on screen, in logical points
    pub depth: f32,            // nearer the camera is smaller
    pub opacity: f32,
    pub clip: Option<Rect>,
    pub interactive: bool,
    pub state: StateBits,      // tapped, moving, selected, disabled, ...
}
```

A draw record is 64 bytes, so 1,000 of them are 64,000 bytes written in order:

```rust
#[repr(C)]
pub struct Draw {
    pub model: [f32; 12],   // a 3 by 4 transform, world space     (48 bytes)
    pub uv: [u16; 4],       // rectangle in the texture layer       (8 bytes)
    pub layer: u16,         // which layer of the texture array     (2 bytes)
    pub material: u8,       // index into the closed material list  (1 byte)
    pub flags: u8,          // which texture array and which mesh   (1 byte)
    pub tint: [u8; 4],      // color and opacity                    (4 bytes)
}
```

The rules for building a frame:

- **No allocation.** The resolved elements and the draw list are arrays the
  client owns and clears each frame. They are sized at startup for the largest
  board the engine can hold. After a game has started, `frame` performs zero
  heap allocations; a frame that outgrew an array would grow it, and the
  allocation gate would fail.
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
  covered too. Cards in a row that overlap each lie 0.3 millimetres above the
  one they cover, so which is on top is a matter of position, never of drawing
  order. The flat layer is drawn without the depth test.
- **Few draw calls.** Consecutive records that use the same mesh and texture
  array are one instanced draw call, whatever their materials. Every blended
  record outside a card face samples the atlas, so a whole band is one call. A
  frame is normally four to eight calls.
- **Load counted as it is built.** While it builds the list, the client core
  adds up numbers that predict GPU load: draw calls, instances, bytes to upload,
  and the screen area of its opaque and of its blended records, each divided by
  the screen's area. The [Performance gates](#performance-gates) section below
  holds these to limits, and counts the pixels actually shaded.

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
- **Seeded effects.** A particle effect's randomness comes from a fixed
  generator seeded by the object's id and the step's index.

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
    Update(GameUpdate),                      // from the session
    Surface { size: Size, scale: f32, safe_area: Insets },
    Pointer(Pointer),                        // mouse, touch or pen
    Key(Key),
    Text(TextChange),                        // the edited field's new contents
    Assets(AssetReady),                      // a texture or glyph page is ready
    Lifecycle(Lifecycle),                    // suspended, resumed, low memory
    Tier(Tier),                              // Baseline or Enhanced
}

pub enum Request {
    BeginTextEntry { field: ElementId, rect: Rect, text: String },
    EndTextEntry,
    Share { path: PathBuf },
    Haptic(Haptic),
    OpenUrl(String),
}
```

There are four hosts.

- **The headless host** has no window. A test or the command-line tool calls
  `handle` and `frame` directly and reads the frames. It can also own a renderer
  that draws into a texture instead of a window, for pictures.
- **The desktop host** uses `winit` 0.30, the current stable release, on macOS
  and Windows.
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
  and it never moves while the window keeps its shape.
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

- **Timing comes from data.** A verb's duration, delay, easing curve and
  **stagger** (the delay between successive objects when one step moves several,
  such as three cards drawn a third of a second apart) are read from
  `data/ui-conventions.json`, the same entries the web interface reads. A change
  to a duration is one edit that both clients pick up.
- **Space comes from data too.** Values only a 3D client needs (how high a
  flight arcs, how far a held card lifts off the table, how far a striking
  creature lunges) are added to the same entries, and the web interface ignores
  them.
- **A step ends when its animation ends.** A script's beats play in order, the
  steps of one beat together, on the time passed to `frame`.
- **Objects come to rest where placement puts them.** When a script has
  finished, every object's pose equals what its placement function gives for the
  new view. A check holds this on every fixture, so animation code cannot leave
  an object slightly out of place.

A verb's entry, with the spatial values added. Durations are in milliseconds and
distances in millimetres:

```json
{
  "name": "fly",
  "values": { "duration": 420, "curve": [0.2, 0.0, 0.1, 1.0], "stagger": 330 },
  "space": { "arc": 55 }
}
```

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
                .on_press(SessionCommand::Click(button_view.id))
        }))
}
```

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
  a single shader that picks its branch from the record's `material`. The card
  mesh, its edge strip and a unit quad are the only meshes. The 64-byte records
  are uploaded as the instance buffer unchanged.
- **Card faces in a texture array.** All the small faces of a match are layers
  of one array texture, so every card on the table can be in one call. Cards
  shown at full size use a second array and add a call.
- **Shadows are single quads.** A card above the table (in flight, held up, in a
  hand) gets one pre-blurred shadow quad beneath it, drawn just above the cards
  lying on the table, which spreads and fades with the card's height. Cards
  lying on the table get none.
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

```toml
[tier.baseline]
draw_calls = 16          # per frame
fragments = 1.75         # pixels shaded, in screens
blended_area = 0.75      # screen area of blended records, in screens
upload_bytes = 262_144   # instance data per frame, room for 4,096 records
```

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

```toml
[tier.enhanced]
draw_calls = 32
fragments = 4.0
particles = 2_000
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

The download queue and disk cache already exist in the Tauri shell. They move
into `beryl_assets` unchanged in behavior, and the Tauri shell uses them from
there.

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
  one when the frame thread wakes. No core is reserved.
- **Asset workers**, two to four, decode, compose, compress and rasterize.
- **The audio thread** is the audio device's callback.

Communication is by channels only:

```rust
// Session thread to frame thread: typed values, no serialization.
updates: Receiver<GameUpdate>,
// Frame thread to session thread: commands, answered later by an update.
commands: Sender<SessionCommand>,
// Workers to frame thread: finished textures and glyph pages.
assets: Receiver<AssetReady>,
```

- **A click is asynchronous.** The frame thread sends the command and draws the
  next frame. The session's reply (new marks, button states) arrives as an
  input. The session computes a reply to a click in microseconds, so in practice
  it is drawn in the very next frame.
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

### The command-line tool

One command shows any moment of any fixture or card spec. It runs the client
core under the headless host.

```bash
just stage pay-default --seat you --at 1250ms --tree
just stage counterspell/cast --layout phone --size 393x759 --png
just stage verbs-zones --film 0ms..3000ms --every 100ms
just stage stress-255 --check
```

- **`--tree`** prints the resolved elements as an indented tree. This is the
  default way to see what is on screen.
- **`--png`** draws the frame with the real renderer into a texture and writes a
  PNG, in either tier.
- **`--film`** writes a strip of frames across a time range, to judge motion.
- **`--check`** runs every interface check over the whole fixture and prints the
  faults.
- **`--size`, `--layout`, `--seat`, `--scale` and `--tier`** choose the window,
  the viewer and the look.

`stress-255` in the last example is a stress board: a hand-built position at the
engine's capacity, defined in the [Performance gates](#performance-gates)
section below.

The tree printout names every element with its role, its name, its id, where it
is and what state it is in:

```text
game 1600x900 seat=you t=1.250s
  table
    card "Counterspell" #41 stack moving(fly 62%) 1180,402 116x162
    card "Island" #7 battlefield.yours.lands tapped 412,640 96x134
    card "Island" #8 battlefield.yours.lands 516,640 96x134 eligible
  layer
    region "prompt" 520,820 560x64
      text "Pay {1}{U}" 536,838 180x28
      button "Auto Pay" 880,832 96x40
      button "Cancel" 984,832 80x40
```

A fault names the element, the moment, the rule and the command that reproduces
it:

```text
FAULT overlap  pay-default seat=you size=375x647 t=0.850s
  button "Auto Pay" (188,598 96x44) overlaps button "Cancel" (270,598 80x44)
  reproduce: just stage pay-default --seat you --size 375x647 --at 850ms --tree
```

### The test driver

Tests drive the client inside the test process with an API shaped like
Playwright's, which the current browser checks already use. It needs no page, no
server and no waiting, because the test owns the clock.

```rust
let mut page = Page::fixture("pay-default", Side::You, Layout::Desktop);
page.locator(Role::Button, "Auto Pay").press();
page.run_until_still();
assert_eq!(page.locator(Role::Card, "Counterspell").zone(), Zone::Stack);
page.check();

// Several cards share a name: narrow the locator.
page.locator(Role::Card, "Island").nth(1).hover();
```

- **Locators** find elements by role and name, optionally inside another
  element. Several cards can share a name, so a locator that matches more than
  one element must be narrowed with `nth` or with the element's id. A locator
  that matches nothing, or matches several without being narrowed, fails with
  the tree printed.
- **Actions** are `press`, `hover`, `drag_to`, `type_text` and `key`. They enter
  through `handle` as pointer and key inputs at a point where the element is
  uncovered, the same path a real click takes.
- **The clock** is advanced with `step(duration)` and `run_until_still()`. A
  **tick** is one step of a test's clock, a sixtieth of a second unless the test
  says otherwise.
- **`check()`** runs every interface check on the current frame.

### Driving the running app

Debug builds of the app listen on a local socket and accept the same commands as
the test driver. An agent can therefore inspect and drive a real game on the
Mac, in the iOS Simulator or in the Android emulator.

```bash
just stage --live --tree
just stage --live --press 'button "Auto Pay"'
just stage --live --png
```

Debug builds also take options that put the app into a known state, for agents
and for manual testing:

- **`--fixture <name>`** starts the app replaying a fixture instead of a game.
- **`--pause-at <time>`** stops the clock at a moment of the fixture.
- **`--delay-frames <milliseconds>`** holds each finished frame back before it
  is shown, to imitate a slow GPU.

Release builds contain neither the listener nor the options.

## Interface checks

An **interface check** is a rule about a frame's resolved elements that must
hold on every frame. The checks are generic: they know nothing about any card or
screen, so they cover every card spec that is ever generated.

- **On screen.** Every interactive element lies inside the window's safe area.
- **No overlap.** No two interactive elements of the flat layer intersect,
  unless one contains the other in the tree. Cards are exempt, because rows and
  hands overlap them on purpose.
- **Reachable.** A hit test at the centre of an interactive flat-layer element
  returns that element. Every interactive card at rest has an uncovered area of
  at least 24 by 24 points (44 by 44 on the phone layout) where a hit test
  returns it.
- **No duplicates.** No two elements share an id, a game object has exactly one
  scene object, and no two sibling elements of the flat layer share a role and a
  name.
- **Text fits.** No laid-out line is wider or taller than its element's box.
- **Named.** Every interactive element has a role and a non-empty name.
- **Phone sizes.** On the phone layout, touch targets are at least 44 by 44
  points and text at least 11 points.
- **No jumps.** Where one of an object's segments ends and the next begins, the
  two poses are equal. Only `appear`, `vanish` and a script that starts the
  table over may place an object without a path.
- **Rest equals placement.** When a script has finished, every object is where
  its placement function puts it.
- **Verb values.** Each verb's measured duration, delay and stagger equal its
  data entry.

What the checks run over:

- **Every tick.** Each fixture is played at 60 ticks a second from both seats,
  and the checks run on every frame, not on sampled moments.
- **A matrix of windows.** In the desktop layout: 1440 by 900, 1600 by 900, 1920
  by 1080 and 2560 by 1080. In the phone layout: 393 by 759 and 375 by 647, the
  two sizes the web interface's phone checks use, and 430 by 932.
- **Every fixture, every card spec, every stress board.**

What that costs, as an estimate to be replaced by measurement:

- A frame of the measured prototype (see [Measurements](#measurements)) takes
  about 54 microseconds. Allow 100 for the real client core with layout, text
  and the checks themselves.
- One card spec moment of 10 seconds is 600 frames, so 60 milliseconds for one
  seat at one window size.
- 1,400 moments, two seats and seven window sizes come to about 20 minutes of
  CPU time.
- The 134 recorded games, at about a minute each, add about 11 minutes.
- Together that is about half an hour of CPU time, or under two minutes across
  the development machine's 18 cores.

So the split between the two automated checks is:

- **Before every push**: the fixtures and card specs a change can affect, at one
  desktop size and one phone size, on every tick.
- **After every push**: everything, over the whole matrix, on every tick.

A fix for an interface bug adds the broken state as a fixture, so the generic
checks cover it from then on. A scripted check for one screen is written only
when no generic check can express the rule, as for the web interface.

A small set of **renderer tests** covers what numbers cannot: they draw known
scenes into a texture and sample a few single pixels with a tolerance, to catch
a wrong texture, a blending mistake or a missing glyph.

## Performance gates

The performance gates turn "never drops a frame" into a property that a check
holds on every change, without a device and without timing noise. They count the
client core's work, which is identical on every run.

### What is counted

Two kinds of work are counted: what the client core does on the CPU, and what
the draw list asks of the GPU.

On the CPU:

- **Instructions per frame**, under Callgrind in the `beryl-bench` Linux virtual
  machine, as the engine's count gate is. The counted boundary is one call to
  `frame` plus the `handle` calls since the previous frame.
- **Allocations in `frame`**, from a counting global allocator in a native test.
  The limit is zero once a game has started. `handle` may allocate, and its
  allocations are counted and limited per fixture.
- **Peak heap bytes** of the client core over a fixture.

On the GPU, time cannot be counted, but work can:

- **Fragments shaded per frame.** The renderer draws the frame in the virtual
  machine on a software Vulkan driver and reads the driver's own count of
  fragment shader runs (a pipeline statistics query). The count is exact and the
  same on every run, and it leaves out pixels the depth buffer rejected.
- **Cost per fragment, per material.** Arm's offline shader compiler reads each
  compiled shader and prints the cycles one fragment costs on a given Mali GPU,
  without running anything. Each material's cycle count has a limit.
- **Fragments times cycles**, summed over materials, is the frame's estimated
  GPU cost. It is the nearest equivalent of an instruction count.
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

- **Recorded games**, the same recordings the interface checks play.
- **Stress boards**: positions built by hand at the engine's capacity, where a
  frame is most expensive. The engine keeps a game in a compact form that holds
  255 objects, and moves a game that outgrows it to a larger form that holds
  1,024. The stress boards are:
  - `stress-255`, a battlefield of 255 objects;
  - `stress-1024`, a battlefield of 1,024 objects;
  - a 60-card hand, fanned, with the pointer moving across it;
  - every verb playing at once on 100 objects;
  - a stack of 50 spells and abilities with arrows to their targets.

The limits live in one file beside the engine's:

```toml
[frame."stress-255"]          # 1,200 frames
worst_frame_instructions = 2_000_000
total_instructions = 1_450_000_000
frame_allocations = 0
tolerance_percent = 1.0       # see below

[frame."stress-1024"]
worst_frame_instructions = 4_000_000
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
load average of 48 to 50 from other jobs. They come from a throwaway prototype,
not from Beryl 3D, and are replaced by the real client's numbers when it exists.

The prototype is a frame's inner loop with no GPU: for 600 cards, advance one
eased animation each, build a transform, project four corners through a camera
matrix, write a 64-byte draw record, and sort the records by depth.

| Measure                                  | Result                 |
| ---------------------------------------- | ---------------------- |
| Time per frame, median of five runs      | 54 microseconds        |
| Time per frame, best and worst run       | 26 and 63 microseconds |
| Heap allocations per frame after frame 1 | 0                      |

At 54 microseconds, 600 cards use 0.3% of a 60 Hz frame on this machine. The
spread between runs is the machine's load, which is why the gates count
instructions instead of timing.

## Sound

Beryl 3D plays the same sounds as the web interface, from the same list:
one-shot sounds tied to script steps and looping ambience.

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

- **The session and its protocol**, unchanged.
- **`data/ui-conventions.json`**: verbs, marks, timing values and the numbered
  exceptions.
- **Design tokens**: one data file generating CSS variables for the web
  interface and constants for Beryl 3D.
- **Fixtures, card specs and the list of sounds.**
- **The image download queue and disk cache**, moved out of the Tauri shell into
  `beryl_assets`.
- **Bug report storage, profiles and settings**, moved out of the Tauri shell
  into a crate both shells use.

What they do not share is placement. The web interface draws the table on a 1600
by 900 surface and imitates perspective by shrinking and tilting each card.
Beryl 3D's table is a real scene. Each is fitted to Arena separately.

### The parity list

The **parity list** is a generated report of everything the web interface can
show, with each item's state in Beryl 3D. A command prints it, and a check keeps
it current. An example of its output, with invented numbers:

```text
fixtures      118 / 134 play with no fault
card specs    1,388 / 1,402 moments play with no fault, both seats
verbs         16 / 16
marks         6 / 6
prompt shapes 10 / 10
screens       9 / 11   missing: testing screen, manual table lobby
conventions   74 / 74 entries confirmed
launch tests  macOS pass, Windows pass, iOS pass, Android pass
```

- **A fixture, spec moment, verb, mark or prompt shape is done** when Beryl 3D
  plays it from both seats, in both layouts, across the window matrix, with
  every interface check passing.
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

Until then, both clients' checks run.

### Platforms and versions

Beryl 3D runs on the platforms the web interface does, with a lower floor on
iOS.

| Platform | Minimum                              | Graphics                         |
| -------- | ------------------------------------ | -------------------------------- |
| macOS    | Apple silicon, as today              | Metal                            |
| Windows  | Windows 10                           | DirectX 12, else OpenGL 3.3      |
| iOS      | iOS 15, which reaches the iPhone 6s  | Metal                            |
| Android  | API level 24 (Android 7.0), as today | Vulkan, else OpenGL ES 3.0       |
| Linux    | the `beryl-bench` virtual machine    | software Vulkan, for counts only |

- **Beryl 3D's iOS host targets iOS 15**, the last version the floor runs. The
  Tauri app keeps its target of iOS 17 while it exists.
- **`wgpu` releases break its API about every three months.** Only
  `beryl_render` names a `wgpu` type, so an upgrade touches one crate.
- **Dependencies are pinned**, and an upgrade is a change of its own.

## Alternatives considered

Each of these was a real option in the design interview. They are recorded so
the reasons are not rediscovered.

- **Bevy.** A full engine with shadows, lighting, bloom and audio ready-made. It
  is an entity-component engine that owns the app's main loop through `winit`,
  which conflicts with decisions C3, C7 and C12. Every frame, even one that only
  animates, would run Bevy's scheduler and its copy of the scene into the
  renderer, third-party code that cannot be held to a counted, allocation-free
  budget. It releases a breaking version every three to four months.
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

1. Run `just stage verbs-zones --seat you --at 1500ms --tree` and note the
   position of one card that is mid-flight.
2. Run the same command with `--png` and open the picture. The card is where the
   tree said, mid-air, with its shadow on the table beneath it.
3. Start the desktop app with `--fixture verbs-zones --pause-at 1500ms`. The
   window shows the same picture as the PNG.
4. Run `just stage --live --tree` against the app. The tree matches step 1.

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
   the app with `--delay-frames 20`. Within a second or two the log records a
   step down to Baseline, and no object moves when it happens.
3. Choose Enhanced and repeat. The tier stays Enhanced.
4. Run `just stage stress-255 --png` and `just stage stress-1024 --png`. Both
   boards are readable: rows have overlapped and identical permanents have piled
   up with counts.
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
   `just stage pay-default --check`. The fault names both buttons, the window
   size and the moment, and its reproduce command shows the overlap.
2. Make one verb's implementation 50 milliseconds longer than its data. The verb
   values check fails and names the verb.
3. Add an allocation to `frame`. The allocation gate fails and names the
   fixture.
4. With the app running, use `just stage --live --press` to play a land. The app
   plays it as if clicked.

### Checking the parity list

1. Run the parity list and pick five items it marks done, including one card
   spec and one manual-table screen.
2. Open each in both clients side by side. Beryl 3D shows everything the web
   interface shows, in the same order.
