// Procedural Enhanced-tier permanent landing dust (`Vfx::Settle`):
// warm yellow collision flash with starburst rays, soft expanding dust cloud, wispy shockwave
// ring, and dark ground shadow (`vfx_card_dust_sm`, `_med`, `_lg`, and `_flying_*`).
// Zero texture samples; smooth quad-edge feathering.

const VFX_SETTLE: u32 = 11u;

const BLOOM_GAIN_SETTLE: f32 = 0.36;

// `style` encodes `SettleKind`:
//   0 = small (MV <= 3), 1 = medium (MV 4-5), 2 = large (MV 6+),
//   3 = flying small,     4 = flying medium,  5 = flying large.
fn shade_settle(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
    style: u32,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let pol = vfx_polar(p);
    let dist = pol.y;
    let angle = pol.x;
    let edge = vfx_quad_edge(uv, 0.84);

    let is_flying = style >= 3u;
    let size_tier = style % 3u;
    let tier_f = f32(size_tier);
    let size_boost = 1.0 + 0.10 * tier_f;

    // Card-shaped elliptical distance so ground shadow and dust hug the card footprint.
    let card_d = length(p * vec2<f32>(1.0, 0.74));

    // 1. Dark ground shadow (`mat_vfx_glow_a01`: (57, 51, 45) ground, cooler for flying).
    let shadow_r = mix(0.18, 0.42 * size_boost, pow(t, 0.45));
    let shadow_env = smoothstep(0.0, 0.14, t) * (1.0 - smoothstep(0.40, 1.0, t));
    let shadow = exp(-(card_d * card_d) / (2.0 * shadow_r * shadow_r)) * shadow_env;
    let shadow_col = select(vec3<f32>(0.224, 0.200, 0.176), vec3<f32>(0.26, 0.28, 0.35), is_flying);

    // 2. Warm yellow-white collision flash + starburst rays (`MAT_CollisionFlash`).
    let flash_env = smoothstep(0.0, 0.06, t) * (1.0 - smoothstep(0.12, 0.42, t));
    let flash_core = exp(-(card_d * card_d) / (2.0 * (0.14 * size_boost) * (0.14 * size_boost)));
    let ray_a = pow(abs(cos(angle * 4.0 + seed * 6.2831853)), 8.0);
    let ray_b = pow(abs(cos(angle * 7.0 - seed * 3.1415927)), 12.0);
    let ray_r = exp(-dist / (0.24 * size_boost)) * (1.0 - exp(-dist / 0.03));
    let rays = (ray_a * 0.75 + ray_b * 0.55) * ray_r;
    let flash = (flash_core * 0.85 + rays * (0.9 + 0.25 * tier_f)) * flash_env;

    // 3. Soft warm dust cloud (`mat_vfx_cloud_01`, plus scrolling cloud for large).
    let cloud_r = mix(0.14, 0.34 * size_boost, pow(t, 0.55));
    let cloud_band = exp(-pow(card_d - cloud_r * 0.78, 2.0) / (2.0 * 0.13 * 0.13));
    let cloud_n = vfx_fbm(p * 5.2 + vec2<f32>(seed * 11.0, -t * 0.65));
    let cloud_env = smoothstep(0.04, 0.20, t) * (1.0 - smoothstep(0.45, 1.0, t));
    var cloud = cloud_band * smoothstep(0.28, 0.76, cloud_n) * cloud_env;
    if (size_tier == 2u) {
        let scroll_n = vfx_fbm(p * 7.2 + vec2<f32>(-t * 0.95, t * 1.35 + seed * 5.0));
        cloud += cloud_band * smoothstep(0.36, 0.80, scroll_n) * cloud_env * 0.55;
    }

    // 4. Wispy shockwave ring (`mat_vfx_ring_wisp_wave_02`, polar-distorted for large).
    let ring_t = clamp((t - 0.07) / 0.74, 0.0, 1.0);
    let ring_r = mix(0.15, 0.40 * size_boost, pow(ring_t, 0.52));
    let wisp_uv = vec2<f32>(cos(angle) * 2.4, sin(angle) * 2.4) + vec2<f32>(seed * 7.3, ring_t * 1.4);
    let ring_wisp = smoothstep(0.25, 0.75, vfx_fbm(wisp_uv));
    let ring_env = smoothstep(0.0, 0.12, ring_t) * (1.0 - smoothstep(0.32, 1.0, ring_t));
    var ring = exp(-pow(dist - ring_r, 2.0) / (2.0 * 0.024 * 0.024)) * (0.45 + 0.75 * ring_wisp) * ring_env;
    if (size_tier == 2u) {
        let polar_n = vfx_fbm(vec2<f32>(angle * 2.2 + seed * 4.0, dist * 8.0 - ring_t * 2.2));
        let ring2_r = ring_r * 0.84;
        let ring2 = exp(-pow(dist - ring2_r, 2.0) / (2.0 * 0.020 * 0.020)) * smoothstep(0.30, 0.78, polar_n) * ring_env;
        ring = max(ring, ring2 * 0.9);
    }

    // 5. Flying wind-swirl wisps and short trail (`Mat_VFX_Wind_Swoosh_2`).
    var wind = 0.0;
    if (is_flying) {
        let swirl_a = angle + dist * 4.2 - t * 3.8 + seed * 6.2831853;
        let swirl_n = vfx_fbm(vec2<f32>(cos(swirl_a) * 2.2, sin(swirl_a) * 2.2) + p * 3.5);
        let wind_band = exp(-pow(dist - mix(0.16, 0.42, t), 2.0) / (2.0 * 0.11 * 0.11));
        let wind_env = smoothstep(0.03, 0.18, t) * (1.0 - smoothstep(0.38, 0.92, t));
        wind = wind_band * smoothstep(0.38, 0.78, swirl_n) * wind_env;
    }

    let dust_col = tint.rgb;
    let flash_col = vec3<f32>(1.0, 0.94, 0.52);
    let wind_col = vec3<f32>(0.76, 0.86, 1.0);

    let body_alpha = clamp((shadow * 0.52 + cloud * 0.58 + ring * 0.32 + wind * 0.28) * edge, 0.0, 0.88) * tint.a;
    let body_rgb = (shadow_col * (shadow * 0.52) + dust_col * (cloud * 0.58 + ring * 0.32) + wind_col * (wind * 0.28))
        * edge * tint.a;
    let add_rgb = (flash_col * flash * 0.95 + dust_col * (cloud * 0.30 + ring * 0.68) + wind_col * (wind * 0.55))
        * edge * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}
