---
name: arena-re
description: >-
  Reverse-engineer how MTG Arena implements a visual effect, shader, animation,
  highlight, spline, or combat/UI indicator from the locally installed macOS
  MTGA client, and translate it clean-room into Beryl 3D. Use when designing or
  matching complex Arena VFX, shaders, particle systems, card highlights,
  targeting lines, or multi-stage animations where video frames alone do not
  reveal the underlying rendering technique.
---

# Arena Reverse-Engineering Reference (Clean-Room VFX & Animation)

Use this skill alongside `arena-video` and `arena-pixel-match` when building
rich VFX, shaders, particle systems, highlights, or animations in Beryl 3D and
gameplay video alone does not explain *how* MTG Arena renders them (blend modes,
UV distortion math, color transfer curves, state flags, or layer stacking).

## Clean-room rule (mandatory)

Beryl is a public repository. **Never** copy, commit, or redistribute any
proprietary asset or code from the MTG Arena installation:

- Never import, convert, or trace MTGA textures (`.png`, `.tga`), 3D meshes,
  prefabs, animation clips, fonts, or audio files.
- Never paste or mechanically port decompiled C# or extracted Metal Shading
  Language (MSL) code into Beryl.
- **Do** inspect MTGA strictly read-only to learn the *technique*: layer stack,
  blend mode, state transitions, UV scroll/polar coordinates, contrast/ramp
  math, and particle behavior. Then author original procedural WGSL, Rust
  geometry, or code-generated sprites (`beryl_assets`) and tune visually
  against `arena-video` recordings.

## Where MTGA lives on macOS and how it is structured

Default Steam installation on macOS:

- App bundle: `~/Library/Application Support/Steam/steamapps/common/MTGA/MTGA.app`
- Downloaded assets: `~/Library/Application Support/Steam/steamapps/common/MTGA/MTGA_Data/Downloads/`

Key facts about the macOS Unity build (Unity 6 / IL2CPP):

- **~85% of Arena's VFX and animations live in data-driven AssetBundles**, not
  in C#. C# (`GameAssembly.dylib`) mainly toggles animator states, assigns
  highlight types, and triggers prefab playback. Always inspect the Manifest,
  Prefabs, Materials, and Shaders before touching IL2CPP disassembly.
- **Manifest (`Manifest_*.mtga`)**: plain-text JSON indexing every downloaded
  `.mtga` bundle to its original Unity `Assets/...` source paths.
- **AssetBundles (`AssetBundle/*.mtga`)**: standard unencrypted `UnityFS` (v8,
  LZ4HC) archives containing `SerializedFile` (v22) objects with embedded
  TypeTrees.
- **Shaders (`Shader_*.mtga` and `globalgamemanagers.assets`)**: on macOS,
  Unity embeds LZ4-compressed **plain-text Metal Shading Language (MSL)**
  (`#include <metal_stdlib>`, `xlatMtlMain`) alongside serialized blend, depth,
  and stencil states.
- **C# metadata (`il2cpp_data/Metadata/global-metadata.dat`)**: unencrypted
  Unity IL2CPP metadata (`0xFAB11BAF`) with unobfuscated C# namespaces, types,
  methods, and field names.
- **Live GPU capture**: `MTGA.app` is signed without Hardened Runtime
  (`codesign` flags `0x0`), so Xcode Metal Frame Capture can inspect live draw
  calls, bound uniforms, and pass order whenever static inspection is ambiguous.

## Helper script

Use the read-only, standard-library helper in this skill's directory:

```bash
SCRIPT=.agents/skills/arena-re/scripts/inspect_mtga.py
python3 "$SCRIPT" manifest "combat|highlight|intention"
python3 "$SCRIPT" prefab Bucket_Card.AttackerIcon_0
python3 "$SCRIPT" cabs Bucket_Card.AttackerIcon_0
python3 "$SCRIPT" materials Bucket_Card.AttackerIcon_0
python3 "$SCRIPT" shader Shader_ParticleUber --match "_DistTex.sample" --match "_MultTex.sample"
python3 "$SCRIPT" symbols "UpdateHalos|DrawCombatArrow|BattlefieldIcons"
```

Pass `--mtga <path>` if MTGA is installed outside the default Steam folder.

## Where Arena puts different kinds of VFX and animations

Search the Manifest first (`manifest <regex>`) to map a feature to its bundles:

| Feature area | Typical Manifest bundle prefixes |
| --- | --- |
| Card eligibility / hover / selection highlights | `Bucket_Card.Highlight_*`, `Bucket_Card.Parts.HighlightsPart_*` |
| Combat attacker & blocker indicators | `Bucket_Card.AttackerIcon_*`, `Bucket_Card.BlockerIcon_*` |
| Combat strikes, splines & hit impacts | `Bucket_Combat.Spline*`, `Bucket_Combat.VFXBirth_*`, `Bucket_Combat.VFXHit_*`, `Bucket_Combat.VFXSustain_*` |
| Targeting, blocking & intention arrows | `Bucket_DuelScene.IntentionLinePrefab_*`, `Shader_IntentionLineNew_*` |
| ETB, triggers, death dissolve & tapped VFX | `Bucket_Card.EtbVFX_*`, `Bucket_Card.EtbTriggerVFX_*`, `Bucket_Card.DeathVFX_*`, `Bucket_Card.TappedCardVFX_*` |
| Ability persistent animations & choices | `Bucket_Card.AbilityPersistAnimation_*`, `Bucket_Card.ChoiceVFX_*`, `Bucket_Ability.GainVfx_*` |
| Turn glows, avatar highlights & duel UI FX | `Atlas_Duel_PlayerTurnGlows_*`, `Bucket_Avatar.HighlightVFX_*`, `Bucket_Avatar.PersistVFX_*`, `Bucket_UI.DuelScene.*` |
| Shared VFX shaders | `Shader_ParticleUber_*`, `Shader_GlowPremultiplied_*`, `Shader_GlowPanner_*`, `Shader_GlowFading_*`, `Shader_TextureMultSmoke_*`, `Shader_Additive_*`, `Shader_CardAdvanced_*` |

## The 6-step clean-room RE + visual-match loop

### 1. Film Arena first (`arena-video`)

Before reading a single shader uniform, film the exact moment in `~/arena` using
`arena-video` (whole table first, then high-resolution crop of the effect):

```bash
just video film <id> <time> --span 4 --fps 2 --cols 4 --width 960
just video film <id> <time> --span 1.5 --fps 4 --cols 3 --width 960 --crop x,y,w,h
```

**Why video comes first**: Raw material properties are frequently misleading
without seeing the composited output. For example, a material's
`_HighRangeColorMult` may look green/cyan on paper while vertex color, texture
tint, and bloom composite it into warm gold; and gameplay frames immediately
expose state-driven shifts (such as blocked attackers turning from crimson to
gold, or brief confirmation flashes) that static prefab dumps obscure.

### 2. Inspect the prefab hierarchy, states and components

Run `prefab` and `cabs` on the target bundle:

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

### 4. Check C# symbols only when orchestration or timing is unclear

If you need to know *when* a state flag toggles, how arrows/splines are driven,
or what controls highlight priority, search `global-metadata.dat`:

```bash
python3 "$SCRIPT" symbols "<regex>"
```

Only disassemble `GameAssembly.dylib` (ARM64 Mach-O) if neither the assets nor
Arena video answer the timing or state logic; for most visual effects, video
frame counts at 30/60 fps plus prefab state names are faster and exact.

### 5. Author clean-room Rust + WGSL for Beryl 3D

Translate the *technique* cleanly into Beryl 3D's architecture
(`docs/beryl-3d.md` and `rust/beryl3d/AGENTS.md`):

- **Baseline vs. Enhanced split (Decision C8)**: Leave Baseline's frozen
  `rust/beryl3d/crates/beryl_render/src/shader.wgsl` untouched (1 texture sample
  per fragment, 2015 floor / OpenGL ES 3.0 budget). Put rich procedural noise,
  UV distortion, Fresnel rims, bloom, and extra particles in the **Enhanced**
  tier (`beryl_render/src/enhanced/`).
- **`Draw` record & `shader_cost` awareness**: `kit_frame::frame::Draw` is fixed
  at 80 bytes (`uv: [u16; 4]`, `params: [f32; 4]`). Adding branches to `fs_lit`
  in `enhanced/entry.wgsl` increases entry-point naga expressions charged to
  every Enhanced material (`beryl_render/tests/all/shader_cost.rs`). For a
  single lightweight material, update `data/beryl3d/gates.json` deliberately;
  for multiple bespoke VFX shaders, add a dedicated Enhanced VFX render pipeline
  instead of expanding `fs_lit`.
- **Bloom routing**: `enhanced/bloom.rs` only blooms `Band::Glows` and
  `Band::LayerCover`. Place luminous VFX in `Band::Glows`, give `bloom.rs`
  access to the VFX shading entry point, and calibrate per-effect bloom gain so
  overlapping glows and arrows do not clip to pure white.
- **Premultiplied body + additive rim**: Combine a premultiplied translucent
  colored plasma/curtain (`alpha > 0`) with additive bright rims and halos
  (`alpha = 0` contribution) in the same fragment output—Beryl 3D's premultiplied
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
- **Respect Beryl conventions**: Check `docs/ui-conventions.md` (for example,
  Beryl taps cards 90° rather than Arena's ~15° tilt, so normalize shader
  geometry by `aspect = height / base_width` rather than assuming a fixed edge
  length).

### 6. Iterate side-by-side (`arena-pixel-match` style)

Render Beryl 3D in Enhanced mode at 2× scale and compare cropped frames directly
beside the Arena crops from step 1:

```bash
just b3d png <scene> --at <moment> --tier enhanced --scale 2
just b3d film <scene> --from <start> --to <end> --frames 4 --tier enhanced
```

Check these failure modes that shader reading alone will miss:

1. **Multi-card crowding**: Adjacent cards' glows/shields colliding or crossing
   like overlapping eyebrows when widths exceed ~105% of card width.
2. **Composite blowout**: Glowing card marks + bright targeting/blocking arrows
   + bloom summing to solid white at intersections.
3. **Row-gap clearance**: Opposing rows' chevrons and shields touching across
   the battlefield gap.
4. **Both seats & orientations**: Verify `you` (pointing up) and `opponent`
   (pointing down), plus tapped vs. untapped cards and bright vs. dark playmat
   regions.
5. **Temporal readability**: Check a 4-frame filmstrip to confirm scrolling
   wisps and particles read as smooth motion rather than high-frequency noise.

## Common pitfalls

- **UnityFS v8 padding**: Bundles with `flags & 0x200` align the start of data
  blocks to a 16-byte boundary after `blocksInfo`; omitting that alignment
  corrupts LZ4 decompression immediately.
- **Uber-shader false positives**: `Shader_ParticleUber` declares `_DistTex`,
  `_MultTex`, and `_FresnelExp` inside `UnityPerMaterial_Type` for *all*
  variants. Always match on actual `.sample(` calls inside `fragment `.
- **Over-interpreting raw HDR floats**: Multiply material colors by vertex
  colors and check the actual video frame before picking Beryl's palette.
- **Forgetting `data/beryl3d/parity.json`**: Any new skill directory under
  `.agents/skills/` must also be listed in `data/beryl3d/parity.json` (and
  symlinked in `.claude/skills/`) so `just b3d parity --check` passes.
