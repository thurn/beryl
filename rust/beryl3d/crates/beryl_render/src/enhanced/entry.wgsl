
// The Enhanced tier's entry points (docs/beryl-3d.md, "The Enhanced tier"), appended to the
// Baseline shader, whose structures and shading functions they reuse. The Baseline shader is
// frozen, and WGSL cannot call an entry point, so `vs_lit` repeats `vs` and `fs_lit` repeats
// the start of `fs`.

struct Lit {
    // From the table's space to the key light's clip space, for the shadow map.
    light_view_projection: mat4x4<f32>,
    // The direction toward the key light times its strength, and the ambient term in w.
    key: vec4<f32>,
    // The camera's position in the table's space, and the sheen's strength in w.
    eye: vec4<f32>,
    // The direction toward the fill light times its strength, and the sheen's power in w.
    fill: vec4<f32>,
};

@group(1) @binding(0) var<uniform> lit: Lit;
@group(1) @binding(1) var shadow_map: texture_depth_2d;
@group(1) @binding(2) var shadow_sampler: sampler_comparison;

// `VertexOut`, and where the vertex is for lighting.
struct LitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) mesh_uv: vec2<f32>,
    @location(2) @interpolate(flat) layers: vec2<u32>,
    @location(3) @interpolate(flat) material: u32,
    @location(4) tint: vec4<f32>,
    @location(5) @interpolate(flat) params: vec4<f32>,
    @location(6) edge: f32,
    @location(7) @interpolate(flat) rect: vec4<u32>,
    // Where the vertex is in the table's space.
    @location(8) world: vec3<f32>,
    // The record's normal, the third column of its transform, not normalized.
    @location(9) normal: vec3<f32>,
    // Where the vertex is in the key light's clip space.
    @location(10) light_position: vec4<f32>,
};

@vertex
fn vs_lit(v: VertexIn) -> LitOut {
    let position = world(v, v.position);
    var clip = u.view_projection * vec4<f32>(position, 1.0);
    if (v.edge > 0.0) {
        // The outer ring of the edge strip, as `vs` places it.
        let ahead = u.view_projection * vec4<f32>(world(v, v.position + v.outward), 1.0);
        let half_target = u.target_size * 0.5;
        let direction = normalize((ahead.xy / ahead.w - clip.xy / clip.w) * half_target);
        clip = vec4<f32>(clip.xy + direction / half_target * v.edge * clip.w, clip.zw);
    }
    if (SHAKEN) {
        clip = vec4<f32>(clip.xy + vec2<f32>(u.shake.x, -u.shake.y) * 2.0 / u.target_size * clip.w, clip.zw);
    }
    var out: LitOut;
    out.clip = clip;
    out.material = v.layer_material.y & 255u;
    out.mesh_uv = v.uv;
    out.tint = v.tint;
    out.params = v.params;
    out.edge = v.edge;
    out.rect = v.rect;
    if (out.material == CARD) {
        out.uv = v.uv;
        out.layers = vec2<u32>(v.layer_material.x, v.rect.x);
    } else {
        let size = vec2<f32>(textureDimensions(pictures));
        out.uv = (vec2<f32>(v.rect.xy) + v.uv * vec2<f32>(v.rect.zw)) / size;
        out.layers = vec2<u32>(v.layer_material.x);
    }
    out.world = position;
    out.normal = vec3<f32>(v.row0.z, v.row1.z, v.row2.z);
    out.light_position = lit.light_view_projection * vec4<f32>(position, 1.0);
    return out;
}

@fragment
fn fs_lit(in: LitOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // As in `fs`: a card seen from behind shows its back, upside down.
    let back = in.material == CARD && !front;
    let layer = select(in.layers.x, in.layers.y, back);
    let uv = select(in.uv, vec2<f32>(in.uv.x, 1.0 - in.uv.y), back);
    // Sampled before branching on the material, in uniform control flow, as `fs` samples it;
    // the shadow map too.
    let face = textureSample(pictures, picture_sampler, uv, i32(layer));
    let mesh_pixel = fwidth(in.mesh_uv);
    let shadow = lit_by_key(in.light_position);
    // The side of the record seen faces the other way when it is seen from behind.
    let normal = select(-in.normal, in.normal, front);
    let view = normalize(lit.eye.xyz - in.world);
    var color = vec4<f32>(0.0);
    switch in.material {
        case TABLE: { color = shade_table_lit(in.tint, normal, view, shadow); }
        case CARD: { color = shade_card_lit(face, in.mesh_uv, in.tint, in.params, normal, view, shadow); }
        case CONTACT_SHADOW: { color = shade_contact_shadow(in.mesh_uv, in.tint, in.params); }
        case GLOW: { color = shade_glow(face, in.tint, in.params); }
        case STREAK: { color = shade_streak(face, in.mesh_uv, in.tint, in.params); }
        case PANEL: { color = shade_panel(in.mesh_uv, mesh_pixel, in.tint, in.rect, in.params); }
        case GLYPH: { color = shade_glyph(face, in.tint); }
        case ICON: { color = shade_icon(face, in.tint); }
        case PARTICLE: { color = shade_particle(face, in.tint, in.params); }
        default: {}
    }
    return color * (1.0 - in.edge);
}
