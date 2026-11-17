// The Enhanced tier's bloom (docs/beryl-3d.md, "The Enhanced tier": "Bloom"): a separable
// Gaussian blur of the glows drawn at a fraction of the frame's size, and the composite that
// adds the blurred light onto the resolved picture. Both are drawn as one full-screen triangle
// with no vertex buffer.

struct Bloom {
    // The step between taps, in texels of the source: (1, 0) across, (0, 1) down.
    direction: vec2<f32>,
    // How many taps lie on each side of the centre.
    radius: u32,
    // How much of the blurred light the composite adds.
    strength: f32,
    // The weight of the tap `i` texels from the centre is `weights[i / 4][i % 4]`.
    weights: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> b: Bloom;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

struct ScreenOut {
    @builtin(position) clip: vec4<f32>,
    // Where the pixel falls on the source, from 0 to 1, y down.
    @location(0) uv: vec2<f32>,
};

// A triangle that covers the whole target: vertices at uv (0, 0), (2, 0) and (0, 2).
@vertex
fn vs_screen(@builtin(vertex_index) index: u32) -> ScreenOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: ScreenOut;
    out.clip = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn weight(i: u32) -> f32 {
    return b.weights[i / 4u][i % 4u];
}

// One direction of the blur: the source's texels along `direction`, weighted.
@fragment
fn blur(in: ScreenOut) -> @location(0) vec4<f32> {
    let step = b.direction / vec2<f32>(textureDimensions(source));
    var sum = textureSampleLevel(source, source_sampler, in.uv, 0.0) * weight(0u);
    for (var i = 1u; i <= b.radius; i++) {
        let offset = step * f32(i);
        let pair = textureSampleLevel(source, source_sampler, in.uv + offset, 0.0)
            + textureSampleLevel(source, source_sampler, in.uv - offset, 0.0);
        sum += pair * weight(i);
    }
    return sum;
}

// The blurred light times `strength`, added onto the picture by the pipeline's blending.
@fragment
fn composite(in: ScreenOut) -> @location(0) vec4<f32> {
    let light = textureSampleLevel(source, source_sampler, in.uv, 0.0);
    return vec4<f32>(light.rgb * b.strength, 0.0);
}
