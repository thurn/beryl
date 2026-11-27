// Test-only procedural VFX (`Vfx::Probe`): a soft tinted ring whose radius expands with
// normalized progress `t`, demonstrating the per-effect shader contract:
// `shade_<effect>(uv, t, seed, aspect, tint) -> vec4<f32>` (premultiplied body + additive rim).

const VFX_PROBE: u32 = 5u;
const BLOOM_GAIN_PROBE: f32 = 0.45;

fn shade_probe(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let polar = vfx_polar(p);
    let radius = mix(0.08, 0.40, clamp(t, 0.0, 1.0));
    let d = abs(polar.y - radius);
    let edge = vfx_quad_edge(uv, 0.82);

    let shimmer = 0.92 + 0.08 * sin(polar.x * 4.0 + seed * 6.2831853);
    let body_band = exp(-(d * d) / (2.0 * 0.042 * 0.042)) * edge;
    let rim_band = exp(-(d * d) / (2.0 * 0.014 * 0.014)) * edge * shimmer;

    let body_alpha = clamp(body_band * 0.55, 0.0, 0.85) * tint.a;
    let body_rgb = tint.rgb * body_alpha;
    let core_col = mix(tint.rgb, vec3<f32>(1.0), 0.45);
    let add_rgb = core_col * rim_band * (0.95 * tint.a);

    return vec4<f32>(body_rgb + add_rgb, body_alpha);
}
