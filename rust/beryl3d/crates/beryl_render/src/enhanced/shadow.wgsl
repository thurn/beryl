
// The Enhanced tier's shadow map (docs/beryl-3d.md, "The Enhanced tier": "A shadow map"): every
// card before the flat layer is drawn into a depth map seen from the key light by `vs_shadow`,
// and `lit_by_key` compares a fragment's depth from the light with the map.

// How much of the color a fragment in full shadow loses: `enhanced.shadow.darkness` in
// `data/beryl3d/space.json`, which a test holds this equal to.
const SHADOW_DARKNESS: f32 = 0.55;

// Where a card-mesh vertex lies in the key light's clip space, for the depth-only pass that
// draws the shadow map. A record of any material but a card's (the table, which shares the card
// mesh) is moved past the far plane, so it casts nothing.
@vertex
fn vs_shadow(
    @location(0) position: vec2<f32>,
    @location(4) row0: vec4<f32>,
    @location(5) row1: vec4<f32>,
    @location(6) row2: vec4<f32>,
    @location(8) layer_material: vec2<u32>,
) -> @builtin(position) vec4<f32> {
    if ((layer_material.y & 255u) != CARD) {
        return vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    let point = vec4<f32>(position, 0.0, 1.0);
    let table = vec4<f32>(dot(row0, point), dot(row1, point), dot(row2, point), 1.0);
    return lit.light_view_projection * table;
}

// How much of the key light reaches a fragment at `light_position` in the light's clip space,
// from 1 - SHADOW_DARKNESS in full shadow to 1: the average of four comparisons with the map in
// a 2 by 2 pattern one texel apart, each filtered by the comparison sampler. A point outside
// the map is lit. Called in uniform control flow, so that it may sample the map.
fn lit_by_key(light_position: vec4<f32>) -> f32 {
    let ndc = light_position.xyz / light_position.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let texel = 1.0 / vec2<f32>(textureDimensions(shadow_map));
    let half_texel = texel * 0.5;
    let depth = ndc.z;
    var lit = 0.0;
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-half_texel.x, -half_texel.y), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(half_texel.x, -half_texel.y), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-half_texel.x, half_texel.y), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(half_texel.x, half_texel.y), depth);
    lit *= 0.25;
    let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0)) && depth >= 0.0 && depth <= 1.0;
    lit = select(1.0, lit, inside);
    return 1.0 - SHADOW_DARKNESS * (1.0 - lit);
}
