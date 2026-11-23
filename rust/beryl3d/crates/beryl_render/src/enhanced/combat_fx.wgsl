// Procedural Enhanced-tier combat visuals: attacker chevron (unblocked crimson-ember / blocked
// solar-gold) and blocker golden energy shield dome. Clean-room WGSL using analytic SDFs,
// multi-octave value noise, premultiplied translucent plasma/curtain bodies, additive luminous
// rims, and zero texture samples.

const VFX_CHEVRON: u32 = 0u;
const VFX_CHEVRON_BLOCKED: u32 = 1u;
const VFX_SHIELD_DOME: u32 = 2u;

// Quad padding around the attacker chevron, in units of its base width.
// Must match `kit_frame::frame::vfx::CHEVRON_PAD`.
const CHEVRON_PAD_X: f32 = 0.20;
const CHEVRON_PAD_BOT: f32 = 0.05;
const CHEVRON_PAD_TOP: f32 = 0.24;

// Quad padding around the blocker shield dome, in units of its span width.
// Must match `kit_frame::frame::vfx::SHIELD_PAD`.
const SHIELD_PAD_X: f32 = 0.14;
const SHIELD_PAD_BOT: f32 = 0.06;
const SHIELD_PAD_TOP: f32 = 0.20;

// Calibrated bloom emission gains for the additive bloom pass.
const BLOOM_GAIN_UNBLOCKED: f32 = 0.58;
const BLOOM_GAIN_GOLDEN: f32 = 0.36;

fn vfx_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let d = dot(q, q.yx + vec2<f32>(33.33));
    return fract((q.x + q.y) * d);
}

fn vfx_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (vec2<f32>(3.0) - 2.0 * f);
    let a = vfx_hash(i);
    let b = vfx_hash(i + vec2<f32>(1.0, 0.0));
    let c = vfx_hash(i + vec2<f32>(0.0, 1.0));
    let d = vfx_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Smooth 3-octave fractal noise for plasma wisps and shield swirl.
fn vfx_fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.55;
    var q = p;
    for (var i = 0; i < 3; i++) {
        sum += amp * vfx_noise(q);
        q = vec2<f32>(q.x * 1.85 - q.y * 0.72, q.x * 0.72 + q.y * 1.85) + vec2<f32>(11.3, 7.1);
        amp *= 0.5;
    }
    return sum;
}

// Euclidean distance from `p` to the line segment `a`..`b`.
fn vfx_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let t = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * t);
}

// Overlay channel blend: warm midtone lift with a crisp bright crest.
fn vfx_overlay(base: vec3<f32>, top: vec3<f32>) -> vec3<f32> {
    let hi = vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - top) * (vec3<f32>(1.0) - base);
    let lo = 2.0 * base * top;
    return select(lo, hi, top > vec3<f32>(0.5));
}

// Attacker chevron: sharp V-rim with white-hot bevel, deep translucent plasma interior streaming
// toward the apex, and controlled outer fiery halo. `blocked` shifts crimson-orange to warm gold.
fn shade_attacker_chevron(
    mesh_uv: vec2<f32>,
    fade: f32,
    time_s: f32,
    seed: f32,
    aspect: f32,
    blocked: bool,
) -> vec4<f32> {
    let quad_w = 1.0 + 2.0 * CHEVRON_PAD_X;
    let quad_h = aspect + CHEVRON_PAD_BOT + CHEVRON_PAD_TOP;
    let px = (mesh_uv.x - 0.5) * quad_w;
    let py = (1.0 - mesh_uv.y) * quad_h - CHEVRON_PAD_BOT;

    let half_w = 0.5;
    let high = max(aspect, 0.08);
    let ax = abs(px);
    let edge_y = high * (1.0 - ax / half_w);
    let d_rim = vfx_segment(vec2<f32>(ax, py), vec2<f32>(half_w, 0.0), vec2<f32>(0.0, high));

    // Dual-layer distorted plasma coordinates: slow distortion warps outward-streaming wisps.
    let t = time_s + seed * 19.7;
    let dist_uv = vec2<f32>(px * 2.8, py * 2.8) + vec2<f32>(-0.25, -0.10) * t * 1.35;
    let warp = vec2<f32>(
        vfx_noise(dist_uv) - 0.5,
        vfx_noise(dist_uv + vec2<f32>(5.7, 13.1)) - 0.5,
    );
    let plasma_uv = vec2<f32>(px * 9.5, py * 3.2 - t * 1.15) + warp * 0.68;
    let streaks = vfx_fbm(plasma_uv);
    let fine = vfx_noise(vec2<f32>(px * 18.0, py * 6.5 - t * 1.85) + warp * 0.45);
    let wisp = smoothstep(0.24, 0.82, streaks * 0.72 + fine * 0.28);

    // Smooth fade before quad boundary so outer halo never clips.
    let q_edge = (1.0 - smoothstep(0.80, 1.0, abs(mesh_uv.x * 2.0 - 1.0)))
        * (1.0 - smoothstep(0.80, 1.0, abs(mesh_uv.y * 2.0 - 1.0)));

    // Taper smoothly toward the outer corners of the card edge.
    let arm = 0.42 + 0.58 * (1.0 - smoothstep(0.20, 0.50, ax));
    // Clean bevel at card base (py == 0) so plasma starts crisply past the card frame.
    let above_base = smoothstep(0.0, 0.028, py);
    let inside_mask = smoothstep(-0.008, 0.010, edge_y - py)
        * smoothstep(0.0, 0.018, half_w - ax + 0.01)
        * above_base;

    // Translucent fiery body: deep saturated ember near base, rising in heat toward the V-edges.
    let rise = clamp(py / max(edge_y, 0.015), 0.0, 1.0);
    let body_grad = pow(rise, 0.72) * 0.70 + 0.30 * smoothstep(0.0, 0.35 * high, py);
    let body_shape = inside_mask * body_grad * arm * q_edge;

    // Crisp V-rim along the two slanted edges with a bright apex focus.
    let apex = exp(-length(vec2<f32>(px, py - high)) / (high * 0.55));
    let rim_w = 0.014 + 0.005 * apex;
    let core_w = 0.0065 + 0.0025 * apex;
    let rim_band = exp(-(d_rim * d_rim) / (2.0 * rim_w * rim_w)) * arm * above_base * q_edge;
    let core_line = exp(-(d_rim * d_rim) / (2.0 * core_w * core_w))
        * (0.72 + 0.38 * apex)
        * arm
        * above_base
        * q_edge;
    let inner_bevel = exp(-d_rim / 0.036) * inside_mask * arm * q_edge;

    // Controlled outer halo past the V-edges, tighter for blocked gold beneath blocking arrows.
    let halo_reach = select(0.068, 0.052, blocked);
    let outer_halo = exp(-d_rim / (halo_reach + 0.018 * apex))
        * arm
        * smoothstep(-0.02, 0.022, py)
        * q_edge;

    let flicker = 0.90 + 0.18 * wisp;

    // Unblocked: deep crimson-red base -> vivid vermilion plasma -> warm white-peach V-rim.
    // Blocked: deep burnished amber base -> rich solar gold -> warm ivory-gold V-rim.
    let deep_col = select(vec3<f32>(0.64, 0.04, 0.01), vec3<f32>(0.62, 0.36, 0.03), blocked);
    let mid_col = select(vec3<f32>(0.98, 0.20, 0.04), vec3<f32>(0.92, 0.68, 0.12), blocked);
    let hot_col = select(vec3<f32>(1.0, 0.54, 0.18), vec3<f32>(0.98, 0.85, 0.34), blocked);
    let core_col = select(vec3<f32>(1.0, 0.94, 0.84), vec3<f32>(1.0, 0.96, 0.78), blocked);

    // Premultiplied translucent plasma body keeps deep crimson/amber saturated over tan cloth.
    let body_tint = mix(deep_col, mid_col, clamp(rise * 0.52 + wisp * 0.58, 0.0, 1.0));
    let body_alpha = clamp(body_shape * (0.55 + 0.34 * wisp), 0.0, 0.86) * fade;
    let body_rgb = body_tint * body_alpha * (0.76 + 0.68 * wisp);

    // Calmer additive gain for blocked gold so the chevron stays readable beneath arrows.
    let add_gain = select(1.0, 0.72, blocked);
    let add_rgb = (
        mid_col * outer_halo * (0.48 * flicker)
        + mix(mid_col, hot_col, wisp) * inner_bevel * (0.56 * (0.65 + 0.55 * wisp))
        + hot_col * rim_band * (0.88 * flicker)
        + core_col * core_line * (1.18 * flicker)
        + core_col * (apex * rim_band * 0.38)
    ) * (fade * add_gain);

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}

// Blocker shield dome: compact arched golden forcefield barrier with a translucent polar-swirling
// energy curtain beneath a slender warm-gold crest ridge.
fn shade_blocker_shield(
    mesh_uv: vec2<f32>,
    fade: f32,
    time_s: f32,
    seed: f32,
    aspect: f32,
) -> vec4<f32> {
    let quad_w = 1.0 + 2.0 * SHIELD_PAD_X;
    let quad_h = aspect + SHIELD_PAD_BOT + SHIELD_PAD_TOP;
    let px = (mesh_uv.x - 0.5) * quad_w;
    let py = (1.0 - mesh_uv.y) * quad_h - SHIELD_PAD_BOT;

    let dome_h = max(aspect, 0.10);
    // Gentle dome arch spanning x in [-0.5, 0.5]: highest at center, tapering near card corners.
    let nx = px * 2.0;
    let arch_y = dome_h * (1.0 - 0.82 * nx * nx);
    let slope = -dome_h * 0.82 * 4.0 * px;
    let inv_slope = inverseSqrt(1.0 + slope * slope);
    let d_signed = (py - arch_y) * inv_slope;
    let d_arch = abs(d_signed);

    // Smooth feathering toward the left and right wings keeps adjacent blockers separated.
    let span = clamp(1.0 - nx * nx, 0.0, 1.0);
    let wings = pow(span, 0.75);
    let center_boost = 0.58 + 0.42 * pow(span, 1.3);
    let q_edge = (1.0 - smoothstep(0.82, 1.0, abs(mesh_uv.x * 2.0 - 1.0)))
        * (1.0 - smoothstep(0.82, 1.0, abs(mesh_uv.y * 2.0 - 1.0)));

    // Polar-scrolling energy swirl centered beneath the card edge.
    let t = time_s + seed * 13.9;
    let origin = vec2<f32>(0.0, -0.78 * dome_h);
    let rel = vec2<f32>(px, py) - origin;
    let angle = atan2(rel.x, max(rel.y, 0.01));
    let radius = length(rel);
    let dist_sample = vfx_noise(vec2<f32>(angle * 3.8, radius * 4.2 - t * 0.55)) - 0.5;
    let swirl = vfx_fbm(vec2<f32>(
        angle * 8.0 + t * 0.22 + dist_sample * 0.6,
        radius * 5.6 - t * 0.40,
    ));
    let shimmer = vfx_noise(vec2<f32>(angle * 15.0 - t * 0.45, radius * 9.5 - t * 0.7));
    let plasma = smoothstep(0.22, 0.82, swirl * 0.74 + shimmer * 0.26);

    // Translucent golden forcefield curtain filling the arch between card edge and crest.
    let inside_dome = smoothstep(0.010, -0.014, d_signed) * smoothstep(0.0, 0.14 * dome_h, py);
    let under_crest = exp(min(d_signed, 0.0) / (0.60 * dome_h));
    let curtain_mask = inside_dome * (0.34 + 0.66 * under_crest) * wings * q_edge;

    // Slender, luminous golden-cream crest ridge.
    let rim_w = 0.0145 + 0.0055 * span;
    let crest_field = exp(-(d_arch * d_arch) / (2.0 * rim_w * rim_w)) * wings * q_edge;
    let core_w = 0.0062 + 0.002 * span;
    let core_ridge =
        exp(-(d_arch * d_arch) / (2.0 * core_w * core_w)) * wings * center_boost * q_edge;

    let deep_gold = vec3<f32>(0.72, 0.48, 0.06);
    let warm_gold = vec3<f32>(0.97, 0.78, 0.22);
    let cream_gold = vec3<f32>(1.0, 0.96, 0.78);

    let lifted = pow(clamp(crest_field, 1e-4, 1.0), 0.48) * warm_gold;
    let over = clamp(vfx_overlay(lifted, cream_gold), vec3<f32>(0.0), vec3<f32>(1.0));
    let crest_col = mix(lifted, over, pow(clamp(crest_field, 1e-4, 1.0), 1.38));

    // Soft outer golden aura just past the convex crest.
    let outer_halo = exp(-max(d_signed, 0.0) / 0.052)
        * smoothstep(-0.035, 0.008, d_signed)
        * wings
        * q_edge;

    // Premultiplied translucent golden dome belly keeps amber-gold rich over light cloth.
    let curtain_alpha = clamp(curtain_mask * (0.40 + 0.30 * plasma), 0.0, 0.66) * fade;
    let curtain_rgb =
        mix(deep_gold, warm_gold, clamp(under_crest * 0.60 + plasma * 0.48, 0.0, 1.0))
            * curtain_alpha
            * (0.88 + 0.58 * plasma);

    // Additive radiant crest and gentle golden outer glow.
    let pulse = 0.92 + 0.16 * plasma;
    let add_rgb = (
        warm_gold * outer_halo * (0.38 * pulse)
        + crest_col * crest_field * (0.72 * pulse)
        + cream_gold * core_ridge * (0.86 * pulse)
        + warm_gold * curtain_mask * under_crest * (0.32 * (0.7 + 0.6 * plasma))
    ) * fade;

    return vec4<f32>(curtain_rgb + add_rgb, curtain_alpha);
}
