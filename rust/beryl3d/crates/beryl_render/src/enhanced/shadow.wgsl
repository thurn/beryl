
// The Enhanced tier's shadow map (docs/beryl-3d.md, "The Enhanced tier": "A shadow map").

// How much of the key light reaches a fragment at `light_position` in the light's clip space,
// from 0 in full shadow to 1. Called in uniform control flow, so that it may sample the map.
//
// Not sampled yet: every fragment is lit.
fn lit_by_key(light_position: vec4<f32>) -> f32 {
    return 1.0;
}
