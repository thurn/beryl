// The backdrop filter's two passes (backdrop.rs): a Gaussian blur across, then down with the
// color filters. Both read texels exactly, with `textureLoad`, and clamp at the picture's edges.
// The blur works on sRGB values, as WebKit's does.

struct Blur {
    // One texel along the blur: (1, 0) across, (0, 1) down.
    step: vec2<i32>,
    // How many texels the blur reaches each side.
    radius: i32,
    // 1 when the source holds linear values, which are encoded as sRGB before the blur.
    decode: u32,
    // The blur's standard deviation in texels.
    sigma: f32,
    // 1 when the output is linear, so that the result is decoded from sRGB after the filters.
    encode: u32,
    pad: vec2<f32>,
    // The color filters' matrix, one row of weights of red, green and blue to a vector.
    rows: array<vec4<f32>, 3>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> blur: Blur;

@vertex
fn vs_screen(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, linear * 12.92, linear <= vec3<f32>(0.0031308));
}

fn decode_srgb(encoded: vec3<f32>) -> vec3<f32> {
    let high = pow((encoded + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, encoded / 12.92, encoded <= vec3<f32>(0.04045));
}

@fragment
fn fs_filter(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(source)) - vec2<i32>(1, 1);
    let centre = vec2<i32>(position.xy);
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    for (var k = -blur.radius; k <= blur.radius; k = k + 1) {
        let at = clamp(centre + blur.step * k, vec2<i32>(0, 0), last);
        var color = textureLoad(source, at, 0).rgb;
        if blur.decode == 1u {
            color = encode_srgb(color);
        }
        let x = f32(k);
        // A radius of 0 has the one weight 1, whatever the deviation.
        let weight = select(exp(-x * x / (2.0 * blur.sigma * blur.sigma)), 1.0, blur.radius == 0);
        sum = sum + color * weight;
        weights = weights + weight;
    }
    let blurred = sum / weights;
    var color = clamp(
        vec3<f32>(
            dot(blur.rows[0].xyz, blurred),
            dot(blur.rows[1].xyz, blurred),
            dot(blur.rows[2].xyz, blurred),
        ),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    if blur.encode == 1u {
        color = decode_srgb(color);
    }
    return vec4<f32>(color, 1.0);
}
