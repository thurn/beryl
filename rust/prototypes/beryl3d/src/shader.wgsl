// One shader for every record: the record's material picks the branch, so records of different
// materials share one instanced draw call. Card faces are layers of one array texture; a card's
// back is the layer in uv.x, shown when the card's back side faces the camera.

struct Uniforms {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var faces: texture_2d_array<f32>;
@group(0) @binding(2) var face_sampler: sampler;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) row0: vec4<f32>,
    @location(3) row1: vec4<f32>,
    @location(4) row2: vec4<f32>,
    @location(5) extra: vec4<u32>,
    @location(6) layer_material: vec2<u32>,
    @location(7) tint: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layers: vec2<u32>,
    @location(2) @interpolate(flat) material: u32,
    @location(3) tint: vec4<f32>,
    @location(4) world: vec2<f32>,
    @location(5) @interpolate(flat) blur: f32,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    let p = vec4<f32>(v.position, 0.0, 1.0);
    let world = vec3<f32>(dot(v.row0, p), dot(v.row1, p), dot(v.row2, p));
    var out: VertexOut;
    out.clip = u.view_proj * vec4<f32>(world, 1.0);
    out.uv = v.uv;
    out.material = v.layer_material.y & 255u;
    // Only a card's extra field names a layer; a shadow's holds its blur.
    out.layers = select(vec2<u32>(0u), vec2<u32>(v.layer_material.x, v.extra.x), out.material == 1u);
    out.tint = v.tint;
    out.world = world.xy;
    out.blur = f32(v.extra.x) / 100.0;
    return out;
}

// Signed distance from a point to a rounded rectangle centred on the origin.
fn rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Sampled before branching on the material, which varies per instance: WGSL allows implicit
    // derivatives only in uniform control flow.
    let layer = select(in.layers.y, in.layers.x, front);
    // Seen from behind, the mesh's texture coordinates run right to left.
    let uv = select(vec2<f32>(1.0 - in.uv.x, in.uv.y), in.uv, front);
    let face = textureSample(faces, face_sampler, uv, i32(layer));
    if (in.material == 0u) {
        // The table: a dark cloth that brightens toward the middle.
        let d = length(in.world * vec2<f32>(1.0 / 900.0, 1.0 / 520.0));
        let shade = mix(0.20, 0.07, clamp(d, 0.0, 1.0));
        return vec4<f32>(shade * 0.55, shade * 0.75, shade, 1.0);
    }
    if (in.material == 2u) {
        // A shadow: the card's rounded rectangle, blurred, in the middle of a larger quad.
        let p = (in.uv - vec2<f32>(0.5)) * vec2<f32>(1.3 * 63.0, 1.3 * 88.0);
        let blur = in.blur * 63.0;
        let d = rounded_box(p, vec2<f32>(31.5, 44.0) - vec2<f32>(blur * 0.5), 3.0);
        let a = in.tint.a * (1.0 - smoothstep(-blur * 0.5, blur, d));
        return vec4<f32>(0.0, 0.0, 0.0, a);
    }
    // A card, premultiplied by its opacity.
    let color = face.rgb * in.tint.rgb;
    return vec4<f32>(color * in.tint.a, in.tint.a);
}
