@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var filtered: sampler;

struct Corner {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Corner {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return Corner(vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0), vec2f(x, y));
}

@fragment
fn fragment(corner: Corner) -> @location(0) vec4f {
    return textureSample(picture, filtered, corner.uv);
}
