
// The Enhanced tier's particles (docs/beryl-3d.md, "The Enhanced tier": "Particles"), which only
// that tier's draw lists hold.

const PARTICLE: u32 = 8u;

// A particle: the sprite's coverage (the atlas's one channel) in the tint's color, fading as
// its `age` (params.x) runs from 0 to 1. Like a glow it adds light to what is beneath it, so
// its opacity is zero.
fn shade_particle(sprite: vec4<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {
    let a = sprite.r * tint.a * (1.0 - params.x);
    return vec4<f32>(tint.rgb * a, 0.0);
}
