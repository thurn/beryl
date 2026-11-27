// Procedural Enhanced-tier mythic rare arrival glow (`Vfx::MythicArrival`,
// `VFX_ETB_Mythic_Tier2_LUT`): wide soft color-tinted glow, sharp starburst diamond flare,
// smoldering edge residue, and dual expanding shockwave rings. Zero texture samples.

const VFX_MYTHIC_ARRIVAL: u32 = 14u;

const BLOOM_GAIN_MYTHIC_ARRIVAL: f32 = 0.28;

fn shade_mythic_arrival(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let pol = vfx_polar(p);
    let angle = pol.x;
    let dist = pol.y;
    let edge = vfx_quad_edge(uv, 0.80);
    let card_d = length(p * vec2<f32>(1.0, 0.74));

    // 1. Wide soft color-tinted aura (`glow`).
    let glow_r = mix(0.16, 0.36, pow(t, 0.45));
    let glow_env = smoothstep(0.0, 0.10, t) * (1.0 - smoothstep(0.30, 1.0, t));
    let glow = exp(-(card_d * card_d) / (2.0 * glow_r * glow_r)) * glow_env;

    // 2. Sharp starburst flare and diamond cross (`flare` + `flare_graphic_lg`).
    let flare_t = clamp((t - 0.06) / 0.36, 0.0, 1.0);
    let flare_env = smoothstep(0.0, 0.14, flare_t) * (1.0 - smoothstep(0.28, 1.0, flare_t));
    let core = exp(-(dist * dist) / (2.0 * 0.065 * 0.065));
    let cross_x = exp(-abs(p.y) / (0.012 + 0.05 * dist)) * exp(-abs(p.x) / 0.36);
    let cross_y = exp(-abs(p.x) / (0.012 + 0.05 * dist)) * exp(-abs(p.y) / 0.28);
    let diag_rays = pow(abs(cos(angle * 4.0 + 0.785398)), 14.0) * exp(-dist / 0.28);
    let flare = (core * 1.15 + (cross_x + cross_y) * 0.85 + diag_rays * 0.75) * flare_env;

    // 3. Smoldering residue around the card perimeter (`smk`).
    let smk_t = clamp((t - 0.10) / 0.90, 0.0, 1.0);
    let smk_band = exp(-pow(card_d - mix(0.18, 0.25, smk_t), 2.0) / (2.0 * 0.08 * 0.08));
    let smk_n = vfx_fbm(p * 6.5 + vec2<f32>(seed * 9.7 - smk_t * 0.25, -smk_t * 0.45));
    let smk_env = smoothstep(0.0, 0.18, smk_t) * (1.0 - smoothstep(0.45, 1.0, smk_t));
    let smolder = smk_band * smoothstep(0.32, 0.78, smk_n) * smk_env;

    // 4. Dual sharp expanding shockwave rings (`rings` + `rings_full`).
    let r1_t = clamp((t - 0.08) / 0.48, 0.0, 1.0);
    let r1_r = mix(0.12, 0.42, pow(r1_t, 0.55));
    let r1_env = smoothstep(0.0, 0.12, r1_t) * (1.0 - smoothstep(0.35, 1.0, r1_t));
    let ray_break = 0.55 + 0.45 * pow(abs(cos(angle * 10.0 + seed * 6.2831853)), 4.0);
    let ring1 = exp(-pow(dist - r1_r, 2.0) / (2.0 * 0.016 * 0.016)) * ray_break * r1_env;

    let r2_t = clamp((t - 0.16) / 0.52, 0.0, 1.0);
    let r2_r = mix(0.14, 0.46, pow(r2_t, 0.55));
    let r2_env = smoothstep(0.0, 0.12, r2_t) * (1.0 - smoothstep(0.38, 1.0, r2_t));
    let ring2 = exp(-pow(dist - r2_r, 2.0) / (2.0 * 0.014 * 0.014)) * r2_env;
    let rings = ring1 * 0.95 + ring2 * 0.70;

    let base_col = tint.rgb;
    let hot_col = mix(base_col, vec3<f32>(1.0, 0.98, 0.92), 0.72);
    let smolder_col = mix(base_col, vec3<f32>(1.0, 0.72, 0.32), 0.25);

    let rgb = (base_col * (glow * 0.75 + rings * 0.85)
        + hot_col * flare * 1.10
        + smolder_col * smolder * 0.70)
        * edge * tint.a;
    let alpha = clamp((glow * 0.42 + flare * 0.55 + smolder * 0.45 + rings * 0.35) * edge, 0.0, 0.88)
        * tint.a;

    return vec4<f32>(rgb, alpha);
}
