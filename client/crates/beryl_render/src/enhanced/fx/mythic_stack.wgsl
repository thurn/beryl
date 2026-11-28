// Procedural Enhanced-tier aura round a mythic rare spell on the stack, and the burst it leaves
// as the spell leaves the stack. MTG Arena draws these from `VFX_Stack_Mythic_Tier2_loop_LUT`
// and `VFX_Stack_Mythic_Tier2_outro_LUT`, tinted through a color ramp by the card's colors.
//
// `Vfx::MythicStack` loops for as long as the spell is on the stack (`t` is a wrapped clock in
// seconds). Its layers are a card-shaped outline whose glow scrolls upward in wisps, slow rays,
// a chromatic shockwave that leaves the card's edge every few seconds, three layers of sparks
// rising past the edges, a four-point star flare that travels round the outline, and slow dust
// motes.
//
// `Vfx::MythicStackOutro` plays once as the spell leaves (`t` is its progress, 0 to 1): the
// outline flares, a card-shaped shockwave and a ring of rays run outward, a star flare bursts
// and sparks are thrown out.
//
// Both quads lie in the card's plane, padded past its edges by `MYTHIC_STACK_PAD` and
// `MYTHIC_STACK_OUTRO_PAD` card widths (`own.json`'s `mythic_stack`). The card face stays
// readable: inside the card only a thin lip of light is drawn. Zero texture samples.

const VFX_MYTHIC_STACK: u32 = 19u;
const VFX_MYTHIC_STACK_OUTRO: u32 = 20u;

// A spell on the stack sits beside the eligible highlights and the targeting arrows, so the loop
// blooms gently. The outro is short and may overlap the mythic arrival flash when the spell is a
// permanent, so it stays below the arrival's gain.
const BLOOM_GAIN_MYTHIC_STACK: f32 = 0.16;
const BLOOM_GAIN_MYTHIC_STACK_OUTRO: f32 = 0.22;

const MYTHIC_STACK_PAD: f32 = 0.22;
const MYTHIC_STACK_OUTRO_PAD: f32 = 0.55;
const MYTHIC_STACK_CORNER_R: f32 = 0.045;

// One layer of sparks on a grid of `scale` cells per card width, scrolled upward at `speed` card
// widths per second, kept within `reach` card widths outside the card's edge (`d`). Roughly one
// cell in `1 / density` holds a spark, and each spark twinkles on its own phase.
fn mythic_sparks(
    p: vec2<f32>,
    d: f32,
    t: f32,
    seed: f32,
    scale: f32,
    speed: f32,
    density: f32,
    reach: f32,
) -> f32 {
    let q = p * scale + vec2<f32>(seed * 17.0, t * speed * scale);
    let cell = floor(q);
    let h = vfx_hash(cell + vec2<f32>(seed * 31.0, 3.7));
    let jitter = vec2<f32>(vfx_hash(cell + vec2<f32>(5.1, 1.3)), vfx_hash(cell + vec2<f32>(2.9, 8.3)));
    let f = fract(q) - 0.25 - jitter * 0.5;
    let dot_r = 0.10 + 0.06 * h;
    let spark = exp(-dot(f, f) / (dot_r * dot_r));
    let present = step(1.0 - density, h);
    let twinkle = 0.5 + 0.5 * sin(t * (3.0 + 4.0 * h) + h * 40.0);
    let band = smoothstep(-0.01, 0.02, d) * (1.0 - smoothstep(reach * 0.4, reach, d));
    return spark * present * twinkle * band;
}

// A four-point star flare at `centre`, `size` card widths along its long rays.
fn mythic_star(p: vec2<f32>, centre: vec2<f32>, size: f32) -> f32 {
    let q = p - centre;
    let ax = abs(q.x);
    let ay = abs(q.y);
    let core = exp(-dot(q, q) / (size * size * 0.02));
    let rays = exp(-ay / (0.004 + 0.02 * ax)) * exp(-ax / size)
        + exp(-ax / (0.004 + 0.02 * ay)) * exp(-ay / size);
    return core + rays * 0.8;
}

// The point a share `s` of the way round the rounded rectangle with half-extents `half`,
// clockwise from the top-left corner.
fn mythic_perimeter(s: f32, half: vec2<f32>) -> vec2<f32> {
    let w = 2.0 * half.x;
    let h = 2.0 * half.y;
    let at = fract(s) * 2.0 * (w + h);
    if (at < w) {
        return vec2<f32>(-half.x + at, -half.y);
    }
    if (at < w + h) {
        return vec2<f32>(half.x, -half.y + (at - w));
    }
    if (at < 2.0 * w + h) {
        return vec2<f32>(half.x - (at - w - h), half.y);
    }
    return vec2<f32>(-half.x, half.y - (at - 2.0 * w - h));
}

fn shade_mythic_stack(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let frame = highlight_frame(uv, aspect, MYTHIC_STACK_PAD);
    let p = frame.xy;
    let half = frame.zw;
    let d = highlight_sd_box(p, half, MYTHIC_STACK_CORNER_R);
    let out_d = max(d, 0.0);
    let edge = vfx_quad_edge(uv, 0.72);
    let pol = vfx_polar(p);

    // 1. Card-shaped outline whose glow scrolls upward in wisps (`ramp` scrolled vertically).
    let wisp = vfx_fbm(vec2<f32>(p.x * 5.0, p.y * 3.2 + t * 0.55) + vec2<f32>(seed * 13.0, 0.0));
    let ramp = 0.55 + 0.45 * sin((p.y * 2.4 + t * 0.9 + seed) * 6.2831853);
    let glow_reach = 0.05 + 0.07 * wisp;
    let glow = exp(-out_d / glow_reach) * smoothstep(-0.03, 0.0, d) * (0.55 + 0.45 * ramp);
    let rim = exp(-(d * d) / (2.0 * 0.007 * 0.007));
    let lip = exp(d / 0.018) * (1.0 - step(0.0, d)) * 0.35;

    // 2. Slow rays leaving the outline, breathing.
    let ray_turn = t * 0.12 + seed * 6.2831853;
    let rays = pow(abs(cos(pol.x * 5.0 + ray_turn)), 28.0)
        * exp(-out_d / 0.09) * smoothstep(0.0, 0.03, d)
        * (0.6 + 0.4 * sin(t * 1.3 + seed * 9.0));

    // 3. Chromatic shockwave from the edge every 2.6 s, its red and blue split a little apart.
    let wave_t = fract(t / 2.6 + seed);
    let wave_r = wave_t * 0.16;
    let wave_fade = (1.0 - wave_t) * smoothstep(0.0, 0.08, wave_t);
    let wave_w = 2.0 * 0.010 * 0.010;
    let wave_g = exp(-pow(d - wave_r, 2.0) / wave_w);
    let wave_r_ch = exp(-pow(d - wave_r - 0.008, 2.0) / wave_w);
    let wave_b_ch = exp(-pow(d - wave_r + 0.008, 2.0) / wave_w);
    let wave = vec3<f32>(wave_r_ch, wave_g, wave_b_ch) * wave_fade;

    // 4. Three layers of sparks rising past the edges.
    let sparks = mythic_sparks(p, d, t, seed, 22.0, 0.10, 0.16, 0.16)
        + mythic_sparks(p, d, t, seed + 0.37, 34.0, 0.16, 0.18, 0.12) * 0.8
        + mythic_sparks(p, d, t, seed + 0.71, 14.0, 0.06, 0.16, 0.20) * 0.6;

    // 5. A four-point star flare that travels round the outline and flickers.
    let star_at = mythic_perimeter(t * 0.07 + seed, half);
    let star_flicker = smoothstep(0.2, 1.0, 0.5 + 0.5 * sin(t * 2.1 + seed * 5.0));
    let star = mythic_star(p, star_at, 0.11) * star_flicker;

    // 6. Slow dust motes drifting past the card.
    let motes = mythic_sparks(p, d, t, seed + 0.53, 8.0, 0.03, 0.12, 0.22) * 0.5;

    let base = tint.rgb;
    let hot = mix(base, vec3<f32>(1.0), 0.45);
    let rgb = (base * (glow * 0.95 + rays * 0.45 + lip + motes)
        + hot * (rim * 0.3 + sparks * 0.7 + star * 0.8)
        + mix(wave, wave * base * 2.0, 0.5) * 0.55)
        * edge * tint.a;
    let body = glow * 0.45 + rim * 0.25 + lip * 0.4;
    let alpha = clamp(body * edge, 0.0, 0.7) * tint.a;
    return vec4<f32>(rgb, alpha);
}

fn shade_mythic_stack_outro(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let frame = highlight_frame(uv, aspect, MYTHIC_STACK_OUTRO_PAD);
    let p = frame.xy;
    let half = frame.zw;
    let d = highlight_sd_box(p, half, MYTHIC_STACK_CORNER_R);
    let out_d = max(d, 0.0);
    let edge = vfx_quad_edge(uv, 0.70);
    let pol = vfx_polar(p);

    // 1. The outline flares and fades.
    let flare_env = smoothstep(0.0, 0.08, t) * (1.0 - smoothstep(0.15, 0.6, t));
    let outline = (exp(-out_d / 0.06) * smoothstep(-0.04, 0.0, d)
        + exp(-(d * d) / (2.0 * 0.01 * 0.01))) * flare_env;

    // 2. A card-shaped shockwave runs outward.
    let wave_t = clamp(t / 0.7, 0.0, 1.0);
    let wave_r = pow(wave_t, 0.6) * 0.42;
    let wave_env = smoothstep(0.0, 0.1, wave_t) * (1.0 - wave_t);
    let wave = exp(-pow(d - wave_r, 2.0) / (2.0 * 0.016 * 0.016)) * wave_env;

    // 3. A ring of rays bursts outward from the card.
    let ray_env = smoothstep(0.0, 0.06, t) * (1.0 - smoothstep(0.1, 0.5, t));
    let rays = pow(abs(cos(pol.x * 7.0 + seed * 6.2831853)), 18.0)
        * exp(-out_d / (0.08 + 0.3 * t)) * smoothstep(0.0, 0.02, d) * ray_env;

    // 4. A star flare bursts on the card's edge.
    let star_at = mythic_perimeter(seed, half) * 0.85;
    let star_env = smoothstep(0.0, 0.05, t) * (1.0 - smoothstep(0.08, 0.45, t));
    let star = mythic_star(p, star_at, 0.22) * star_env;

    // 5. Sparks thrown outward from the edge.
    let fling = mythic_sparks(p * (1.0 - 0.35 * t), d - t * 0.25, t * 2.0, seed, 26.0, 0.0, 0.3, 0.3)
        * (1.0 - smoothstep(0.3, 1.0, t));

    let base = tint.rgb;
    let hot = mix(base, vec3<f32>(1.0), 0.7);
    let rgb = (base * (outline * 0.9 + wave * 0.8 + rays * 0.6)
        + hot * (star * 1.1 + fling * 0.9 + outline * 0.3))
        * edge * tint.a;
    let alpha = clamp((outline * 0.4 + wave * 0.25 + star * 0.3) * edge, 0.0, 0.8) * tint.a;
    return vec4<f32>(rgb, alpha);
}
