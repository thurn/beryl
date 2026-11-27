// Procedural Enhanced-tier card highlight shimmer (`Vfx::HighlightShimmer`) and one-shot appear
// pop (`Vfx::HighlightPop`), shared across Eligible (Cold), Selected, and Hover highlight styles:
// - `shade_highlight_shimmer`: emissive rounded-rect outline frame, scrolling distorted flare
//   panner (~3.5 s period), and gently breathing thin additive rim.
// - `shade_highlight_pop`: one-shot 400 ms outward-expanding card-shaped burst wave on appear.
// Zero texture samples; smooth quad-edge feathering so adjacent hand cards never crowd or clip.

const VFX_HIGHLIGHT_SHIMMER: u32 = 12u;
const VFX_HIGHLIGHT_POP: u32 = 13u;

const BLOOM_GAIN_HIGHLIGHT_SHIMMER: f32 = 0.14;
const BLOOM_GAIN_HIGHLIGHT_POP: f32 = 0.14;

const HIGHLIGHT_SHIMMER_PAD: f32 = 0.14;
const HIGHLIGHT_POP_PAD: f32 = 0.19;
const HIGHLIGHT_CORNER_R: f32 = 0.05;

// Signed distance to an axis-aligned rounded rectangle with half-extents `b` and corner radius `r`.
fn highlight_sd_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Maps quad UV into card-width units (`base_w = 1.0`) and returns `(p, half_extents)`.
fn highlight_frame(uv: vec2<f32>, aspect: f32, pad: f32) -> vec4<f32> {
    let quad_scale = 1.0 + 2.0 * pad;
    let height_ratio = quad_scale * max(aspect, 0.1);
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(quad_scale, height_ratio);
    let half_h = max(0.5 * (height_ratio - 2.0 * pad), 0.08);
    return vec4<f32>(p.x, p.y, 0.5, half_h);
}

// `style`: 0 = Eligible (Cold cyan-mint), 1 = Selected (orange-gold), 2 = Hover (cyan-blue).
fn shade_highlight_shimmer(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
    style: u32,
) -> vec4<f32> {
    let frame = highlight_frame(uv, aspect, HIGHLIGHT_SHIMMER_PAD);
    let p = frame.xy;
    let half = frame.zw;
    let d = highlight_sd_box(p, half, HIGHLIGHT_CORNER_R);
    let edge = vfx_quad_edge(uv, 0.80);

    // Palette: saturated Arena Cold cyan-mint (128, 236, 213) blended with mark cyan and subtle
    // seafoam (117, 193, 97) outer crest for Eligible; other styles inherit their mark tint.
    let base_col = select(
        tint.rgb,
        mix(tint.rgb, vec3<f32>(0.38, 0.92, 0.86), 0.45),
        style == 0u,
    );
    let accent_col = select(
        mix(tint.rgb, vec3<f32>(1.0), 0.20),
        vec3<f32>(0.459, 0.757, 0.380),
        style == 0u,
    );
    let core_col = mix(base_col, vec3<f32>(0.84, 1.0, 1.0), 0.36);

    // 1. Emissive card-shaped outline frame (`frame ttp`): crisp rim hugging d = 0 with a subtle
    //    inner lip so the card edge reads cleanly on both light and dark playmats.
    let rim_d = d - 0.007;
    let frame_rim = exp(-(rim_d * rim_d) / (2.0 * 0.012 * 0.012));
    let inner_d = d + 0.007;
    let inner_lip = exp(-(inner_d * inner_d) / (2.0 * 0.0075 * 0.0075)) * 0.45;
    let frame_band = (frame_rim + inner_lip) * smoothstep(-0.022, -0.003, d);

    // 2. Scrolling distorted flare panner (`flare panner loop`, ~3.5 s period): diagonal dual-layer
    //    value noise + slow perimeter travelling shimmer glint.
    let pan = vec2<f32>(t * 0.28, -t * 0.24) + vec2<f32>(seed * 7.3, seed * 11.1);
    let n1 = vfx_noise(p * 3.1 + pan);
    let n2 = vfx_noise(p * 5.6 - pan * 1.15 + vec2<f32>(n1 * 0.65, -n1 * 0.55));
    let d_flare = d - (n2 - 0.5) * 0.018;
    let out_d = max(d_flare - 0.010, 0.0);
    let flare_mask = exp(-(out_d * out_d) / (2.0 * 0.032 * 0.032)) * smoothstep(-0.016, 0.005, d_flare);
    let flare_mod = 0.48 + 0.72 * smoothstep(0.24, 0.80, n2);
    let halo_band = flare_mask * flare_mod;

    let angle = atan2(p.y, p.x);
    let glint = pow(0.5 + 0.5 * cos(angle * 2.0 + t * 1.75 + seed * 6.2831853), 3.0);

    // 3. Thin additive outline (`outline add`, ~3.3 s gentle breath).
    let breath = 0.85 + 0.15 * sin(t * 1.90 + seed * 6.2831853);
    let thin_d = d - 0.005;
    let thin_rim = exp(-(thin_d * thin_d) / (2.0 * 0.0075 * 0.0075)) * breath;

    let body_alpha = clamp((frame_band * 0.74 + halo_band * 0.34) * edge, 0.0, 0.82) * tint.a;
    let body_col = mix(base_col, accent_col, clamp(halo_band * 0.24, 0.0, 0.24));
    let body_rgb = body_col * body_alpha;
    let add_rgb = (core_col * thin_rim * 0.46
        + base_col * (halo_band * 0.26 + frame_rim * 0.28 * glint))
        * edge
        * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}

// One-shot 400 ms outward appear pop (`voom`): expanding card-shaped energy wave + soft burst.
fn shade_highlight_pop(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
    style: u32,
) -> vec4<f32> {
    let u = clamp(t, 0.0, 1.0);
    let frame = highlight_frame(uv, aspect, HIGHLIGHT_POP_PAD);
    let p = frame.xy;
    let half = frame.zw;
    let d = highlight_sd_box(p, half, HIGHLIGHT_CORNER_R);
    let edge = vfx_quad_edge(uv, 0.82);

    let ease = 1.0 - pow(1.0 - u, 2.4);
    let env = smoothstep(0.0, 0.14, u) * pow(1.0 - smoothstep(0.18, 1.0, u), 1.45);

    let wave_d = mix(0.008, 0.095, ease);
    let n = vfx_noise(p * 4.4 + vec2<f32>(seed * 9.1, -u * 1.8));
    let d_warp = d - (n - 0.5) * 0.016;

    let crest_d = d_warp - wave_d;
    let crest = exp(-(crest_d * crest_d) / (2.0 * 0.019 * 0.019));
    let burst_d = d_warp - wave_d * 0.72;
    let burst = exp(-(burst_d * burst_d) / (2.0 * 0.042 * 0.042)) * smoothstep(-0.022, 0.012, d_warp);

    let base_col = select(
        tint.rgb,
        mix(tint.rgb, vec3<f32>(0.42, 0.92, 0.84), 0.50),
        style == 0u,
    );
    let outer_col = select(
        tint.rgb,
        vec3<f32>(0.459, 0.757, 0.380),
        style == 0u,
    );
    let core_col = mix(base_col, vec3<f32>(0.88, 1.0, 1.0), 0.38);

    let body_alpha = clamp((crest * 0.34 + burst * 0.22) * env * edge, 0.0, 0.62) * tint.a;
    let body_rgb = mix(base_col, outer_col, clamp(u * 0.45 + 0.15, 0.0, 0.60)) * body_alpha;
    let add_rgb = (core_col * crest * 0.52 + base_col * burst * 0.28) * env * edge * tint.a;

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}
