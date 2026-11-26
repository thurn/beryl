# Beryl 3D

Beryl 3D is Beryl's native Rust client on macOS, Windows, iOS and Android. It
draws the game with a GPU and uses Beryl's session and rules engine.

[Beryl 3D architecture](beryl-3d-architecture.md) describes the client core's
crate boundaries, scene tree, layout profiles, arrangement, features and checks.
This document describes what the client draws, the frame it hands to the
renderer and how the hosts run it. Start with the
[workspace guide](../client/AGENTS.md) for build and test commands.

The requirements, in priority order:

1. Smooth animation and low memory use on the target floor, including crowded
   boards and play against a searching AI.
2. Terminal tools for agents to drive, inspect and check the client.
3. Deterministic measurements of the work each frame takes.
4. Fast, deterministic interface checks of layout and motion.
5. Native iOS and Android apps.
6. Layout and text APIs that work across window sizes.
7. Spatial motion, shadows and lighting within the frame budget.

The performance floor is a 2015-class device, such as an iPhone 6s or a
mid-range Android phone with OpenGL ES 3.0. The floor is a design target.
[Performance gates](#performance-gates) use work counts as proxies for device
performance.

## Decisions

- **C1. The session supplies script timing.** Every host plays the timing in the
  script.
- **C2. The performance floor is 2015-class hardware.**
- **C3. Screens use a retained tree of named elements with flexbox and grid.**
  A10 extends the tree to the table and adds relations between its nodes.
  [The scene](beryl-3d-architecture.md#the-scene-one-tree) describes the shared
  contract.
- **C4. The table is a 3D scene with a fixed camera orientation per profile.**
  Prompts, menus and text can be drawn flat over the scene.
- **C5. Interface checks read frame records.** The app and picture tools use the
  same renderer.
- **C6. A card face is composed into one texture per size.** Board-size faces
  serve the table, and a bounded cache holds the full-size faces being read.
- **C7. The desktop uses `winit`.** Swift and Kotlin hosts own phone input,
  display pacing and platform services.
- **C8. Baseline and Enhanced differ in shading.** The tier can step down during
  play without changing layout or motion.
- **C9. Search can use every core.** The frame thread has higher scheduling
  priority where the platform permits it.
- **C10. The gate holds the worst single frame and the total.** A5 amends the
  gate to use per-board budgets and record a trend.
  [Checks](beryl-3d-architecture.md#checks) describes both.
- **C12. Beryl owns its presentation code over `wgpu`, `taffy` and `parley`.**
  This keeps the work of an animation frame under Beryl's control.
- **C13. GPU gates count work.** When actual device performance disagrees with a
  proxy, the device result governs.
- **C14. Run the interface checks before landing.** A6 amends which cases run
  there. The landing suite uses distinct recordings and authored scenes.
  [Checks](beryl-3d-architecture.md#checks) describes corpus selection.
- **C15. Prefer settings that sustain smooth animation.** On macOS the app's
  fullscreen mode is a composed window over the display.
- **C16. Agents can measure frame delivery with scripted input, controlled load
  and frame logs.**
- **C17. Beryl 3D has its own Cargo workspace and landing checks.**
- **C18. Prefer a steady frame cadence.** Ask for the highest supported refresh
  rate, then reduce shading or presentation rate when needed.

## The frame

`Client::handle` consumes input, `Client::prepare` prepares work for the frame,
and `Client::frame` fills a reusable `kit_frame::Frame` at the time the host
supplies. The frame has two main outputs.

- **Resolved elements** give each named thing its stable ID, parent, role, state
  and projected geometry in logical points. The tree printout, locators, hit
  testing and interface checks use these records.
- **Draw records** give the renderer transforms, materials, textures and
  per-instance parameters in paint order. Each `Draw` is an 80-byte record laid
  out as the renderer's instance buffer expects. Clip rectangles and paints that
  need more space live in separate frame tables. The renderer batches adjacent
  records with compatible state without changing their order.

The client core owns what is visible and where it goes. The renderer owns how to
shade it. The frame also carries sound cues, text line boxes, work counters and
draw load. The counters report preparation work as well as the work of drawing
each tick.

**Opaque first, nearest first.** The table and opaque cards can write depth
before distant fragments are shaded. Translucent records follow in ordered
bands. A card's marks are depth-tested with the table so a closer card can hide
them. The flat layer is drawn over the table using its own clipping and paint
order. A shake effect offsets the rendered table without changing hit geometry
or moving the flat layer.

### Drawing only when something changes

`Client::wake` reports that work is due now, at a future deadline, or on the
next input. Hosts use this to avoid rendering a static table. An input, session
update or asset completion can wake the host. Hosts drain the client's requests
after preparing and drawing the frame.

### FPS monitor

Settings can turn on a panel at the top left. It updates about twice a second.
The large number counts frames submitted in the last sample. The one-minute low
shows the rate implied by the longest gap between submissions, so a single
stutter remains visible after FPS recovers. The readout sits beside the desktop
log button and reserves a single row above the table in compact windows. With
the monitor enabled, the host also draws when the scene is still. Turning it off
restores on-demand drawing.

The target is the rate Beryl requests. On macOS, the link reading is the cadence
Core Animation reports, which can change independently of the request. The
current FPS is colored relative to the lower of those rates when both are
available. The link reading describes callback timing. FPS describes frame
submissions.

## Determinism

The same initial state, timed input stream and assets produce the same frame
records. The client core reads no clock, file, thread or system font. The host
supplies time, input and asset results. Runtime tuning is loaded outside the
core and passed in as data.
[Data and reload](beryl-3d-architecture.md#data-and-reload) describes that
boundary.

The core samples tracks at the requested time. A frame that only animates reuses
its storage. The preparation path can do work such as compiling beats, placing
objects, shaping text or laying out a changed feature. Fixed ordering and seeded
particle motion make the output reproducible. Playback tests own the clock and
can stop between any two inputs.

A view that arrives during a script queues behind it. If the window changes
during playback, the client recomputes the resting layout and preserves
continuity into the new poses.

### Motion the pointer drives

A hand card can lift toward the pointer, but the hit test still uses its place
in the fan so it does not lose the hover that raised it. A held card follows the
pointer, with a lean driven by the gesture. A release resolves the gesture
through the same client routing as other inputs. The touch hosts track one
active finger at a time.

## Hosts

`beryl_host` supplies window, input, asset and timing support. The `beryl_3d`
app connects it to the session and platform services.

- **Desktop.** `winit` translates window events. On macOS a display link
  schedules drawing, and the app's fullscreen mode uses a composed window that
  covers the display. Display changes update the surface and the client's
  logical size and scale.
- **iOS.** The Swift host owns its Metal view, `CADisplayLink`, touches and
  native platform requests.
- **Android.** The Kotlin host owns the `SurfaceView`, `Choreographer`, touches
  and activity lifecycle. It stops frame callbacks on pause and reconnects the
  surface when Android recreates it.
- **Windowless.** Text inspection can drive the client without a GPU. Offscreen
  capture uses the renderer and the asset path used by the app.

`just dev`, `just bundle` and `just smoke` build the desktop client. The bundle
includes the icon and audio on macOS and an `audio/` directory beside the
executable on Windows. The macOS icon lives under `beryl_3d/macos/`.

The phone projects live under `beryl_3d/ios/` and `beryl_3d/android/`. Both use
the app identifier `ca.thurn.beryl3d`. The Android project includes the Gradle
wrapper. Use `just ios` and `just android` to launch in a simulator or emulator.
`just b3d ios-build` and `just b3d android-build` build packages without
launching them. For Android builds on Windows, set `JAVA_HOME` to JDK 21 and
install the Android command-line tools under
`%LOCALAPPDATA%\Android\sdk\cmdline-tools\latest` or the equivalent under
`ANDROID_HOME` before running `just mobile-setup`.

The phone hosts call the Rust app through the C ABI in `beryl_3d/src/ffi`. The
boundary carries surface state, logical size and safe insets, time, gestures and
lifecycle events. The hosts poll for requests such as text input, haptics,
opening links and sharing files. A phone uses a native text field over the
position of the focused element so the platform can supply the keyboard and text
editing.

Frame pacing compares presentation gaps with the rate the host asked for
(`client/crates/beryl_host/src/rate.rs`). A late frame counts toward a step down
only when drawing explains it: the frame before it spent more than half the
interval between the start of its tick and the return of its present call,
including the wait for a drawable. A late tick after a short frame never steps
down, because a lower rate does not help it. When 60 of the last 240 animation
frames were explained misses, Automatic graphics mode first switches to
Baseline, and then the rate steps down to the next divisor of the display rate.
The rate steps back up after a run of 600 frames in which at most 30 were late
or had too much work for the higher rate. A step down within 600 frames of a
step up doubles the run the next step up needs. The thresholds are in
`data/client/frame-rate.json`. The frame log records the selected rate and tier
alongside the presentation gaps.

**Low memory.** The client drops the full-size face cache and the renderer
releases its full-size texture array. Board-size faces can still be drawn, and
the array is recreated when a full-size face is uploaded again.

## The table

### Space and camera

The table is the plane `z = 0`. Its coordinates are in millimetres, with `x` to
the viewer's right, `y` away from the viewer and `z` above the table. A card's
geometry is sized like a physical card. Cards have real heights: the stack of
cards in a pile rises by one card thickness per card, and cards in flight rise
in front of what they pass.

A profile supplies the camera fit and the part of the table that must stay in
view. The orientation is fixed for the profile. The desktop camera is fitted to
Arena recordings. [Layout profiles](beryl-3d-architecture.md#layout-profiles)
describes how the profile supplies the table regions and camera.

### Objects and placement

[Arrangement](beryl-3d-architecture.md#arrangement) describes the containers and
shared solver. These are the visible conventions the arrangement serves.

- The battlefield has rows for creatures, lands and other permanents. A tapped
  card turns in the table plane. Plain identical permanents can share a pile,
  with the lowest slot on top so choosing one removes the card the player sees.
  A permanent with attachments, counters or other visible differences stays
  separate when stacking would hide information.
- An Aura or Equipment peeks from the right of the permanent it is attached to.
  Cards held in exile and the other cards of a merged permanent peek from the
  left. A phased-out permanent stays in place and is faded.
- A hand fans or arranges its cards in compact rows. Hidden cards show their
  backs. Reading a card lifts or shows its face without removing it from its
  zone in the session.
- A library, graveyard or exile pile shows its top and the thickness of the
  cards beneath it. Pressing a pile that offers inspection opens its viewer.
- The stack arranges spells and drawn ability faces together. A selection tray
  holds cards a prompt offers from another zone, grouped when the prompt names
  groups. Cards the opponent is looking at from their library lie face down
  beside it.
- The revealed-card pile retains faces from a reveal for later inspection. A
  revealed face in that pile has its own element ID.
- Marks sit on or near their subjects. Prompt marks show which cards can be
  picked, which are selected and what a spell or ability targets. The mark is
  derived from the view or script that owns it.

### Verbs

A verb is one kind of script step, such as `fly`, `vanish` or `point`.
`game_play` compiles each step into tracks or other timed work. A single step
can move several objects. Arrows are drawn from their subjects' current
positions, so they follow moving cards. When movement ends, an object rests at
the pose the arrangement gives it. A notice can outlive the beat that started
it.

### Timing a script

A `PlayerView` carries the script that transforms the previous view into that
view. A script contains ordered beats. Each beat can run several steps together,
subject to their delays, waits and dependencies. The session uses
`data/ui-conventions.json` to supply each step's timing and effect cues. Both
clients lay out the beat from the same timing data. They can play faster to
catch up, and reduced motion collapses motion durations.

Prompt state lives in the session. The client sends commands and uses the
returned view and marks. It can make local motion for gestures or prompt
presentation without inferring what happened in the game.
[Engine architecture](engine-architecture.md) describes the protocol.

## The flat layer

### Elements

An element has a role, a name when a player needs to identify it, and style and
behavior values. Interactive elements have stable locators. The layer retains
element identity across updates and lays out flexbox and grid with `taffy`. An
element can show card art or attach a view of a scene object.
[Features](beryl-3d-architecture.md#features) describes how a view supplies
elements and how actions route back to it.

### Widgets

The reusable widgets in `kit_ui` give buttons, lists, menus and tooltips their
roles and interaction behavior. A context menu can open near the point that
asked for it. A tooltip is anchored to the item it describes. See
[Toolkit capabilities](beryl-3d-architecture.md#toolkit-capabilities) for the
shared style and input vocabulary.

### Floats

The layer can lay out an overlay once and change its opacity or offset as time
advances. This lets a life change, mana payment or temporary notice move without
rebuilding text on every tick. The feature that presents it owns its lifetime,
and any anchor to the table uses the scene's relation contract.

### Sizes

The host supplies scale and safe-area insets. Layout uses logical points. Text
and icons can be rasterized at physical pixel size.

In the compact portrait game screen, each player's life total sits by their side
of the table. The menu opens settings and gives access to the log and bug
report. The phase indicator sits in the middle band. The primary action sits in
the dock by the viewer's hand, with other actions above or beside it. The prompt
text has its own slot. The dock and life total can move clear of an expanded
hand, and the active prompt can use free space in the middle band. Start,
testing and lobby screens use a scrolling column when their controls need more
height than the safe area provides.

### Text

`kit_text` shapes and breaks text with `parley` and the bundled fonts. Rules
text uses styled runs and inline boxes for mana symbols, which wrap with
surrounding words. Shaped results can be reused across frames. Text that is part
of a card face is composed into that face's texture.

`beryl_assets` rasterizes glyphs for an atlas at the requested pixel scale.
While a worker prepares another scale, an available glyph can be sampled at the
new size. Its rasterization does not change the layout. A character missing from
the bundled fonts uses the missing-character glyph.

## Rendering

`beryl_render` consumes the frame and owns the `wgpu` pipelines, device and
surfaces. Both visual tiers use the same projected positions, sizes and timing.
Tier-specific records add shading work at those positions.

### The Baseline tier

Baseline uses materials that can run within the downlevel device limits. Depth
writes let hidden pixels of opaque table geometry and cards be rejected before
shading. A card face is a sample from a prepared texture. Every card's outline
is softened by a strip one pixel wide that fades to nothing, so a tilted card
has no stair-stepped edge. An opaque card draws the strip as a second record
among the blended ones, and a blended card draws it with its own record. Both
tiers draw the strip, since the Enhanced tier draws the flat layer with one
sample. A lifted card can cast a soft contact shadow, drawn as a quad on the
table or just above cards beneath it. The shadow spreads and fades with height.
Baseline keeps these shadows and the effect sprites in the main draw list.

### Effects

An effect is timed shading, such as a halo, flash, dissolve, strike or life
pulse. The script timing names the effect and its subject. The client can keep
an effect playing after the script ends. A later run of the same effect on the
same subject replaces it. Both tiers play the cue, and Enhanced can add
particles or bloom. The [effect definitions](beryl-3d-architecture.md#effects)
describe the data used to draw each cue.

A dissolving card is blended, and its shader computes the breaking edge. `shake`
offsets the table's rendered picture, as described in [The frame](#the-frame).

### The Enhanced tier

Enhanced adds a depth shadow map, lighting and sheen on the tabletop and cards,
multisampling, bloom, and seeded particles for effects that specify them. It
uses the same light direction as the contact shadows so changing tier keeps
shadows on the same side. A frame can omit bloom work when it has nothing to
bloom. Baseline draws arrows as atlas ribbons. Enhanced follows the same paths
with procedural ribbons and spearheads. The tier checks compare resolved
geometry, including the arrow paths, and draw records shared by both tiers.

### Choosing a tier

Automatic graphics mode starts with a short offscreen Enhanced probe on a
crowded board at the window's size. The median measured draw has to fit within
the headroom set by the startup tuning. The median decides, not the slowest
draw, because each draw is timed on the wall clock: on a loaded machine one slow
draw in 30 reflects the load, not the GPU, and a wrong choice of Enhanced
corrects itself when the rate rule drops the tier. The warmup draw is excluded
because it creates resources and first uses the pipelines. The result is logged
with the median and the slowest draw. Repeated late animation frames step down
to Baseline for the rest of the run. Choosing Baseline or Enhanced explicitly
fixes the tier.

## Card faces

A prepared face is one composited texture. Using one sample for a face keeps the
art, frame, title and rules text from becoming separate blend passes on every
animation frame. `game_table` describes the card and ability faces the session
needs. The host supplies those recipes to the asset service.

### Sizes and memory

Every prepared board face is 256 × 360 texels. A face prepared for reading is
672 × 936. The renderer keeps separate arrays for the two sizes and limits how
many full-size faces stay resident. The card back is available alongside the
faces. Under memory pressure a card can still draw with its board-size texture.

The renderer and asset service agree on the supported texture format. The
preference is ASTC, ETC2, then BC7, with uncompressed RGBA as the fallback. The
selected format affects the texture cache key.

### Preparation

`beryl_assets` loads a scan from the shared art cache or composes a face with
Beryl's card frame, then compresses the result for upload. The fallback face
makes the card readable while its scan is unavailable. Workers handle decoding,
composition, compression and glyph rasterization. The frame thread receives
upload data and ready notifications. A full-size face is requested when the
player reads the card.

The texture encoders used by `ctt` include C++ code. The Android app bundles the
NDK's shared C++ runtime beside the Rust library. The Windows build needs the
MSVC tools for the same encoders.

## Threads

The host's frame thread owns the client and renderer. It drains queued inputs,
prepares and presents frames, then routes requests. A live game runs on the
session thread. A click can enqueue a session command, and the reply wakes the
host when it is ready.

Search threads can use every core at default priority, with the frame thread at
a higher priority where supported. Asset, audio and frame-log work use separate
threads so their I/O and decoding stay off the presentation path.
`beryl_host::threads` maps the scheduling classes to the platform's APIs.

No core is reserved. On a two-core floor device, reserving one would leave only
one core for search even while the table is idle.

## The agent interface

- All commands start with `just b3d`. Commands are words such as `tree` and
  `check`. `list`, `info`, `map` and command help expose the valid names and
  inputs.
- Reproduction commands can be pasted back into a shell. Frame, seat and window
  information travel with a fault. Output is bounded by default and gives a way
  to inspect the rest.
- Inspection, capture and replay share one notation for times, moments, locators
  and actions. Keep concurrent runs' output separate.
- Tests and previews own their clocks. They can act at the current frame or
  advance to a named moment without waiting for wall time.

The [b3d skill](../.agents/skills/b3d/SKILL.md) has command recipes.
[The architecture's agent interface](beryl-3d-architecture.md#the-agent-interface)
describes the generated map, explanations, scaffolds and stories.

### Edit-to-output time

The inspector links the protocol and client core. The recorder is the separate
program that links the game engine and prepares the recordings pack. The tools
reuse the shared test-profile build. The Python launcher can run the linked
binary in Cargo's `deps` directory, so a no-op build does not make macOS check a
newly copied executable.

The workspace uses unpacked line-table debug information for local dev builds.
In the 2026-11-15 comparison that informed the setting, it lowered the median
for an edit followed by a tree printout from 11.9 s to 8.5 s on the development
Mac. Those figures describe that build, before the current crate division.

### Times, moments, locators and actions

Use durations such as `850ms`, or moments from `info` such as `end` and
`prompt:2`. A locator can be copied from a tree line, for example
`button "Auto Pay"`, with a parent or ID to distinguish duplicates. A drive
action such as `press button "Auto Pay"` uses the same locator. Failed locators
report what the frame contained.

### What is printed

Tree lines report role, name, ID, place when present, projected bounds and
state. A moving card can report its verb and progress. `--within` narrows the
output to a place or element, and `--full` expands abbreviated groups.

### The test driver

The replay driver in `app_client` accepts recorded protocol views and runs input
through the normal client path. It supplies the recording's replies unless the
test drives the choice. A test advances the clock explicitly before inspecting
or asserting on a new frame. It can exercise a real client interaction without a
renderer or a running session.

### Driving the running app

Debug app runs can take a drive script. Its time is relative to the run, and its
actions enter through the host's input path. The live tools can inspect or send
actions to a debug app that exposes its local socket. Use the frame log to
examine pacing and render work at the moment of interest. A windowless replay is
suitable for inspecting the client. Actual presentation timing requires a host
with a display or mobile surface.

## Interface checks

`just b3d map checks` lists the rules the runner applies to frames.
[Checks](beryl-3d-architecture.md#checks) describes their coverage. Use a
recording or story that exercises a defect so the generic checks can catch it
again. Reachability is checked in projected space because a card can be visible
yet give the player no uncovered area in which to press it. Motion checks relate
a changing pose to the step that owns it and to the pose where it should come to
rest.

### What the checks run over

The runner plays recordings and standalone stories across the window matrix. A
check can declare its applicable inputs. A stress scene, for example, can supply
the crowded state a recorded game cannot reliably reach.

## Test speed

Replaying views with a stub atlas and a controlled clock keeps the renderer,
asset work and rules engine out of the interface test loop. The
[corpus and tip sweep](beryl-3d-architecture.md#checks) decide which scenes run
before and after landing.

### Recordings

The recorder builds a shared pack of named views, scripts and replies.
`data/beryl3d/fixtures.json` defines the named fixtures. Card specs supply
moments that need a real session. Authored stories supply states directly,
including the stress boards. See
[Scenes](beryl-3d-architecture.md#scenes-recordings-and-stories) for how to
choose between them.

## Performance gates

Per-board budgets and the verified-tip trend are described in the
[architecture checks](beryl-3d-architecture.md#checks). The gate covers ordinary
play and stress boards. A board with a prompt can include the preparation cost
of a newly laid-out screen. Crowded rows, hands and the stack exercise work that
an ordinary recording may never reach.

### What is counted

- **Native work.** The replay reports work such as elements visited, segments
  evaluated, records written, layout nodes, text runs, placed objects and
  compiled steps. It also reports draw load and allocations. A counting
  allocator requires zero allocations inside `Client::frame` and accounts for
  allocations in `handle` separately.
- **Instructions.** Callgrind collects instructions in the Linux `beryl3d-bench`
  guest under Lima on macOS and WSL on Windows. Each measurement covers `frame`
  and the `handle` calls since the previous frame. The separate guest keeps
  graphics-driver setup and measurement apart from the engine's count gate.
- **GPU work.** The software Vulkan renderer counts fragment shader invocations
  over the passes of a frame. The count excludes fragments rejected before
  shading. The shader-cost check independently limits expressions and texture
  samples by material. Draw calls, uploads and blended area expose work that
  fragment counts alone cannot explain. The crowded board and simultaneous verb
  replays hold separate screen-area budgets for contact shadows, other blended
  scene draws, and blended overlay draws. The total blended area is still
  reported for comparison across revisions.

### Finding the worst frame

The native pass replays each board and ranks frames by the work counters. The
GPU check renders the highest-ranked frames in both tiers. The Callgrind run
collects a separate dump for each frame in one replay, then reports the largest
count and the total. It also checks that the instruction peak is among the
native candidates. A failure to rank that peak means the cheap counters need to
account for the missing work. `just client gate --json` reports measured work
alongside any budget failures.

### Frame calibrations

`data/client/frame-calibration.json` records the reference revision, fixture,
and runtime for each registered Ignite verifier. Native checks use the work
budgets in `budgets.json` with any calibrated per-board overrides. Instruction
checks use the verifier's measured per-board budgets and scaling limits.
`budgets.json` still records the original limits. Ignite reports a missing or
incompatible profile as incomplete.

Use `just client vm` to provision the guest. To enroll a verifier or rebase a
profile, use an Ignite lease at an accepted revision. Run the native `gate`
binary and measure the same board list twice through `counts.measure` in
`tools/src/beryl_tools/client/counts.py`. It reports the guest runtime and the
instruction count of every frame. Confirm the counts repeat and that a lowered
limit fails. Record the revision and runtimes under the verifier ID, and keep
the zero-allocation and native rank assertions.

### Measurements

- **Mac fullscreen mode, 2026-11-15.** On the development Mac at 120 Hz under
  load, fullscreen Space runs stepped down to 30 Hz and still saw late frames.
  The settled composed fullscreen runs held 120 Hz, with one and three late
  frames respectively. This is why macOS fullscreen uses a composed window
  (C15).

- **Opaque depth order, 2026-11-15.** On the llvmpipe Vulkan driver, two
  overlapping opaque quads shaded 20,480 fragments far first and 16,384 near
  first. The same result held under downlevel limits. This is why the opaque
  pass draws nearer geometry first and why the fragment gate uses the driver's
  count.

- **Rate rule on the development Mac, 2026-11-25.** The frame logs of 24 play
  sessions on a 120 Hz ProMotion display held 1,110 late frames. On late frames
  the client's work and encoding took 0.33 ms at the median. The old rule
  stepped down when 3 of 60 frames were late for any reason, so 16 sessions
  ended at 60 Hz or lower and 7 reached 30 Hz. The lower rates did not stop the
  late frames: one session had 0.03% of frames late at 120 Hz and 1.9% at 30 Hz.
  In 4 seconds at 120 Hz no session had more than 20 late frames, except one
  2-second stretch that the display held to 60 Hz with 94, each waiting for a
  drawable. That is why a step down needs 60 explained misses in 240 frames, and
  why a step up allows 30 late frames in 600 (5%, above the 1.9% seen with
  nothing wrong in the client).

- **Late ticks on the development Mac, 2026-11-25.** Each run played
  `just client app --agent expert --tier baseline --window fullscreen` to the
  end of its game (25 to 33 s) at a load average of 16 to 28. Every run drew on
  every tick, because the saved FPS monitor setting was on. Five runs at 120 Hz
  drew 15,410 frames. After the first three seconds of each run, 36 were late.
  The causes that `just client frames` names were `handler` 26 (72%),
  `link skipped` 8, `callback late` 1 and `handling only` 1. In each `handler`
  frame the frame before it had stalled in one stage: 15 times in the present
  call, for 12.2 to 30.1 ms (median 0.02 ms), and 11 times in encoding and
  submission, for 9.7 to 16.1 ms (median 0.25 ms). A 1 ms `sample` profile of a
  sixth run showed what the stalls are. Of the 568 samples in the present call,
  465 were waiting for wgpu's lock to be released by its readers, inside
  wgpu-core's `Queue::present`. A further 62 samples waited for the same lock in
  wgpu-hal's Metal `Queue::submit`. The only other thread that uses the device
  is `scene GPU timing` (`client/crates/beryl_render/src/gpu_timing.rs`). It
  calls `device.poll` every millisecond and runs in the utility class, below the
  AI's search threads, so the AI can preempt it while it holds the lock. In two
  runs with that thread in the interactive class, `handler` late frames after
  startup fell from 1.7 to 0.9 per 1,000 frames. In five background runs
  (`--window background --for 120s`) at a load average of 40 to 49 before the
  change, 16,216 frames had 51 late frames after the first three seconds
  (`handler` 35, `link skipped` 6, `handling only` 5, `drawable` 4,
  `callback late` 1; 2.16 `handler` late frames per 1,000 frames). Moving
  `scene GPU timing` to the interactive class and sleeping until 4 ms after
  `submit_ns` before the first poll reduced this across five background runs at
  a load average of 33 to 58 (15,646 frames) to 2 late frames after the first
  three seconds (`handler` 1, `callback late` 1; 0.06 `handler` late frames per
  1,000 frames).

- **A fixed 60 Hz on the development Mac, 2026-11-25.** Three pairs of runs, as
  above, asked the display link for a fixed 120 Hz and then a fixed 60 Hz
  (`--fixed-hz`). Late frames for each 1,000 frames:

  | Pair | Load at 120 Hz | 120 Hz             | Load at 60 Hz | 60 Hz             |
  | ---- | -------------- | ------------------ | ------------- | ----------------- |
  | 1    | 19.1           | 7.46 (26 of 3,484) | 24.5          | 1.06 (2 of 1,882) |
  | 2    | 28.2           | 6.18 (22 of 3,560) | 23.5          | 0.00 (0 of 1,432) |
  | 3    | 22.3           | 9.23 (33 of 3,575) | 27.9          | 1.54 (3 of 1,945) |

  After the first three seconds, the 120 Hz runs had 54 `handler`, 8
  `link skipped`, 3 `handling only` and 1 `callback late`, and the 60 Hz runs
  had 5 `handler`. A link asked for a lower rate does not add late frames by
  itself, so the macOS host keeps asking the link for the rate it chose. The
  1.9% of late frames at 30 Hz in the earlier sessions did not recur.

- **Fullscreen Space against the composed window, 2026-11-25.** Six pairs of
  runs played `just client app --agent expert --tier baseline` to the end of its
  game (23 to 46 s), once in the composed fullscreen window and once in a
  fullscreen Space (winit's `Fullscreen::Borderless`), back to back. Pairs 4 to
  6 added `--load 16:default`. Other work kept the load average at 30 to 59 in
  every run except the first. Late frames at 120 Hz for each minute after the
  first three seconds, and the late frames waiting for a drawable in the first
  three seconds:

  | Pair | Load, composed | Composed    | Startup | Load, Space | Space       | Startup |
  | ---- | -------------- | ----------- | ------- | ----------- | ----------- | ------- |
  | 1    | 15.5           | 36.7 (12)   | 15      | 42.8        | 212.8 (131) | 25      |
  | 2    | 46.8           | 130.1 (65)  | 11      | 43.8        | 174.1 (103) | 4       |
  | 3    | 40.2           | 115.3 (75)  | 12      | 37.0        | 252.3 (144) | 0       |
  | 4    | 30.3           | 154.8 (85)  | 0       | 33.3        | 337.2 (244) | 28      |
  | 5    | 51.4           | 161.9 (111) | 11      | 42.3        | 193.1 (117) | 33      |
  | 6    | 46.3           | 145.4 (77)  | 14      | 59.2        | 243.8 (137) | 24      |

  The Space had more late frames than the composed window in all six pairs. The
  rule was to switch only if the Space had at most half the composed window's
  count in five pairs, so macOS fullscreen stays a composed window (C15) and the
  Space mode was removed. After startup the composed runs had 243 `handler`, 171
  `link skipped`, 7 `handling only`, 2 `callback late` and 2 `drawable` late
  frames. The Space runs had 104 `drawable` in one run, and `link skipped` was
  their largest cause. Neither mode was free of `drawable` late frames in the
  first three seconds in all of its runs. In 149 of the `handler` frames the
  stalled frame spent longest in render encoding and submission, and 46 of those
  had 2 or more buffer sets in flight, where `Slots::write`
  (`client/crates/beryl_render/src/slots.rs`) can wait for the GPU while the
  frame holds a drawable. A ticket under the "Hold 120 Hz" epic moves that write
  before the drawable is taken. These figures predate the change to the
  `scene GPU timing` thread in the entry on late ticks, whose lock waits also
  show up as `handler` frames.

## Checks and isolation

Ignite declares source, replay, interface, corpus and frame checks in
`ignite/checks/client.toml`. The shared script checks, windowless desktop
startup, iOS Simulator build and Android APK build are in
`ignite/checks/script.toml`. The gate uses automated checks.

## Sound

The hosts use the cues and loop definitions under `data/audio/`. `game_play`
lists each due cue in `Frame::sounds` on the script clock so a replay can check
a cue against its step. The host sends cues and settings to the audio service.
Effects can be decoded ahead of play. Music is loaded as needed and uses the
shared loop definitions. Windowless runs are silent.

The app uses `rodio` for decoding and mixing. Desktop and iOS use its playback
support. On Android the Kotlin host pulls samples into an `AudioTrack`. This
keeps the library loadable on Android API 24 and 25, where `cpal`'s AAudio
backend is unavailable.

## Bug reports

Cmd+B or Ctrl+B opens the report during a game. At the manual table, use
Cmd+Shift+B or Ctrl+Shift+B. The client freezes the table before raising the
sheet. The host reads back the renderer's frame and saves the resolved tree from
that moment. Closing the sheet resumes the table clock.

The native app uses the shared bug storage. A saved report can include the
picture, view and replay, `tree.txt`, and the frame log `frames.jsonl`. Its
`ui.json` identifies the client and, for a recording, the seat and time to seek
to. `just b3d bug <report>` prints the report and applicable reproduction
commands. A replay supplied with `just b3d app --replay` can resume at a saved
decision without putting the reproduction among the player's saved games.

The first report from a desktop launch with local filing configured creates a
parked Ignite epic. Each report from that launch files as a child of the same
epic. The report folder keeps the launch ID, so `just bugs file` can group
reports saved while Ignite was unavailable and reports pulled from phones.

`just mobile-bugs` imports from the native app's `Documents/bugs` on iOS and
`files/bugs` on Android. Pass `--device` to include a paired iPhone.

## Saved data and compatibility

### Desktop data

`beryl_shell` resolves the desktop data paths. The data folder holds replays,
the games ledger and the deck library.

| Platform | Data folder                                    | Log folder                           |
| -------- | ---------------------------------------------- | ------------------------------------ |
| macOS    | `~/Library/Application Support/ca.thurn.beryl` | `~/Library/Logs/ca.thurn.beryl`      |
| Windows  | `%APPDATA%\ca.thurn.beryl`                     | `%LOCALAPPDATA%\ca.thurn.beryl\logs` |

Set `BERYL_PROFILE` to give an instance its own data under
`<data folder>/profiles/<name>`, with `logs/` and `bugs/` inside the profile.
The default profile writes reports to `<data folder>/bugs`. `BERYL_BUGS`
overrides the report folder.

Beryl 3D keeps the existing desktop saves and profiles. Each phone package keeps
its data in its own application sandbox.

### Recording timelines

`just b3d timeline <recording>` prints when each script step begins and ends,
with its effect and sound cues. `just b3d motion-film <verb>` uses
`data/beryl3d/motion-recordings.json` to choose a replay, then films the verb
from both seats on desktop and phone alongside available Arena recordings.

### Platforms and versions

| Platform | Target                  | Graphics                            |
| -------- | ----------------------- | ----------------------------------- |
| macOS    | Apple silicon           | Metal                               |
| Windows  | Windows 10              | DirectX 12 or OpenGL                |
| iOS      | iOS 15                  | Metal                               |
| Android  | API 24                  | Vulkan or OpenGL ES                 |
| Linux    | Windowless benchmark VM | Software Vulkan for fragment counts |

The phone targets are set in the iOS project and Android Gradle configuration
under `rust/beryl3d/crates/beryl_3d/`. Library versions are recorded in the
workspace's Cargo manifests and lockfile. Keep `wgpu` types within the renderer
boundary so upgrades stay local.

## Out of scope

- Screen-reader support and complete keyboard-only play. Named elements still
  carry roles for inspection and input.
- Writing systems outside the bundled fonts.
- Maintaining a second renderer or checking exact images for every board.
- A physical floor-device test in the landing gate.

## Alternatives considered

- **A full engine such as Bevy.** Owning the loop and using `wgpu` directly keep
  the per-frame scheduler and scene transfer out of Beryl's frame path. In the
  2026-11-14 prototype, Bevy used about twenty times the CPU per frame for the
  comparison scene. Slint also had its own element system, which would require a
  second layout and inspection boundary beside the table.
- **Using the web table's 2D coordinates permanently.** A perspective camera
  puts the table, movement and shadows in the same space.
- **A CPU reference renderer.** It would require a second implementation of each
  shader and new reference images for intentional visual changes.
- **One windowing library on all platforms.** Native phone hosts let the
  platform own frame pacing, lifecycle and text entry.
- **A core reserved for rendering.** It would reduce search capacity even during
  a static frame. Scheduling priority lets the frame thread take time when it
  has work.
- **Live card-face layers or an all-blended Baseline pass.** Composed faces and
  an opaque depth pass reduce sampling and repeated shading on crowded boards.
- **A smaller GPU layer such as miniquad.** The client needs Vulkan and DirectX
  backends as well as support for the Baseline feature set.
