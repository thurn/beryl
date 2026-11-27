// Procedural Enhanced-tier counter VFX (`Vfx::CounterAdd`, `Vfx::CounterAddNeg`,
// `Vfx::CounterRemove`, `Vfx::ShieldAdd`, `Vfx::ShieldRemove`).
//
// Each effect plays across a padded card quad (`size = 1.8`, `aspect = 1.4`) so both the
// top-right counter badge (`COUNTER_BADGE_POS`) and the card face receive the burst without
// extra draw calls or texture samples. Zero texture samples; smooth quad-edge feathering.

const VFX_COUNTER_ADD: u32 = 6u;
const VFX_COUNTER_ADD_NEG: u32 = 7u;
const VFX_COUNTER_REMOVE: u32 = 8u;
const VFX_SHIELD_ADD: u32 = 9u;
const VFX_SHIELD_REMOVE: u32 = 10u;

const BLOOM_GAIN_COUNTER_ADD: f32 = 0.42;
const BLOOM_GAIN_COUNTER_NEG: f32 = 0.44;
const BLOOM_GAIN_COUNTER_REMOVE: f32 = 0.40;
const BLOOM_GAIN_SHIELD_ADD: f32 = 0.42;
const BLOOM_GAIN_SHIELD_REMOVE: f32 = 0.36;

// Top-right counter badge center in aspect-scaled quad coordinates (`aspect = 1.4`, `size = 1.8`,
// with `uv.y = 0` at the top of the quad).
const COUNTER_BADGE_POS: vec2<f32> = vec2<f32>(0.22, -0.23);

// Signed distance to a classic heater shield centered at the origin with half-width `r`.
fn shield_emblem_sdf(q: vec2<f32>, r: f32) -> f32 {
    let p = vec2<f32>(q.x, -q.y) / max(r, 1e-4);
    let ax = abs(p.x);
    // Flat/arched top, straight upper sides, curved taper to bottom point.
    let top_d = p.y - 0.85 + 0.12 * ax * ax;
    let side_x = mix(0.88, 0.0, smoothstep(0.15, -1.05, p.y));
    let side_d = ax - side_x;
    let bot_d = -1.05 - p.y;
    return max(max(top_d, side_d), bot_d) * r;
}

// Tall vertical flare + horizontal cross-glint centered at `origin`.
fn counter_flare(q: vec2<f32>, v_len: f32, v_w: f32, h_len: f32, h_w: f32) -> f32 {
    let vert = exp(-(q.x * q.x) / (2.0 * v_w * v_w))
        * exp(-(q.y * q.y) / (2.0 * v_len * v_len));
    let horiz = exp(-(q.y * q.y) / (2.0 * h_w * h_w))
        * exp(-(q.x * q.x) / (2.0 * h_len * h_len));
    return vert + 0.45 * horiz;
}

// Expanding crisp shockwave ring around `origin`.
fn counter_ring(dist: f32, radius: f32, width: f32) -> f32 {
    let d = dist - radius;
    return exp(-(d * d) / (2.0 * width * width));
}

// Deterministic radial sparks flying outward from `origin`.
fn counter_sparks(q: vec2<f32>, t: f32, seed: f32, count: i32, max_r: f32) -> f32 {
    var acc = 0.0;
    for (var i = 0; i < count; i++) {
        let fi = f32(i);
        let h1 = vfx_hash(vec2<f32>(fi * 1.37 + seed * 17.0, 3.1));
        let h2 = vfx_hash(vec2<f32>(fi * 2.91 + seed * 11.0, 7.7));
        let angle = (fi / f32(count)) * 6.2831853 + (h1 - 0.5) * 0.42;
        let dir = vec2<f32>(cos(angle), sin(angle));
        let speed = mix(0.55, 1.15, h2) * max_r;
        let head_r = speed * pow(t, 0.65);
        let tail_r = max(head_r - mix(0.03, 0.08, h1) * (1.0 - t * 0.5), 0.0);
        let d = vfx_segment(q, dir * tail_r, dir * head_r);
        let life = (1.0 - smoothstep(0.35, 1.0, t + (h2 - 0.5) * 0.18));
        acc += exp(-(d * d) / (2.0 * 0.0055 * 0.0055)) * life;
    }
    return clamp(acc, 0.0, 1.8);
}

// Tumbling angular debris shards flying outward from `origin`.
fn counter_debris(q: vec2<f32>, t: f32, seed: f32, count: i32, max_r: f32) -> f32 {
    var acc = 0.0;
    for (var i = 0; i < count; i++) {
        let fi = f32(i);
        let h1 = vfx_hash(vec2<f32>(fi * 3.13 + seed * 19.0, 5.3));
        let h2 = vfx_hash(vec2<f32>(fi * 1.79 + seed * 23.0, 9.1));
        let angle = (fi / f32(count)) * 6.2831853 + (h1 - 0.5) * 0.55;
        let dir = vec2<f32>(cos(angle), sin(angle));
        let dist = mix(0.45, 1.15, h2) * max_r * pow(t, 0.68);
        let center = dir * dist + vec2<f32>(0.0, -0.06 * t * t);
        let rot = angle + t * (h1 - 0.5) * 6.0;
        let c = cos(rot);
        let s = sin(rot);
        let local = q - center;
        let rq = vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
        let sz = mix(0.011, 0.020, h1) * (1.0 - 0.45 * t);
        let box_d = max(abs(rq.x) - sz * 1.35, abs(rq.y) - sz * 0.72);
        let piece = 1.0 - smoothstep(0.0, 0.004, box_d);
        let fade = 1.0 - smoothstep(0.45, 1.0, t);
        acc = max(acc, piece * fade);
    }
    return acc;
}

// Positive / standard counter added (`PS_Counter_Spawn_Sm_KH`):
// bright white-cyan vertical flare and hot core on the badge, expanding shockwave ring, and
// wispy energy smoke sweeping over the card.
fn shade_counter_add(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let q = p - COUNTER_BADGE_POS;
    let dist = length(q);
    let edge = vfx_quad_edge(uv, 0.80);

    // 1. Tall vertical flare on the badge (peaks early, fades by t = 0.45).
    let flare_env = (1.0 - smoothstep(0.06, 0.45, t));
    let flare = counter_flare(q, mix(0.04, 0.24, smoothstep(0.0, 0.18, t)), 0.016, 0.08, 0.012)
        * flare_env;

    // 2. Hot badge core flash.
    let core_env = 1.0 - smoothstep(0.05, 0.65, t);
    let core = exp(-(dist * dist) / (2.0 * 0.042 * 0.042)) * core_env;

    // 3. Expanding shockwave ring from the badge.
    let ring_t = clamp((t - 0.06) / 0.78, 0.0, 1.0);
    let ring_r = mix(0.02, 0.44, pow(ring_t, 0.65));
    let ring_env = smoothstep(0.0, 0.12, ring_t) * (1.0 - smoothstep(0.35, 1.0, ring_t));
    let ring = counter_ring(dist, ring_r, 0.018) * ring_env;

    // 4. Wispy smoke curl across the card face.
    let card_d = length(p * vec2<f32>(1.0, 0.72));
    let n = vfx_fbm(p * 6.5 + vec2<f32>(seed * 5.0, -t * 1.4));
    let smoke_ring = exp(-pow(card_d - mix(0.08, 0.34, t), 2.0) / (2.0 * 0.085 * 0.085));
    let smoke_env = smoothstep(0.08, 0.28, t) * (1.0 - smoothstep(0.45, 1.0, t));
    let smoke = clamp(smoke_ring * smoothstep(0.32, 0.78, n) * smoke_env, 0.0, 1.0);

    let body_alpha = clamp((smoke * 0.55 + ring * 0.40 + core * 0.35) * edge, 0.0, 0.85) * tint.a;
    let body_rgb = tint.rgb * body_alpha;
    let hot_col = mix(tint.rgb, vec3<f32>(1.0), 0.78);
    let add_rgb = (hot_col * (flare * 1.25 + core * 0.95) + tint.rgb * (ring * 0.85 + smoke * 0.35))
        * edge * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}

// Harmful (-1/-1 style) counter added (`vfx_counter_spawn_neg`):
// harsher crimson-violet cross-flare, tight shockwave, and dark smoky wisps.
fn shade_counter_add_neg(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let q = p - COUNTER_BADGE_POS;
    let dist = length(q);
    let edge = vfx_quad_edge(uv, 0.80);

    // Harsher diagonal + vertical spikes on the badge.
    let diag1 = vec2<f32>((q.x + q.y) * 0.7071, (q.y - q.x) * 0.7071);
    let flare_env = 1.0 - smoothstep(0.04, 0.42, t);
    let flare = (counter_flare(q, 0.18, 0.014, 0.11, 0.011)
        + 0.65 * counter_flare(diag1, 0.12, 0.011, 0.12, 0.011)) * flare_env;

    let core_env = 1.0 - smoothstep(0.05, 0.60, t);
    let core = exp(-(dist * dist) / (2.0 * 0.038 * 0.038)) * core_env;

    let ring_t = clamp((t - 0.04) / 0.72, 0.0, 1.0);
    let ring_r = mix(0.02, 0.38, pow(ring_t, 0.58));
    let ring_env = smoothstep(0.0, 0.10, ring_t) * (1.0 - smoothstep(0.30, 1.0, ring_t));
    let ring = counter_ring(dist, ring_r, 0.016) * ring_env;

    // Dark smoky wisps across the card.
    let n = vfx_fbm(p * 8.0 + vec2<f32>(seed * 7.0, t * 1.2));
    let card_d = length(p * vec2<f32>(1.0, 0.72));
    let wisp_env = smoothstep(0.05, 0.22, t) * (1.0 - smoothstep(0.42, 1.0, t));
    let dark_smoke = clamp(exp(-(card_d * card_d) / (2.0 * 0.22 * 0.22)) * smoothstep(0.28, 0.74, n) * wisp_env, 0.0, 1.0);

    let dark_col = vec3<f32>(0.24, 0.04, 0.16);
    let body_alpha = clamp((dark_smoke * 0.72 + ring * 0.35 + core * 0.35) * edge, 0.0, 0.88) * tint.a;
    let body_rgb = mix(dark_col, tint.rgb, 0.45) * body_alpha;
    let hot_col = mix(tint.rgb, vec3<f32>(1.0, 0.88, 0.86), 0.70);
    let add_rgb = (hot_col * (flare * 1.15 + core * 0.90) + tint.rgb * (ring * 0.85)) * edge * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}

// Counter removed (`vfx_counter_removal`):
// flash, expanding ring, 14 outward sparks, wispy smoke, and 8 tumbling shatter pieces.
fn shade_counter_remove(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let q = p - COUNTER_BADGE_POS;
    let dist = length(q);
    let edge = vfx_quad_edge(uv, 0.80);

    let flash_env = 1.0 - smoothstep(0.02, 0.32, t);
    let flash = exp(-(dist * dist) / (2.0 * 0.045 * 0.045)) * flash_env;

    let ring_r = mix(0.02, 0.36, pow(clamp(t / 0.65, 0.0, 1.0), 0.60));
    let ring_env = 1.0 - smoothstep(0.20, 0.68, t);
    let ring = counter_ring(dist, ring_r, 0.015) * ring_env;

    let sparks = counter_sparks(q, t, seed, 14, 0.34);
    let debris = counter_debris(q, t, seed, 8, 0.30);

    let n = vfx_fbm(q * 9.0 + vec2<f32>(seed * 3.0, t * 1.5));
    let wisp = exp(-(dist * dist) / (2.0 * 0.14 * 0.14))
        * smoothstep(0.35, 0.80, n)
        * (1.0 - smoothstep(0.25, 0.85, t));

    let debris_col = vec3<f32>(0.42, 0.48, 0.92);
    let body_alpha = clamp((debris * 0.85 + wisp * 0.45 + ring * 0.30) * edge, 0.0, 0.90) * tint.a;
    let body_rgb = mix(tint.rgb, debris_col, debris) * body_alpha;
    let hot_col = mix(tint.rgb, vec3<f32>(1.0), 0.80);
    let add_rgb = (hot_col * (flash * 1.15 + sparks * 0.85) + tint.rgb * (ring * 0.75 + debris * 0.35))
        * edge * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}

// Shield counter added (`VFX_SNC_Shield_Counter_Add`):
// diamond star flash, teal-cyan energy shockwave, 8 sparks, glowing shield emblem, and card rim.
fn shade_shield_add(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let q = p - COUNTER_BADGE_POS;
    let dist = length(q);
    let edge = vfx_quad_edge(uv, 0.80);

    // 1. Star-diamond flash.
    let flash_env = 1.0 - smoothstep(0.02, 0.28, t);
    let flash = counter_flare(q, 0.14, 0.013, 0.14, 0.013) * flash_env;

    // 2. Teal-cyan shockwave ring.
    let ring_r = mix(0.02, 0.40, pow(clamp(t / 0.65, 0.0, 1.0), 0.62));
    let ring_env = 1.0 - smoothstep(0.22, 0.68, t);
    let ring = counter_ring(dist, ring_r, 0.016) * ring_env;

    // 3. 8 cyan sparks.
    let sparks = counter_sparks(q, t, seed, 8, 0.32);

    // 4. Glowing SDF shield emblem on the badge.
    let emblem_env = smoothstep(0.04, 0.20, t) * (1.0 - smoothstep(0.50, 1.0, t));
    let sd = shield_emblem_sdf(q, mix(0.052, 0.068, smoothstep(0.0, 0.35, t)));
    let emblem_fill = (1.0 - smoothstep(-0.004, 0.006, sd)) * emblem_env;
    let emblem_rim = exp(-(sd * sd) / (2.0 * 0.007 * 0.007)) * emblem_env;

    // 5. Subtle teal card-rim pulse.
    let box_d = max(abs(p.x) - 0.275, abs(p.y) - 0.385);
    let card_rim = exp(-(box_d * box_d) / (2.0 * 0.014 * 0.014))
        * smoothstep(0.10, 0.30, t) * (1.0 - smoothstep(0.45, 0.95, t));

    let body_alpha = clamp((emblem_fill * 0.45 + ring * 0.35 + card_rim * 0.30) * edge, 0.0, 0.85) * tint.a;
    let body_rgb = tint.rgb * body_alpha;
    let hot_col = mix(tint.rgb, vec3<f32>(1.0), 0.72);
    let add_rgb = (hot_col * (flash * 1.15 + emblem_rim * 0.90 + sparks * 0.70)
        + tint.rgb * (ring * 0.85 + card_rim * 0.55)) * edge * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}

// Shield counter broken (`VFX_SNC_Shield_Counter_Break`):
// ice-white crackle across a deep steel-blue shield emblem that splits into 3 flying shards.
fn shade_shield_remove(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let q = p - COUNTER_BADGE_POS;
    let edge = vfx_quad_edge(uv, 0.80);

    // 1. Intact emblem + ice-white crackle in early phase (t < 0.42).
    let intact_env = 1.0 - smoothstep(0.24, 0.44, t);
    let sd_intact = shield_emblem_sdf(q, 0.064);
    let intact_fill = (1.0 - smoothstep(-0.003, 0.004, sd_intact)) * intact_env;

    let crack_env = smoothstep(0.0, 0.08, t) * (1.0 - smoothstep(0.25, 0.48, t));
    let crack_d = min(
        vfx_segment(q, vec2<f32>(-0.040, -0.030), vec2<f32>(0.015, 0.010)),
        vfx_segment(q, vec2<f32>(0.015, 0.010), vec2<f32>(0.035, 0.050)),
    );
    let crack = exp(-(crack_d * crack_d) / (2.0 * 0.005 * 0.005)) * crack_env;

    // 2. 3 outward-flying shield shards (left, top, bottom) starting at t = 0.25.
    let split_t = clamp((t - 0.24) / 0.76, 0.0, 1.0);
    let split_env = smoothstep(0.0, 0.12, split_t) * (1.0 - smoothstep(0.45, 1.0, split_t));
    let fly = 0.16 * pow(split_t, 0.70);
    let s_l = shield_emblem_sdf(q - vec2<f32>(-fly, -0.02 * split_t), 0.038);
    let s_t = shield_emblem_sdf(q - vec2<f32>(0.02 * split_t, -fly), 0.038);
    let s_b = shield_emblem_sdf(q - vec2<f32>(0.03 * split_t, fly), 0.036);
    let shards = (
        (1.0 - smoothstep(-0.003, 0.004, s_l))
        + (1.0 - smoothstep(-0.003, 0.004, s_t))
        + (1.0 - smoothstep(-0.003, 0.004, s_b))
    ) * split_env;

    let bits = counter_sparks(q, split_t, seed, 6, 0.24) * step(0.20, t);

    let steel_blue = vec3<f32>(0.15, 0.42, 0.61);
    let ice_white = vec3<f32>(0.90, 0.96, 1.00);
    let body_alpha = clamp((intact_fill * 0.78 + shards * 0.72) * edge, 0.0, 0.90) * tint.a;
    let body_rgb = mix(steel_blue, tint.rgb, 0.35) * body_alpha;
    let add_rgb = (ice_white * (crack * 1.35 + bits * 0.75) + tint.rgb * (shards * 0.45))
        * edge * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}
