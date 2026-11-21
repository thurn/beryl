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
