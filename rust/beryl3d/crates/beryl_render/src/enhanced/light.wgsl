// The Enhanced tier's lighting of cards and the table (docs/beryl-3d.md, "The Enhanced tier":
// "Per-pixel lighting"): an ambient term, the key light, which the shadow map shades, and a soft
// fill from the other side, all multiplying the Baseline color, which stays the sRGB value it is
// stored as. A card also takes a faint white sheen where its face mirrors the key light toward
// the camera, so the highlight moves across the face as the card tilts.
//
// `normal` is the record's normal on the side seen, not normalized; `view` the unit direction
// from the fragment toward the camera; `shadow` how much of the key light reaches the fragment,
// from 0 to 1. The lights are `lit`, as `light::lighting` writes them: `key.xyz` the direction
// toward the key light times its strength, `key.w` the ambient term, `fill.xyz` the direction
// toward the fill light times its strength, `fill.w` the sheen's power and `eye.w` its strength.

// How much light reaches a point facing `n` (a unit vector): 1 for a card lying flat in the open.
fn diffuse(n: vec3<f32>, shadow: f32) -> f32 {
    return lit.key.w + max(dot(n, lit.key.xyz), 0.0) * shadow + max(dot(n, lit.fill.xyz), 0.0);
}

// A card, lit. The sheen is added after the multiply, scaled by the color's opacity, so a
// dissolving or fading card's highlight goes with it, as its color does.
fn shade_card_lit(
    face: vec4<f32>,
    mesh_uv: vec2<f32>,
    tint: vec4<f32>,
    params: vec4<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    shadow: f32,
) -> vec4<f32> {
    let color = shade_card(face, mesh_uv, tint, params);
    let n = normalize(normal);
    let half_vector = normalize(normalize(lit.key.xyz) + view);
    let sheen = lit.eye.w * pow(max(dot(n, half_vector), 0.0), lit.fill.w) * shadow;
    return vec4<f32>(color.rgb * diffuse(n, shadow) + vec3<f32>(sheen * color.a), color.a);
}

// The table, lit as a card is, without the sheen.
fn shade_table_lit(tint: vec4<f32>, normal: vec3<f32>, view: vec3<f32>, shadow: f32) -> vec4<f32> {
    let color = shade_table(tint);
    return vec4<f32>(color.rgb * diffuse(normalize(normal), shadow), color.a);
}
