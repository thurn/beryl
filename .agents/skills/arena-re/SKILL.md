---
name: arena-re
description: >-
  Reverse-engineer how MTG Arena implements a visual effect, shader, animation,
  highlight, spline, or combat/UI indicator from the locally installed macOS
  MTGA client, and translate it clean-room into the client. Use when designing or
  matching complex Arena VFX, shaders, particle systems, card highlights,
  targeting lines, or multi-stage animations where video frames alone do not
  reveal the underlying rendering technique.
---

# Arena Reverse-Engineering Reference (Clean-Room VFX & Animation)

Use this skill alongside `arena-video` and `arena-pixel-match` when building
rich VFX, shaders, particle systems, highlights, or card-motion animations in
the client and gameplay video alone does not explain *how* MTG Arena renders or
evaluates them (blend modes, UV distortion math, color transfer curves, spline
interpolation, Hermite timing curves, state flags, or layer stacking).

## Clean-room rule (mandatory)

Beryl is a public repository. **Never** copy, commit, or redistribute any
proprietary asset or code from the MTG Arena installation:

- Never import, convert, or trace MTGA textures (`.png`, `.tga`), 3D meshes,
  prefabs, animation clips, fonts, or audio files.
- Never paste or mechanically port decompiled C# or extracted Metal Shading
  Language (MSL) code into Beryl.
- **Do** inspect MTGA strictly read-only to learn the *technique*: selection
  rules, spline/easing math, layer stack, blend mode, state transitions, UV
  scroll/polar coordinates, contrast/ramp math, and particle behavior. Then
  author original procedural WGSL, Rust geometry/splines, or code-generated
  sprites (`beryl_assets`) and tune visually against `arena-video` recordings.

## Where MTGA lives on macOS and how it is structured

This skill and its helper scripts read the macOS build only. The Windows build
is laid out differently (`GameAssembly.dll`, Direct3D shaders), and a Windows
machine without the Mac client can still film Arena with `arena-video`.

Default Steam installation on macOS:

- App bundle: `~/Library/Application Support/Steam/steamapps/common/MTGA/MTGA.app`
- Downloaded assets: `~/Library/Application Support/Steam/steamapps/common/MTGA/MTGA_Data/Downloads/`

Key facts about the macOS Unity build (Unity 6 / IL2CPP):

- **~85% of Arena's VFX and animations live in data-driven AssetBundles and
  Asset Lookup Trees (ALTs)**, not hardcoded in C#. C# (`GameAssembly.dylib`)
  mainly evaluates generic spline/easing engines, toggles animator states,
  assigns highlight types, and triggers prefab playback. Always inspect ALTs,
  the Manifest, Prefabs, Materials, and Shaders before touching IL2CPP
  disassembly.
- **Asset Lookup Trees (`MTGA_Data/Downloads/ALT/ALT_*.mtga`)**: plain-text JSON
  decision trees (`Root`, `Nodes`, `Connections`) that map game context
  (`FromZone`, `FromOwner`, `ToZone`, `ToOwner`, `GameState_Phase`,
  `CardData_FaceDown`, `ZoneTransferReason`) to specific spline, VFX, SFX, and
  stagger assets. These are **not** UnityFS bundles.
- **Manifest (`Manifest_*.mtga`)**: plain-text JSON indexing every downloaded
  `.mtga` bundle to its original Unity `Assets/...` source paths.
- **AssetBundles (`AssetBundle/*.mtga`)**: standard unencrypted `UnityFS` (v8,
  LZ4HC) archives containing one or more `SerializedFile` (v22) objects with
  embedded TypeTrees. Scene bundles like `Bucket_Scenes.ScenePayloads_*` hold
  many `SerializedFile` nodes (one per scene), not just `nodes[0]`.
- **Shaders (`Shader_*.mtga` and `globalgamemanagers.assets`)**: on macOS,
  Unity embeds LZ4-compressed **plain-text Metal Shading Language (MSL)**
  (`#include <metal_stdlib>`, `xlatMtlMain`) alongside serialized blend, depth,
  and stencil states.
- **C# metadata (`il2cpp_data/Metadata/global-metadata.dat`)**: unencrypted
  Unity IL2CPP v39 metadata (`0xFAB11BAF`) with unobfuscated C# namespaces,
  types, fields, methods, and metadata tokens (`0x06xxxxxx`).
- **Compiled C# (`MTGA.app/Contents/Frameworks/GameAssembly.dylib`)**: universal
  Mach-O (`0xcafebabe`) with an ARM64 slice using chained fixups (`ptr & ((1 <<
  36) - 1)`). `Il2CppCodeGenModule` tables map each method token to its exact
  ARM64 virtual address for `xcrun lldb -b` batch disassembly.
- **Live GPU capture**: `MTGA.app` is signed without Hardened Runtime
  (`codesign` flags `0x0`), so Xcode Metal Frame Capture can inspect live draw
  calls, bound uniforms, and pass order whenever static inspection is ambiguous.

## Helper script

Use the read-only, standard-library helper in this skill's directory:

```bash
SCRIPT=.agents/skills/arena-re/scripts/inspect_mtga.py
python3 "$SCRIPT" manifest "combat|highlight|intention|spline"
python3 "$SCRIPT" alt ALT_Card "LibraryToHand"
python3 "$SCRIPT" alt ALT_ZoneTransfer "Stagger"
python3 "$SCRIPT" prefab Bucket_Card.AttackerIcon_0
python3 "$SCRIPT" cabs Bucket_Card.AttackerIcon_0
python3 "$SCRIPT" materials Bucket_Card.AttackerIcon_0
python3 "$SCRIPT" shader Shader_ParticleUber --match "_DistTex.sample" --match "_MultTex.sample"
python3 "$SCRIPT" symbols "UpdateHalos|DrawCombatArrow|BattlefieldIcons"
python3 "$SCRIPT" types "SplineMovementData|MovementData|ZoneTransferGroup"
python3 "$SCRIPT" disasm "SplineData" "HermiteGetPoint|GetModifiedPositions"
```

Pass `--mtga <path>` if MTGA is installed outside the default Steam folder.

For numeric `MonoBehaviour` / `ScriptableObject` / `Transform` / `Camera` fields
(`AnimationCurve` Hermite keyframes, `SplineData` node vectors/Eulers, camera
FOV and root transform), run `UnityPy` on the fly with `uv` (always set
`FALLBACK_UNITY_VERSION` because MTGA strips the version header from bundles):

```bash
uv run --with UnityPy python -c '
import UnityPy
UnityPy.config.FALLBACK_UNITY_VERSION = "2022.3.0f1"
env = UnityPy.load(".../Bucket_Card.MovementPayload_Spline_0_....mtga")
for obj in env.objects:
    if obj.type.name == "MonoBehaviour":
        tree = obj.read_typetree()
        if "LibraryToHand" in tree.get("m_Name", ""):
            print(tree["m_Name"], tree.get("Speed"), tree.get("Easing"))
'
```

## Where Arena puts different kinds of VFX and animations

Search ALTs (`alt`) and the Manifest (`manifest <regex>`) first to map a feature
to its rules and bundles:

| Feature area | Typical ALT / Manifest bundle prefixes |
| --- | --- |
| Zone-transfer selection rules, phase splits & stagger | `ALT/ALT_Card_*.mtga` (`ALT_Card.MovementPayload_Spline`, `_NonStack_SFX`, `_VFX`), `ALT/ALT_ZoneTransfer_*.mtga` (`ZoneTransfer_GroupStagger`, `ZoneTransfer_HangTime`), `ALT/ALT_RegionTransfer_*.mtga` |
| Card movement splines & Hermite easing curves | `Bucket_Card.MovementPayload_Spline_*` (`SplineMovementData`: `Speed`, `Easing: AnimationCurve`, `Spline: SplineData`) |
| Card holders & zone anchors (hands, libraries) | `Bucket_Prefab.CardHolderBasePrefab_*` (`LocalHand_Desktop_16x9`, `OpponentHand_Desktop_16x9`, `LocalLibrary`, `OpponentLibrary`) |
| DuelScene camera & root transforms | `Bucket_Scenes.ScenePayloads_*` (`DuelScene.unity`, `TopDownCameraController`, main `Camera` FOV/position/rotation) |
| Card eligibility / hover / selection highlights | `Bucket_Card.Highlight_*`, `Bucket_Card.Parts.HighlightsPart_*` |
| Combat attacker & blocker indicators | `Bucket_Card.AttackerIcon_*`, `Bucket_Card.BlockerIcon_*` |
| Combat strikes, splines & hit impacts | `Bucket_Combat.Spline*`, `Bucket_Combat.VFXBirth_*`, `Bucket_Combat.VFXHit_*`, `Bucket_Combat.VFXSustain_*` |
| Targeting, blocking & intention arrows | `Bucket_DuelScene.IntentionLinePrefab_*`, `Shader_IntentionLineNew_*` |
| ETB, triggers, death dissolve & tapped VFX | `Bucket_Card.EtbVFX_*`, `Bucket_Card.EtbTriggerVFX_*`, `Bucket_Card.DeathVFX_*`, `Bucket_Card.TappedCardVFX_*` |
| Ability persistent animations & choices | `Bucket_Card.AbilityPersistAnimation_*`, `Bucket_Card.ChoiceVFX_*`, `Bucket_Ability.GainVfx_*` |
| Turn glows, avatar highlights & duel UI FX | `Atlas_Duel_PlayerTurnGlows_*`, `Bucket_Avatar.HighlightVFX_*`, `Bucket_Avatar.PersistVFX_*`, `Bucket_UI.DuelScene.*` |
| Shared VFX shaders | `Shader_ParticleUber_*`, `Shader_GlowPremultiplied_*`, `Shader_GlowPanner_*`, `Shader_GlowFading_*`, `Shader_TextureMultSmoke_*`, `Shader_Additive_*`, `Shader_CardAdvanced_*` |

### Key `Core.dll` C# classes for card motion and zone transfers

Use `types` / `disasm` on these classes when investigating card flight, timing,
or zone layout behavior:

| C# type (`Core.dll`) | Role |
| --- | --- |
| `MovementSystem.SplineMovementSystem` | Drives active spline transfers (`UpdateMovement`, `MoveTowards`, `GetDefaultSpeed`) |
| `MovementData` | Per-card progress/pose evaluator (`UpdateProgress`, `UpdatePosition`, `UpdateRotation`, `UpdateScale`, `.cctor` default regroup speed/curve) |
| `SplineMovementData` / `SplineData` | `PlaceOnCurveEased`, `GetModifiedPositions` (`WorldAbsolute` endpoint clamp vs. relative TRS warp), `GetModifiedRotations`, `HermiteGetPoint` (uniform cubic Catmull-Rom), `GetPCFFPoints` (linear-reflected phantom endpoints), `GetRotationOnCurve` (piecewise `Quaternion.Slerp`) |
| `ZoneTransferGroup` / `ZoneTransferUXEvent` | Batch stagger scheduling (`TryGetStaggerDuration`, `Update`), including adaptive stagger compression (`MAX_STAGGER_TIME = 1.5 s`, so large batches use `1.5 / count`) |
| `CardHolderBase` / `BaseHandCardHolder` / `HandCardHolder` / `OpponentHandCardHolder` / `LibraryCardHolder` | Zone resting slot order, fan layout, `GetLayoutSplineEvents`, `PlayImpactShake` |
| `TopDownCameraController` | DuelScene perspective camera framing (Unity Y-up left-handed coordinates; needed to project Arena world-space spline nodes into screen NDC and camera depth ratios) |

## The 6-step clean-room RE + visual-match loop

### 1. Film Arena first (`arena-video`)

Before reading a single shader uniform or spline node, film the exact moment in
`~/arena` using `arena-video` (whole table first, then high-resolution crop of
the effect or a 12–15 fps strip for fast card flights):

```bash
just video film <id> <time> --span 4 --fps 2 --cols 4 --width 960
just video film <id> <time> --span 1.5 --fps 12 --cols 5 --width 360
just video film <id> <time> --span 1.5 --fps 4 --cols 3 --width 960 --crop x,y,w,h
```

**Why video comes first**: Raw material properties and serialized curves are
frequently misleading without seeing the composited output. A material's
`_HighRangeColorMult` may look green/cyan on paper while vertex color, texture
tint, and bloom composite it into warm gold; gameplay frames immediately expose
state-driven shifts (blocked attackers turning crimson to gold), zone ordering
(Arena mirrors the opponent's hand across the table so newly drawn cards enter
the **leftmost** slot), and where the motion actually feels fast or paused.

### 2. Inspect ALTs, prefabs, serialized assets, and scene transforms

For **card motion & zone transfers**:

1. Query `ALT_Card` and `ALT_ZoneTransfer` with `alt` to find every branch
   (e.g., Beginning-phase vs. non-Beginning `Speed`, face-down variants, stagger
   duration, SFX/VFX delays).
2. Read the referenced `ScriptableObject` / `MonoBehaviour` assets via `UnityPy`
   (`read_typetree()`) to inspect `Speed`, `AnimationCurve` Hermite keyframes,
   and `SplineData` control nodes.
3. Read `DuelScene.unity` camera parameters from `Bucket_Scenes.ScenePayloads_*`
   (iterating all non-`.resS` nodes) so you can project Arena's world-space
   control points into screen-space NDC and camera depth ratios (`rel_scale =
   depth_end / depth_node`).

For **VFX & indicators**, run `prefab` and `cabs` on the target bundle:

```bash
python3 "$SCRIPT" prefab <bundle-prefix>
python3 "$SCRIPT" cabs <bundle-prefix>
```

Record:

- Child objects and component types (`ParticleSystem`, `MeshRenderer`,
  `LineRenderer`, `Animator`, quality toggles like `EnableByQualitySetting`).
- State flags embedded in the prefab (e.g., `IsBlocked`, `IsPendingBlock`,
  `IsExertingVFX`).
- Which `Shader_*` and `Textures_*` bundles the `CAB-*` dependencies resolve to.
  Texture filenames alone (`T_maskCard_halfEdge_top`, `electric_halfswirl`,
  `T_smoke_curly_tile`, `Sprite_VFX_Spark`) tell you what conceptual layers
  compose the effect without ever opening or copying the image files.

### 3. Read the material properties and shader math

Run `materials` on the prefab bundle and `shader` on each referenced `Shader_*`
bundle:

```bash
python3 "$SCRIPT" materials <bundle-prefix>
python3 "$SCRIPT" shader <shader-prefix> --match "<texture>.sample"
```

Extract only the rendering architecture and parameters:

- **Blend mode** (`_SrcBlend`, `_DstBlend`, `_ALPHAPREMULTIPLY_ON`): check
  whether the effect uses pure additive (`1, 1`) or premultiplied alpha
  (`1, 10`). Premultiplied translucent bodies keep saturated reds/golds vivid
  over light playmats where pure additive washes out to pastel or white.
- **UV motion & distortion**: pan speeds (`_DistTex_PanX/Y`, `_MultTex_PanX/Y`),
  tiling scales (`_ST`), polar vs. Cartesian coordinates
  (`_USEPOLARCOORDMULTTEX_ON`), and distortion strength.
- **Luminance & contrast curves**: e.g., `GlowPremultiplied` lifts midtones with
  `pow(alpha, 0.39)`, overlay-blends a hot core color weighted by
  `pow(t, 1.38)`, and sharpens the peak with `pow(mixed * 1.06, 5.48)`.
- **Active shader keywords** on the material (`DISTTEX_ON`, `MASKTEX_ON`,
  `MULTTEX_ON`, `FRESNEL_ON`). Filter `shader` by `.sample(` calls in the
  fragment body, not by uniform names in `UnityPerMaterial_Type`, because uber
  shaders declare every uniform struct field in every variant.

### 4. Inspect C# types and ARM64 disassembly when rules or math are in code

Use `symbols`, `types`, and `disasm` when neither assets nor video answer
orchestration, speed multipliers, stagger compression, or interpolation math:

```bash
python3 "$SCRIPT" types "MovementData|SplineData"
python3 "$SCRIPT" disasm "MovementData" "UpdateProgress|UpdatePosition"
```

Common things C# disassembly resolves in minutes that video alone hides:

- **Exact progress speed formula**: e.g., `progress += dt * endPoint.Speed *
  spline.Speed * globalSpeedMultiplier`, and default non-spline regroup speed
  (`GLOBALSPEED = 4.0` = `250 ms`).
- **Endpoint handling**: `WorldAbsolute` + `Clamp` overwrites only `nodes[0]`
  and `nodes[N - 1]` while keeping interior nodes fixed in space, whereas
  relative splines apply an affine TRS warp.
- **Spline boundary & rotation rules**: uniform cubic Catmull-Rom with reflected
  phantom endpoints (`P = 2C - F`, `FF = 2F - C`) vs. piecewise `Quaternion.Slerp`
  for rotation.
- **Adaptive stagger caps**: e.g., `ZoneTransferGroup` compressing `0.25 s`
  stagger to `1.5 / count` when `0.25 * count > 1.5 s`.

### 5. Author clean-room Rust + WGSL for the client

Translate the *technique* cleanly into the client's architecture
(`docs/beryl-3d.md` and `client/AGENTS.md`):

#### For VFX & Shaders

- **Baseline vs. Enhanced split (Decision C8)**: Leave Baseline's frozen
  `client/crates/beryl_render/src/shader.wgsl` untouched (1 texture sample
  per fragment, 2015 floor / OpenGL ES 3.0 budget). Put rich procedural noise,
  UV distortion, Fresnel rims, bloom, and extra particles in the **Enhanced**
  tier (`client/crates/beryl_render/src/enhanced/`).
- **`Draw` record & `shader_cost` awareness**: `kit_frame::frame::Draw` is fixed
  at 80 bytes (`uv: [u16; 4]`, `params: [f32; 4]`). Adding branches to `fs_lit`
  in `enhanced/entry.wgsl` increases entry-point naga expressions charged to
  every Enhanced material (`beryl_render/tests/all/shader_cost.rs`). For a
  single lightweight material, update `data/client/gates.json` deliberately;
  for multiple bespoke VFX shaders, add a dedicated Enhanced VFX render pipeline
  instead of expanding `fs_lit`.
- **Bloom routing**: `enhanced/bloom.rs` only blooms `Band::Glows` and
  `Band::LayerCover`. Place luminous VFX in `Band::Glows`, give `bloom.rs`
  access to the VFX shading entry point, and calibrate per-effect bloom gain so
  overlapping glows and arrows do not clip to pure white.
- **Premultiplied body + additive rim**: Combine a premultiplied translucent
  colored plasma/curtain (`alpha > 0`) with additive bright rims and halos
  (`alpha = 0` contribution) in the same fragment output—the client's premultiplied
  blend state (`One`, `OneMinusSrcAlpha`) supports both simultaneously in one
  pass.
- **Procedural noise vs. code-generated atlas**: Simple effects run cleanly with
  0 extra texture samples using 2–3 octave WGSL value/gradient noise. For
  heavier effects, generate a small deterministic tileable RGBA noise/mask page
  in Rust at startup (`beryl_assets`) to trade ALU for 1 cheap texture sample.
- **Determinism, zero allocation, and ambient wake**:
  - Compute UV phases, flicker, and looping sparks as pure closed-form functions
    of `(slot_seed, now)` with zero heap allocations in `Client::frame`.
  - Never make `Show::wake` return `Wake::Now` forever for idle looping VFX, or
    headless test commands like `--do still` will never settle. Signal ambient
    VFX ticks to live display hosts (`beryl_host/src/frames.rs`) separately from
    finite script wake, and freeze ambient motion under reduced motion.

#### For 3D Card Motion, Splines & Timing

Key the client files to read and update together:

- **Step durations, phase splits, stagger & regrouping**:
  `data/ui-conventions.json`, `engine/crates/beryl_protocol/src/script/timing.rs`
  (and `timing/rules.rs` and `timing/tests.rs`).
- **Compact 3D spline & Hermite curve engine**:
  `client/crates/kit_base/src/pose/spline.rs` (`Shape::Spline`,
  `HermiteCurve`, `SplineNode`) and
  `client/crates/kit_base/src/pose/segment.rs`.
- **Data-driven verb tuning & layout profiles**: `data/client/verbs.json`,
  `client/crates/kit_base/src/data/verbs.rs`,
  `data/client/profiles/desktop.json` and `phone.json`.
- **Verb playback & camera projection**:
  `client/crates/game_play/src/verbs/fly.rs`.
- **Resting zone and hand order**:
  `client/crates/game_table/src/table/hands.rs`.
- **Probe tests**: `client/crates/game_probe/tests/all/verbs_fly_draw.rs`.

Essential motion translation rules:

- **Update real step durations — never squeeze Arena's curves into old Beryl
  timings**: Compressing a 1429 ms Hermite hold-plateau curve into 1050 ms (or
  an 833 ms opponent draw into 475 ms) distorts the pause shoulders and flight
  feel. Update `data/ui-conventions.json` and `beryl_protocol::script::timing`,
  run `just conventions`, and inspect the updated playback with
  `just client timeline <recording>`.
- **Avoid camera depth-ratio speed warp**: Arena's camera sits far enough back
  that library-to-hand depth varies by only `~1.6:1`, so 3D world Catmull-Rom
  maps evenly to screen speed. The client's camera puts the table at `~1300 mm`
  and the hand at `~300 mm` (`~4.8:1` ratio), which warps screen-space velocity
  if Catmull-Rom runs directly in Beryl world coordinates. Use `Shape::Spline`'s
  **homogeneous camera-ray Catmull-Rom** (`(ray * w, w)` weighted by Arena's
  frustum depth ratio `w = 1 / rel_scale`, divided by `w(u)` and scaled by
  Catmull-Rom `depth(u)`) so screen-space NDC velocity matches Arena 1:1 while
  preserving true 3D depth for shadows and layering.
- **Check resting zone order, Z-rotation symmetry, and edge-on flips**:
  - Verify which side of a zone newly arriving cards enter (e.g., opponent hand
    cards are mirrored across the table and enter the **leftmost** slot.
    Clamping to a different slot can create an end-of-flight U-turn).
  - `CentredFan` with `inverted: true` rotates opponent hand cards by `π` about
    local Z; set `start_half_turn` on symmetric face-down departures so cards do
    not spin 180° in-plane mid-flight.
  - Derive `Pose::turn` from the 3D front-face normal
    `(world_rot * Vec3::Z).dot(camera_back) >= 0` across edge-on flips so
    `StateBits::FACE_DOWN` switches on the exact edge-on frame with zero
    transform discontinuity.
- **Respect `Segment` size, zero-allocation, and toolkit-layer budgets**:
  - `kit_base::pose::Segment` must stay `<= 256` bytes
    (`clippy::large_types_passed_by_value` across `game_play`) and `Shape`
    variant size spread `< 200` bytes (`clippy::large_enum_variant`), with zero
    heap allocations in `Client::frame` (`client-gate`). Quantize spline nodes
    (`i16`/`u16`, smallest-three quaternions) and assert the size budget in
    tests (`pose::spline::tests::segment_stays_within_value_size_budget`).
  - `kit_base` is a toolkit crate enforced by `just client levels`: never mention
    Magic domain terms (`library`, `hand`, `opponent`, `card`) in `kit_base`
    code or comments.
- **Respect Beryl conventions**: Check `docs/ui-conventions.md` (for example,
  Beryl taps cards 90° rather than Arena's ~15° tilt, so normalize shader
  geometry by `aspect = height / base_width` rather than assuming a fixed edge
  length).

### 6. Iterate side-by-side (`arena-pixel-match` style)

Render the client at 1× playback speed (`--no-catch-up`) and compare filmstrips
directly beside the Arena clips from step 1:

```bash
just client png <scene> --at <moment> --tier enhanced --scale 2
# Note: step:V.B.S is the step's MIDPOINT. Use --no-catch-up and ±half-duration
# to film a full 1x animation from start to end:
just client film <scene> --seat you --no-catch-up \
  --from step:V.B.S-<half>ms --to step:V.B.S+<half>ms --frames 15
```

Check these failure modes that static asset reading and unit tests miss:

1. **Multi-card crowding**: Adjacent cards' glows/shields colliding or crossing
   like overlapping eyebrows when widths exceed ~105% of card width.
2. **Composite blowout**: Glowing card marks + bright targeting/blocking arrows
   + bloom summing to solid white at intersections.
3. **Row-gap clearance**: Opposing rows' chevrons and shields touching across
   the battlefield gap.
4. **Both seats, batch sizes & orientations**: Verify `you` and `opponent`
   seats, single vs. multi-card batches, tapped vs. untapped cards, desktop vs.
   phone layouts, and bright vs. dark playmat regions.
5. **Motion artifacts**: Watch the 1× filmstrip for wrong hand-side entry or
   end-of-flight U-turns, 180° in-plane back-spins on face-down flights,
   compressed hold plateaus, or depth-ratio speed warp.
6. **Temporal readability**: Check multi-frame strips to confirm scrolling wisps
   and particles read as smooth motion rather than high-frequency noise.

## Common pitfalls

- **Forgetting `--no-catch-up` or midpoint math in `just client film`**: Without
  `--no-catch-up`, the client replays queued scripts at 2×–4× speed; and
  `step:V.B.S` resolves to the step's **midpoint**, not its start.
- **Trusting video-estimated durations over serialized `Speed` + curve**:
  Zero-slope Hermite ease-in keeps a card visually over its starting pile for
  ~150–200 ms, so video stopwatches (`data/arena-motion.json`) can under-report
  the true spline duration; always verify against the ALT + `SplineMovementData`
  `Speed` and curve tail.
- **UnityFS v8 padding**: Bundles with `flags & 0x200` align the start of data
  blocks to a 16-byte boundary after `blocksInfo`; omitting that alignment
  corrupts LZ4 decompression immediately.
- **Multi-node scene bundles**: `Bucket_Scenes.ScenePayloads_*` stores many
  `SerializedFile` nodes (one per scene plus `.resS`); iterate all non-`.resS`
  nodes instead of assuming `nodes[0]`.
- **`UnityPy` version warning**: MTGA strips the Unity version string from
  bundles; set `UnityPy.config.FALLBACK_UNITY_VERSION = "2022.3.0f1"` before
  calling `read_typetree()`.
- **Mach-O ARM64 chained fixups & `objdump` range bug**: Raw 64-bit pointers
  inside `GameAssembly.dylib` `__DATA_CONST`/`__DATA` carry chained-fixup
  metadata in their upper bits; mask with `(1 << 36) - 1` before comparing
  against virtual addresses. Also, Apple's `objdump` / `xcrun llvm-objdump`
  ignores `--start-address`/`--stop-address` when given `--arch=arm64` on a
  universal binary (dumping millions of lines); use `xcrun lldb -b` (as
  `inspect_mtga.py disasm` does) to disassemble a single function range in
  under a second.
- **Uber-shader false positives**: `Shader_ParticleUber` declares `_DistTex`,
  `_MultTex`, and `_FresnelExp` inside `UnityPerMaterial_Type` for *all*
  variants. Always match on actual `.sample(` calls inside `fragment `.
- **Over-interpreting raw HDR floats**: Multiply material colors by vertex
  colors and check the actual video frame before picking Beryl's palette.
- **Dual Rust workspaces formatting**: `just fmt` formats `engine/` and `client/`, JS, Python,
  and Markdown, while `client/` is a separate Cargo workspace that also
  needs `cargo fmt --manifest-path client/Cargo.toml --all`.
