// Procedural Enhanced-tier luminous line ribbons and faceted spearheads (targeting arrows,
// live cursor aims, combat attack/block beams, and ability-to-source connections). Clean-room
// WGSL replicating Arena's layered scrolling-plasma ribbon + elongated 3D-faceted spearhead
// technique with zero texture samples.

const VFX_LINE_RIBBON: u32 = 3u;
const VFX_LINE_HEAD: u32 = 4u;

const LINE_STYLE_AIM: u32 = 0u;
const LINE_STYLE_FRIENDLY: u32 = 1u;
const LINE_STYLE_HOSTILE: u32 = 2u;
const LINE_STYLE_ATTACK: u32 = 3u;
const LINE_STYLE_BLOCK: u32 = 4u;
const LINE_STYLE_SOURCE: u32 = 5u;

// Must match `kit_frame::frame::vfx::LINE_HEAD_PAD`.
const LINE_HEAD_PAD_X: f32 = 0.55;
const LINE_HEAD_PAD_Y: f32 = 0.95;

struct LinePalette {
    outer: vec3<f32>,
    mid: vec3<f32>,
    core: vec3<f32>,
    intensity: f32,
    bloom: f32,
    speed: f32,
};

fn line_palette(style: u32) -> LinePalette {
    switch style {
        case LINE_STYLE_FRIENDLY: {
            return LinePalette(
                vec3<f32>(0.02, 0.56, 0.22),
                vec3<f32>(0.10, 0.96, 0.44),
                vec3<f32>(0.88, 1.00, 0.92),
                1.04,
                0.42,
                1.50,
            );
        }
        case LINE_STYLE_HOSTILE: {
            return LinePalette(
                vec3<f32>(0.86, 0.05, 0.06),
                vec3<f32>(1.00, 0.22, 0.14),
                vec3<f32>(1.00, 0.92, 0.86),
                1.06,
                0.46,
                1.50,
            );
        }
        case LINE_STYLE_ATTACK: {
            // Arena confirmed attack-to-planeswalker rose-crimson laser beam.
            return LinePalette(
                vec3<f32>(0.88, 0.06, 0.16),
                vec3<f32>(1.00, 0.24, 0.34),
                vec3<f32>(1.00, 0.94, 0.96),
                1.05,
                0.44,
                1.75,
            );
        }
        case LINE_STYLE_BLOCK: {
            // Arena confirmed blocker solar-gold beam, bloom-calibrated for shield/chevron overlap.
            return LinePalette(
                vec3<f32>(0.86, 0.50, 0.04),
                vec3<f32>(1.00, 0.82, 0.20),
                vec3<f32>(1.00, 0.98, 0.84),
                0.96,
                0.28,
                1.60,
            );
        }
        case LINE_STYLE_SOURCE: {
            // Arena faint cool pale teal-silver ability-to-parent connection.
            return LinePalette(
                vec3<f32>(0.24, 0.64, 0.56),
                vec3<f32>(0.56, 0.90, 0.82),
                vec3<f32>(0.92, 0.99, 0.97),
                0.58,
                0.15,
                1.05,
            );
        }
        default: {
            // LINE_STYLE_AIM: Arena electric cyan-blue targeting energy.
            return LinePalette(
                vec3<f32>(0.00, 0.44, 0.98),
                vec3<f32>(0.10, 0.88, 1.00),
                vec3<f32>(0.88, 0.99, 1.00),
                1.08,
                0.48,
                1.55,
            );
        }
    }
}

fn line_bloom_gain(style: u32) -> f32 {
    return line_palette(style).bloom;
}

// Luminous curved or straight line ribbon shaded along continuous arc-length coordinates
// (`v = mix(params.y, params.z, mesh_uv.x)`). Combines a crisp hot centerline, a saturated
// premultiplied plasma core-body that holds contrast over bright cloth, and dual-layer
// forward-scrolling procedural energy wisps.
fn shade_line_ribbon(
    mesh_uv: vec2<f32>,
    fade: f32,
    style: u32,
    params: vec4<f32>,
) -> vec4<f32> {
    let time_s = params.x;
    let v = clamp(mix(params.y, params.z, mesh_uv.x), 0.0, 1.0);
    let u = mesh_uv.y;
    let d = abs(u - 0.5) * 2.0;
    let len_ratio = max(params.w, 1.0);

    let pal = line_palette(style);
    let is_beam = style == LINE_STYLE_ATTACK || style == LINE_STYLE_BLOCK;
    let is_source = style == LINE_STYLE_SOURCE;

    // Lengthwise intensity envelope:
    // - Targeting / aim rises smoothly from tail into a bright plasma arch, tucking into the
    //   spearhead right at the tip so there is no dim pinch neck behind the head.
    // - Combat beams enter quickly from the creature and stay crisp all along the shaft.
    // - Source lines emerge softly from the permanent and carry a legible cool silver thread.
    var env: f32;
    if (is_beam) {
        let enter = smoothstep(0.0, 0.05, v);
        let tuck = 1.0 - smoothstep(0.95, 1.0, v);
        env = enter * (0.84 + 0.16 * v) * tuck;
    } else if (is_source) {
        let enter = smoothstep(0.0, 0.14, v);
        let tuck = 1.0 - smoothstep(0.94, 1.0, v);
        env = enter * (0.38 + 0.62 * pow(v, 0.75)) * tuck;
    } else {
        let enter = smoothstep(0.0, 0.07, v);
        let rise = 0.42 + 0.58 * pow(v, 0.80);
        let tuck = 1.0 - smoothstep(0.94, 1.0, v);
        env = enter * rise * tuck;
    }

    // Dual-layer forward-scrolling procedural plasma wisps along continuous arc length.
    let along_tile = v * max(len_ratio * 0.28, 1.8);
    let n1 = vfx_noise(vec2<f32>(
        u * 2.4 - 0.22 * time_s,
        along_tile * 1.45 - pal.speed * 0.72 * time_s,
    ));
    let n2 = vfx_noise(vec2<f32>(
        u * 4.6 + 0.15 * time_s,
        along_tile * 3.10 - pal.speed * 1.55 * time_s,
    ));
    let wisp = smoothstep(0.18, 0.84, n1 * 0.52 + n2 * 0.48);

    // Tight crisp centerline inside a saturated plasma body and controlled outer glow halo.
    let q_edge = 1.0 - smoothstep(0.74, 1.0, d);
    let core_w = select(select(0.078, 0.062, is_beam), 0.055, is_source);
    let body_w = select(select(0.185, 0.140, is_beam), 0.125, is_source);
    let halo_w = select(select(0.28, 0.23, is_beam), 0.20, is_source);

    let core_x = exp(-(d * d) / (2.0 * core_w * core_w));
    let body_x = exp(-(d * d) / (2.0 * body_w * body_w)) * q_edge;
    let halo_x = exp(-d / halo_w) * q_edge;

    let energy = 0.66 + 0.68 * wisp;
    let strength = env * pal.intensity * fade;

    // Premultiplied translucent plasma body keeps colors vivid and rich over tan cloth.
    let body_alpha = clamp(body_x * strength * (0.64 + 0.26 * wisp), 0.0, 0.90);
    let body_tint = mix(pal.outer, pal.mid, clamp(body_x * 0.68 + wisp * 0.32, 0.0, 1.0));
    let body_rgb = body_tint * body_alpha * (1.02 + 0.38 * wisp);

    // Additive outer aura, flowing plasma filaments, and blazing white-tinted centerline.
    let add_rgb = (
        pal.outer * halo_x * (0.50 * energy)
        + pal.mid * body_x * (0.98 * energy)
        + pal.core * core_x * (1.62 * (0.82 + 0.36 * wisp))
    ) * strength;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}

// Faceted luminous spearhead seated at the tip of an Enhanced line. Uses an isotropic diamond
// kite SDF (sharp tip at +x, swept wings at -0.36 of half-length, tapered rear bevel joining
// the ribbon), a blazing 3D central ridge, crisp edge bevels, and a soft outer bloom aura.
fn shade_line_head(
    mesh_uv: vec2<f32>,
    fade: f32,
    style: u32,
    params: vec4<f32>,
) -> vec4<f32> {
    let time_s = params.x;
    let aspect = max(params.w, 1.0);
    let pal = line_palette(style);

    // Map mesh_uv into normalized spearhead coordinates where the head spans [-1, 1] x [-1, 1].
    let hx = (mesh_uv.x - 0.5) * 2.0 * (1.0 + LINE_HEAD_PAD_X);
    let hy = (mesh_uv.y - 0.5) * 2.0 * (1.0 + LINE_HEAD_PAD_Y);
    let ay = abs(hy);

    // Isotropic coordinates (units of spearhead width) so rim and halo thickness stay uniform.
    let half_l = 0.5 * aspect;
    let half_w = 0.5;
    let wing_x = -0.36 * half_l;
    let p = vec2<f32>(hx * half_l, ay * half_w);

    let tip = vec2<f32>(half_l, 0.0);
    let wing = vec2<f32>(wing_x, half_w);
    let rear = vec2<f32>(-half_l, 0.0);

    let d_front = vfx_segment(p, tip, wing);
    let d_rear = vfx_segment(p, wing, rear);
    let d_edge = min(d_front, d_rear);

    // Analytic inside mask with crisp anti-aliased boundary.
    var edge_y: f32;
    if (p.x >= wing_x) {
        edge_y = half_w * (half_l - p.x) / max(half_l - wing_x, 1e-4);
    } else {
        edge_y = half_w * (p.x + half_l) / max(wing_x + half_l, 1e-4);
    }
    let signed_y = p.y - edge_y;
    let in_x = step(-half_l - 0.02, p.x) * step(p.x, half_l + 0.02);
    let inside = smoothstep(0.028, -0.020, signed_y) * in_x;

    // Fade cleanly before the padded quad edges.
    let q_edge = (1.0 - smoothstep(0.76, 1.0, abs(mesh_uv.x * 2.0 - 1.0)))
        * (1.0 - smoothstep(0.76, 1.0, abs(mesh_uv.y * 2.0 - 1.0)));

    // Subtle forward-flowing energy shimmer across the faceted spearhead.
    let shimmer = vfx_noise(vec2<f32>(hx * 2.8 - pal.speed * 1.25 * time_s, hy * 3.2));
    let pulse = 0.88 + 0.24 * shimmer;

    // 3D faceted central ridge: blazing white-tinted spine and sharp luminous bevel edges.
    let ridge = exp(-(p.y * p.y) / (2.0 * 0.16 * 0.16)) * inside;
    let tip_hot = exp(-length(p - tip) / (0.50 * half_l)) * inside;
    let rim = exp(-(d_edge * d_edge) / (2.0 * 0.060 * 0.060)) * q_edge;
    let outer_glow = exp(-d_edge / 0.26) * q_edge;

    let strength = pal.intensity * fade;

    // Solid premultiplied facet body keeps the spearhead razor-sharp over bright card art.
    let body_alpha = clamp(inside * 0.88 * strength, 0.0, 0.94);
    let facet_col = mix(pal.outer, pal.mid, clamp(0.50 + 0.32 * ridge + 0.18 * shimmer, 0.0, 1.0));
    let body_rgb = facet_col * body_alpha * (1.05 + 0.28 * shimmer);

    // Additive radiant spine, sharp bevel rim, tip focus, and soft surrounding aura.
    let add_rgb = (
        pal.outer * outer_glow * (0.54 * pulse)
        + pal.mid * rim * (0.96 * pulse)
        + pal.core * ridge * (1.42 * pulse)
        + pal.core * tip_hot * (0.72 * pulse)
    ) * strength;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}
