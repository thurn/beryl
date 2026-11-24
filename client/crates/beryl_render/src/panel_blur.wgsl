// The bounded backdrop filter samples a padded crop of the already-drawn frame. Bloom's
// separable blur runs on the crop, then this shader puts it back within the panel's shape.
struct Panel {
    crop: vec4<f32>,
    rect: vec4<f32>,
    clip: vec4<f32>,
    // Radius in physical pixels, opacity, whether the target is sRGB, and padding.
    appearance: vec4<f32>,
    rows: array<vec4<f32>, 3>,
};

@group(0) @binding(0) var<uniform> panel: Panel;
@group(0) @binding(1) var original: texture_2d<f32>;
@group(0) @binding(2) var blurred: texture_2d<f32>;
@group(0) @binding(3) var linear_sampler: sampler;

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, linear * 12.92, linear <= vec3<f32>(0.0031308));
}

fn decode_srgb(encoded: vec3<f32>) -> vec3<f32> {
    let high = pow((encoded + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, encoded / 12.92, encoded <= vec3<f32>(0.04045));
}

@fragment
fn downsample(in: ScreenOut) -> @location(0) vec4<f32> {
    let uv = (panel.crop.xy + in.uv * panel.crop.zw) / vec2<f32>(textureDimensions(original));
    var ink = textureSampleLevel(original, linear_sampler, uv, 0.0).rgb;
    if panel.appearance.z != 0.0 {
        ink = encode_srgb(ink);
    }
    return vec4<f32>(ink, 1.0);
}

@fragment
fn composite_panel(in: ScreenOut) -> @location(0) vec4<f32> {
    let pixel = in.clip.xy;
    let sharp = textureLoad(original, vec2<i32>(pixel), 0);
    if any(pixel < panel.clip.xy) || any(pixel >= panel.clip.xy + panel.clip.zw) {
        return sharp;
    }
    let half = panel.rect.zw * 0.5;
    let radius = clamp(panel.appearance.x, 0.0, min(half.x, half.y));
    let offset = abs(pixel - panel.rect.xy - half) - half + vec2<f32>(radius);
    let distance = length(max(offset, vec2<f32>(0.0))) + min(max(offset.x, offset.y), 0.0) - radius;
    let coverage = clamp(0.5 - distance, 0.0, 1.0) * panel.appearance.y;
    let uv = (pixel - panel.crop.xy) / panel.crop.zw;
    let ink = textureSampleLevel(blurred, linear_sampler, uv, 0.0).rgb;
    var adjusted = clamp(vec3<f32>(
        dot(panel.rows[0].xyz, ink),
        dot(panel.rows[1].xyz, ink),
        dot(panel.rows[2].xyz, ink),
    ), vec3<f32>(0.0), vec3<f32>(1.0));
    if panel.appearance.z != 0.0 {
        adjusted = decode_srgb(adjusted);
    }
    return vec4<f32>(mix(sharp.rgb, adjusted, coverage), sharp.a);
}
