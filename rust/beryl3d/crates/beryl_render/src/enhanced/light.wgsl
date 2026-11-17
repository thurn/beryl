
// The Enhanced tier's lighting of cards and the table (docs/beryl-3d.md, "The Enhanced tier":
// "Per-pixel lighting"). `normal` is the record's normal on the side seen, not normalized;
// `view` the unit direction from the fragment toward the camera; `shadow` how much of the key
// light reaches the fragment, from 0 to 1.
//
// Not lit yet: each is the Baseline color darkened by the shadow.

// A card, lit.
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
    return vec4<f32>(color.rgb * shadow, color.a);
}

// The table, lit.
fn shade_table_lit(tint: vec4<f32>, normal: vec3<f32>, view: vec3<f32>, shadow: f32) -> vec4<f32> {
    let color = shade_table(tint);
    return vec4<f32>(color.rgb * shadow, color.a);
}
