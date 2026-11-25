
// The Enhanced tier draws cards on the table and cards held by the pointer into a depth map
// seen from the key light by `vs_shadow`. Then `lit_by_key` compares a fragment's depth from
// the light with the map.

// Where a card-mesh vertex lies in the key light's clip space, for the depth-only pass that
// draws the shadow map. A record of any material but a card's (the table, which shares the card
// mesh) is moved past the far plane, so it casts nothing. A card drawn with a soft edge
// (`Mesh::CardEdge`) casts with the card mesh's triangles alone.
//
// A card casts what the main pass shows of it. One less than half opaque (a phased-out card,
// or one fading out) casts nothing, as the Baseline tier's contact shadow under it fades with
// its opacity. A dissolving card breaks up from the top of its face down (`shade_card`), so its
// shadow is cut to the rows below the breaking edge's middle: each vertex above the cut is moved
// down onto it, and a card fully dissolved casts a line with no area.
@vertex
fn vs_shadow(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(4) row0: vec4<f32>,
    @location(5) row1: vec4<f32>,
    @location(6) row2: vec4<f32>,
    @location(7) rect: vec4<u32>,
    @location(8) layer_material: vec2<u32>,
    @location(9) tint: vec4<f32>,
    @location(10) params: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    // An opaque card's edge strip drawn alone (`EDGE_ONLY`) is drawn here with the card's
    // triangles only, which the card's own record already casts.
    if ((layer_material.y & 255u) != CARD || tint.a < 0.5 || rect.y == EDGE_ONLY) {
        return vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    // The card mesh's uv runs down the face, from 0 at its top edge, and its position's y up it.
    let cut = params.x * (1.0 + DISSOLVE_EDGE) - 0.5 * DISSOLVE_EDGE;
    let y = select(position.y, (0.5 - cut) * CARD_SIZE.y, params.x > 0.0 && uv.y < cut);
    let point = vec4<f32>(position.x, y, 0.0, 1.0);
    let table = vec4<f32>(dot(row0, point), dot(row1, point), dot(row2, point), 1.0);
    return lit.light_view_projection * table;
}

// How much of the key light reaches a fragment at `light_position` in the light's clip space,
// from 1 - darkness in full shadow to 1, where darkness is `lit.shade.x` (`shadow.darkness` in
// `data/client/lighting.json`): the average of four comparisons with the map in
// a 2 by 2 pattern one texel apart, each filtered by the comparison sampler. A point outside
// the map is lit. Called in uniform control flow, so that it may sample the map.
fn lit_by_key(light_position: vec4<f32>) -> f32 {
    let ndc = light_position.xyz / light_position.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let texel = 1.0 / vec2<f32>(textureDimensions(shadow_map));
    let half_texel = texel * 0.5;
    let depth = ndc.z;
    var reached = 0.0;
    reached += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-half_texel.x, -half_texel.y), depth);
    reached += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(half_texel.x, -half_texel.y), depth);
    reached += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-half_texel.x, half_texel.y), depth);
    reached += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(half_texel.x, half_texel.y), depth);
    reached *= 0.25;
    let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0)) && depth >= 0.0 && depth <= 1.0;
    reached = select(1.0, reached, inside);
    return 1.0 - lit.shade.x * (1.0 - reached);
}
