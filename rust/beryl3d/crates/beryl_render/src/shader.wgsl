// The Baseline tier's one shader (docs/beryl-3d.md, "The Baseline tier"). Every record is an
// instance; its material picks the branch, so records of different materials share one draw
// call. Each material is shaded by its own function named `shade_` and the material, which the
// GPU counts measure by name. Colors are premultiplied by their opacity, so ordinary and
// additive shapes share one blend setting. Colors are blended as the sRGB values they are
// stored as, as a browser blends them.

struct Uniforms {
    view_projection: mat4x4<f32>,
    // The target's size in physical pixels.
    target_size: vec2<f32>,
    // How far the picture of the table is shaken, in physical pixels, x right and y down.
    shake: vec2<f32>,
};

// Whether this pipeline's records are shaken: those drawn with the depth test are, the flat
// layer's are not.
override SHAKEN: bool = true;

const TABLE: u32 = 0u;
const CARD: u32 = 1u;
const CONTACT_SHADOW: u32 = 2u;
const GLOW: u32 = 3u;
const PANEL: u32 = 5u;
const GLYPH: u32 = 6u;

// A card's printed size and corner radius, in millimetres, as the card mesh is built.
const CARD_SIZE: vec2<f32> = vec2<f32>(63.0, 88.0);
const CARD_RADIUS: f32 = 3.0;

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var pictures: texture_2d_array<f32>;
@group(0) @binding(2) var picture_sampler: sampler;

struct VertexIn {
    // The mesh.
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) outward: vec2<f32>,
    @location(3) edge: f32,
    // The record: `Draw` as the client core wrote it.
    @location(4) row0: vec4<f32>,
    @location(5) row1: vec4<f32>,
    @location(6) row2: vec4<f32>,
    @location(7) rect: vec4<u32>,
    // `layer`, then `material` in the low byte and `flags` in the high byte.
    @location(8) layer_material: vec2<u32>,
    @location(9) tint: vec4<f32>,
    @location(10) params: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Where to sample the bound texture array.
    @location(0) uv: vec2<f32>,
    // Where the vertex falls on the mesh, from 0 to 1.
    @location(1) mesh_uv: vec2<f32>,
    // The layer to sample from the front and from the back.
    @location(2) @interpolate(flat) layers: vec2<u32>,
    @location(3) @interpolate(flat) material: u32,
    @location(4) tint: vec4<f32>,
    @location(5) @interpolate(flat) params: vec4<f32>,
    // 0 inside a shape, rising to 1 across the edge strip's one pixel.
    @location(6) edge: f32,
    // The record's `uv` as it was written: a panel's border color, from 0 to 255.
    @location(7) @interpolate(flat) rect: vec4<u32>,
};

fn world(v: VertexIn, p: vec2<f32>) -> vec3<f32> {
    let point = vec4<f32>(p, 0.0, 1.0);
    return vec3<f32>(dot(v.row0, point), dot(v.row1, point), dot(v.row2, point));
}

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var clip = u.view_projection * vec4<f32>(world(v, v.position), 1.0);
    if (v.edge > 0.0) {
        // The outer ring of the edge strip: one physical pixel outward on screen, along the
        // outline's outward direction as the camera sees it.
        let ahead = u.view_projection * vec4<f32>(world(v, v.position + v.outward), 1.0);
        let half_target = u.target_size * 0.5;
        let direction = normalize((ahead.xy / ahead.w - clip.xy / clip.w) * half_target);
        clip = vec4<f32>(clip.xy + direction / half_target * v.edge * clip.w, clip.zw);
    }
    if (SHAKEN) {
        clip = vec4<f32>(clip.xy + vec2<f32>(u.shake.x, -u.shake.y) * 2.0 / u.target_size * clip.w, clip.zw);
    }
    var out: VertexOut;
    out.clip = clip;
    out.material = v.layer_material.y & 255u;
    out.mesh_uv = v.uv;
    out.tint = v.tint;
    out.params = v.params;
    out.edge = v.edge;
    out.rect = v.rect;
    if (out.material == CARD) {
        // A card's face fills its layer; its back's layer is in the record's `uv[0]`.
        out.uv = v.uv;
        out.layers = vec2<u32>(v.layer_material.x, v.rect.x);
    } else {
        // A sprite: `rect` is its rectangle on an atlas page, in texels.
        let size = vec2<f32>(textureDimensions(pictures));
        out.uv = (vec2<f32>(v.rect.xy) + v.uv * vec2<f32>(v.rect.zw)) / size;
        out.layers = vec2<u32>(v.layer_material.x);
    }
    return out;
}

// Signed distance from a point to a rounded rectangle centred on the origin.
fn rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

// The table: its tint.
fn shade_table(tint: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(tint.rgb * tint.a, tint.a);
}

// A card: its face times its tint, turned toward white by `whiten` (params.y). No lighting.
fn shade_card(face: vec4<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {
    let color = mix(face.rgb * tint.rgb, vec3<f32>(1.0), params.y);
    return vec4<f32>(color * tint.a, tint.a);
}

// The soft shadow under a raised card: the card's rounded rectangle, blurred by `params.x`
// millimetres, fading to nothing at the edge of the card mesh it is drawn with.
fn shade_contact_shadow(mesh_uv: vec2<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {
    let p = (mesh_uv - vec2<f32>(0.5)) * CARD_SIZE;
    let blur = clamp(params.x, 0.5, 20.0);
    let d = rounded_box(p, CARD_SIZE * 0.5 - vec2<f32>(blur), CARD_RADIUS);
    let a = tint.a * (1.0 - smoothstep(-blur, blur, d));
    return vec4<f32>(tint.rgb * a, a);
}

// A glow: the sprite's coverage (the atlas's one channel) in the tint's color, fading as
// `progress` (params.x) runs to 1. Light adds to what is beneath it, so its opacity is zero.
fn shade_glow(sprite: vec4<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {
    let a = sprite.r * tint.a * (1.0 - params.x);
    return vec4<f32>(tint.rgb * a, 0.0);
}

// A panel of the flat layer: a rounded rectangle `params.xy` logical points large with corners
// of radius `params.z`, filled with the tint, and a border `params.w` wide in the color held in
// the record's `uv` (`border`, from 0 to 255). Both edges are antialiased over one pixel;
// `mesh_pixel` is how far `mesh_uv` moves across one pixel.
fn shade_panel(
    mesh_uv: vec2<f32>,
    mesh_pixel: vec2<f32>,
    tint: vec4<f32>,
    border: vec4<u32>,
    params: vec4<f32>,
) -> vec4<f32> {
    let size = params.xy;
    let pixel = max(max(mesh_pixel.x * size.x, mesh_pixel.y * size.y), 1e-4);
    let d = rounded_box((mesh_uv - vec2<f32>(0.5)) * size, size * 0.5, params.z);
    let outer = clamp(0.5 - d / pixel, 0.0, 1.0);
    let inner = select(1.0, clamp(0.5 - (d + params.w) / pixel, 0.0, 1.0), params.w > 0.0);
    let color = mix(vec4<f32>(border) / 255.0, tint, inner);
    let a = color.a * outer;
    return vec4<f32>(color.rgb * a, a);
}

// A glyph: its coverage (the atlas's one channel) times the tint.
fn shade_glyph(sprite: vec4<f32>, tint: vec4<f32>) -> vec4<f32> {
    let a = sprite.r * tint.a;
    return vec4<f32>(tint.rgb * a, a);
}

@fragment
fn fs(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // A card seen from behind shows its back, its texture coordinates running right to left.
    let back = in.material == CARD && !front;
    let layer = select(in.layers.x, in.layers.y, back);
    let uv = select(in.uv, vec2<f32>(1.0 - in.uv.x, in.uv.y), back);
    // Sampled before branching on the material, which varies per instance: WGSL allows
    // `textureSample` only in uniform control flow.
    let picture = textureSample(pictures, picture_sampler, uv, i32(layer));
    // Derivatives too are allowed only in uniform control flow.
    let mesh_pixel = fwidth(in.mesh_uv);
    var color = vec4<f32>(0.0);
    switch in.material {
        case TABLE: { color = shade_table(in.tint); }
        case CARD: { color = shade_card(picture, in.tint, in.params); }
        case CONTACT_SHADOW: { color = shade_contact_shadow(in.mesh_uv, in.tint, in.params); }
        case GLOW: { color = shade_glow(picture, in.tint, in.params); }
        case PANEL: { color = shade_panel(in.mesh_uv, mesh_pixel, in.tint, in.rect, in.params); }
        case GLYPH: { color = shade_glyph(picture, in.tint); }
        default: {}
    }
    return color * (1.0 - in.edge);
}
