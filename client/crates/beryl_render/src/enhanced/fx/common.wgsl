// Shared procedural VFX helpers used across Enhanced-tier effect modules (`fx/*.wgsl` and
// `combat_fx.wgsl`): value noise, 3-octave fBm, distance-to-segment, polar coordinates, overlay
// channel blend, and quad-border feathering. Zero texture samples.

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

// Polar coordinates `(angle, radius)` around the origin, with `angle` in `[-pi, pi]`.
fn vfx_polar(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(p.y, p.x), length(p));
}

// Overlay channel blend: warm midtone lift with a crisp bright crest.
fn vfx_overlay(base: vec3<f32>, top: vec3<f32>) -> vec3<f32> {
    let hi = vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - top) * (vec3<f32>(1.0) - base);
    let lo = 2.0 * base * top;
    return select(lo, hi, top > vec3<f32>(0.5));
}

// Smooth fade to zero before the quad boundary (`start` in `[0, 1)`) so outer halos never clip.
fn vfx_quad_edge(uv: vec2<f32>, start: f32) -> f32 {
    return (1.0 - smoothstep(start, 1.0, abs(uv.x * 2.0 - 1.0)))
        * (1.0 - smoothstep(start, 1.0, abs(uv.y * 2.0 - 1.0)));
}
