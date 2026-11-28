// Procedural Enhanced-tier mana tap effects, tinted by the color of the mana.
//
// `Vfx::ManaBirth`, on the source as its mana leaves (Arena's `vfx_ManaTap_<Color>_Birth`): a
// dark under-glow, a soft additive glow, a ring of swirling light in the mana's color round a hot
// pip, a ring burst that runs outward, and 20 white sparks thrown out from the middle.
//
// `Vfx::ManaHit`, on what the mana pays for as it lands (Arena's `vfx_manaStack_<color>_hit`): a
// white four-point light-ping flare with long side flares, colored wisps that spread over the
// card, and 12 sparks in the mana's color.
//
// Both quads are square in the subject's plane. Zero texture samples; smooth quad-edge feathering.

const VFX_MANA_BIRTH: u32 = 15u;
const VFX_MANA_HIT: u32 = 16u;

// Each effect draws over a small area, but up to seven lands can pay for one spell at once and
// their light can overlap on the table's bloom: these keep a pile of them short of white.
const BLOOM_GAIN_MANA_BIRTH: f32 = 0.30;
const BLOOM_GAIN_MANA_HIT: f32 = 0.26;

// `tint` most of the way to white: the hot middle of a mana color's light.
fn mana_hot(tint: vec3<f32>) -> vec3<f32> {
    return mix(tint, vec3<f32>(1.0), 0.78);
}

// A soft spark at `p`, at `centre`, `radius` across, stretched `stretch` times along `dir`.
fn mana_spark(p: vec2<f32>, centre: vec2<f32>, dir: vec2<f32>, radius: f32, stretch: f32) -> f32 {
    let d = p - centre;
    let along = dot(d, dir) / stretch;
    let across = dot(d, vec2<f32>(-dir.y, dir.x));
    let r = max(radius, 1e-4);
    return exp(-(along * along + across * across) / (r * r));
}

fn shade_mana_birth(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let pol = vfx_polar(p);
    let dist = pol.y;
    let angle = pol.x;
    let edge = vfx_quad_edge(uv, 0.80);
    let color = tint.rgb;
    let hot = mana_hot(color);

    // 1. Dark under-glow, so the light reads on a bright playmat.
    let under_env = smoothstep(0.0, 0.10, t) * (1.0 - smoothstep(0.35, 0.85, t));
    let under = exp(-(dist * dist) / (2.0 * 0.20 * 0.20)) * under_env;

    // 2. Soft additive glow in the mana's color, up at once and gone by two thirds.
    let glow_env = smoothstep(0.0, 0.06, t) * (1.0 - smoothstep(0.18, 0.70, t));
    let glow = exp(-(dist * dist) / (2.0 * 0.15 * 0.15)) * glow_env;

    // 3. The swirl: a band of polar noise turning round the middle, which widens a little and
    // thins away. Its crest is hot.
    let swirl_t = clamp(t / 0.75, 0.0, 1.0);
    let swirl_r = mix(0.14, 0.21, pow(swirl_t, 0.6));
    let turn = angle + dist * 9.0 - t * 7.5 + seed * 6.2831853;
    let swirl_n = vfx_fbm(vec2<f32>(cos(turn), sin(turn)) * 2.3 + vec2<f32>(seed * 13.0, t * 1.4));
    let band = exp(-pow(dist - swirl_r, 2.0) / (2.0 * 0.045 * 0.045));
    let swirl_env = smoothstep(0.0, 0.07, t) * (1.0 - smoothstep(0.28, 0.72, t));
    let swirl = band * smoothstep(0.30, 0.78, swirl_n) * swirl_env;
    let crest = band * smoothstep(0.58, 0.86, swirl_n) * swirl_env;

    // 4. The pip: a hot point in the middle that swells and fades.
    let pip_env = smoothstep(0.0, 0.08, t) * (1.0 - smoothstep(0.20, 0.55, t));
    let pip = exp(-(dist * dist) / (2.0 * 0.05 * 0.05)) * pip_env;

    // 5. A thin ring burst that runs outward over the first third.
    let ring_t = clamp(t / 0.36, 0.0, 1.0);
    let ring_r = mix(0.06, 0.40, pow(ring_t, 0.5));
    let ring_env = smoothstep(0.0, 0.15, ring_t) * (1.0 - smoothstep(0.35, 1.0, ring_t));
    let ring = exp(-pow(dist - ring_r, 2.0) / (2.0 * 0.014 * 0.014)) * ring_env;

    // 6. 20 white sparks thrown out from the middle, each at its own angle and speed, slowing
    // as they go.
    var sparks = 0.0;
    for (var i = 0u; i < 20u; i++) {
        let fi = f32(i);
        let h = vfx_hash(vec2<f32>(fi * 7.13 + seed * 31.0, fi * 3.71 + 1.3));
        let h2 = vfx_hash(vec2<f32>(fi * 1.97 + 5.1, seed * 17.0 + fi * 0.37));
        let born = h2 * 0.18;
        let age = clamp((t - born) / (0.45 + 0.25 * h), 0.0, 1.0);
        let live = step(born, t) * (1.0 - age);
        let a = (fi + h * 0.8) * 0.31415927 * 2.0;
        let dir = vec2<f32>(cos(a), sin(a));
        let reach = (0.10 + 0.30 * h2) * (1.0 - (1.0 - age) * (1.0 - age));
        sparks += mana_spark(p, dir * (0.05 + reach), dir, 0.010, 2.4) * live * live;
    }

    let body_alpha = clamp(under * 0.38 * edge, 0.0, 0.5) * tint.a;
    let add = (color * (glow * 0.55 + swirl * 0.95 + ring * 0.55)
        + hot * (crest * 0.85 + pip * 1.1)
        + vec3<f32>(1.0) * min(sparks, 1.5) * 0.9) * edge * tint.a;
    return vec4<f32>(add, body_alpha);
}

fn shade_mana_hit(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let pol = vfx_polar(p);
    let dist = pol.y;
    let angle = pol.x;
    let edge = vfx_quad_edge(uv, 0.78);
    let color = tint.rgb;
    let hot = mana_hot(color);

    // 1. The light-ping flare: a white core and four rays, the side pair longest, which flash
    // up at once and close by the half.
    let ping_env = smoothstep(0.0, 0.05, t) * (1.0 - smoothstep(0.10, 0.48, t));
    let core = exp(-(dist * dist) / (2.0 * 0.026 * 0.026));
    let ax = abs(p.x);
    let ay = abs(p.y);
    let side = exp(-ay * ay / (2.0 * 0.006 * 0.006)) * exp(-ax / 0.20);
    let upright = exp(-ax * ax / (2.0 * 0.006 * 0.006)) * exp(-ay / 0.11);
    let ping = (core * 1.2 + side * 0.95 + upright * 0.75) * ping_env;

    // 2. A soft glow in the mana's color over what it lands on.
    let glow_env = smoothstep(0.0, 0.08, t) * (1.0 - smoothstep(0.20, 0.80, t));
    let glow = exp(-(dist * dist) / (2.0 * 0.14 * 0.14)) * glow_env;

    // 3. Wisps: polar noise that spreads outward from the middle and thins.
    let wisp_r = mix(0.05, 0.30, pow(t, 0.55));
    let wisp_n = vfx_fbm(vec2<f32>(angle * 1.6 + seed * 9.0, dist * 6.0 - t * 3.0));
    let wisp_band = exp(-pow(dist - wisp_r, 2.0) / (2.0 * 0.07 * 0.07));
    let wisp_env = smoothstep(0.02, 0.12, t) * (1.0 - smoothstep(0.35, 0.95, t));
    let wisps = wisp_band * smoothstep(0.42, 0.80, wisp_n) * wisp_env;

    // 4. 12 sparks in the mana's color scattered outward, stretched along their way.
    var sparks = 0.0;
    for (var i = 0u; i < 12u; i++) {
        let fi = f32(i);
        let h = vfx_hash(vec2<f32>(fi * 5.31 + seed * 23.0, fi * 2.17 + 0.7));
        let h2 = vfx_hash(vec2<f32>(fi * 3.03 + 2.9, seed * 11.0 + fi * 0.61));
        let born = h2 * 0.12;
        let age = clamp((t - born) / (0.55 + 0.35 * h), 0.0, 1.0);
        let live = step(born, t) * (1.0 - age);
        let a = (fi + h) * 0.52359878;
        let dir = vec2<f32>(cos(a), sin(a));
        let reach = (0.12 + 0.30 * h2) * (1.0 - (1.0 - age) * (1.0 - age));
        sparks += mana_spark(p, dir * (0.04 + reach), dir, 0.013, 2.8) * live;
    }

    let body_alpha = 0.0;
    let add = (vec3<f32>(1.0) * ping * 0.70
        + color * (glow * 0.35 + wisps * 0.70)
        + hot * min(sparks, 1.5) * 0.90) * edge * tint.a;
    return vec4<f32>(add, body_alpha);
}
