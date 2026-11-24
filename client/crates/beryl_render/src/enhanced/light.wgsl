// The Enhanced tier's lighting of cards and the table (docs/beryl-3d.md, "The Enhanced tier":
// "Per-pixel lighting"): an ambient term, the key light and a soft fill from the other side,
// multiplying the Baseline color, which stays the sRGB value it is stored as. A card also takes a
// faint white sheen where its face mirrors the key light toward the camera, so the highlight
// moves across the face as the card tilts. The shadow map's `shadow` scales the whole result,
// not the key light alone, so that a shadow darkens what it falls on by `shadow.darkness`
// as the Baseline tier's contact shadows darken it by their opacity.
//
// `normal` is the record's normal on the side seen, not normalized; `view` the unit direction
// from the fragment toward the camera; `shadow` how much of the light reaches the fragment, from
// 1 - darkness in full shadow to 1. The lights are `lit`, as `light::lighting` writes them:
// `key.xyz` the direction toward the key light times its strength, `key.w` the ambient term,
// `fill.xyz` the direction toward the fill light times its strength, `fill.w` the sheen's power
// and `eye.w` its strength.

// How much light reaches a point facing `n` (a unit vector) in the open: 1 for a card lying flat.
fn diffuse(n: vec3<f32>) -> f32 {
    return lit.key.w + max(dot(n, lit.key.xyz), 0.0) + max(dot(n, lit.fill.xyz), 0.0);
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
    let sheen = lit.eye.w * pow(max(dot(n, half_vector), 0.0), lit.fill.w);
    let rgb = color.rgb * diffuse(n) + vec3<f32>(sheen * color.a);
    return vec4<f32>(rgb * shadow, color.a);
}

// The table, lit as a card is, without the sheen. `params` and `fragment` are `shade_table`'s.
fn shade_table_lit(
    tint: vec4<f32>,
    params: vec4<f32>,
    fragment: vec2<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    shadow: f32,
) -> vec4<f32> {
    let color = shade_table(tint, params, fragment);
    return vec4<f32>(color.rgb * diffuse(normalize(normal)) * shadow, color.a);
}
