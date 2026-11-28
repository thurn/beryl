// Procedural Enhanced-tier flares for a card arriving on the stack and leaving it.
//
// `Vfx::StackEnter`, as a spell lands on the stack (Arena's `vfx_stack_flare_enter`): a gold flash
// in the card's own rounded outline that is up at once and gone by the half, a soft smoke cloud, 10
// gold sparks thrown outward, a few flecks of dark debris, and white glitter that builds over the
// card.
//
// `Vfx::StackExit`, as a card leaves the stack (Arena's `vfx_stack_flare_exit`): a smaller puff of
// smoke, 5 wisps that curl away from the middle, a faint ghost outline of the card that thins
// away, and a few pale spores that drift up.
//
// Both quads are 1.7 times the card's width and 1.4 times as tall as wide, so the card fills the
// middle (`STACK_CARD_HALF`). Zero texture samples; smooth quad-edge feathering.

const VFX_STACK_ENTER: u32 = 17u;
const VFX_STACK_EXIT: u32 = 18u;

// The flash lies over a spell, and several spells can arrive in one beat, on top of the glow the
// stack already has: these keep their overlap short of white.
const BLOOM_GAIN_STACK_ENTER: f32 = 0.30;
const BLOOM_GAIN_STACK_EXIT: f32 = 0.16;

// Half the card's width and height in the quad's isotropic coordinates.
const STACK_CARD_HALF: vec2<f32> = vec2<f32>(0.29, 0.41);

// Signed distance from `p` to a rounded box of half size `half` and corner radius `r`: negative
// inside.
fn stack_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// A soft round speck at `p`, `radius` across.
fn stack_speck(p: vec2<f32>, centre: vec2<f32>, radius: f32) -> f32 {
    let d = p - centre;
    return exp(-dot(d, d) / (radius * radius));
}

fn shade_stack_enter(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let edge = vfx_quad_edge(uv, 0.82);
    let gold = tint.rgb;
    let hot = mix(gold, vec3<f32>(1.0), 0.30);
    let sd = stack_box(p, STACK_CARD_HALF, 0.03);

    // 1. The flash: the card's own outline, filled with gold light that is brightest at once,
    // with a rim of hotter light just outside it, gone by the half.
    let flash_env = (1.0 - smoothstep(0.04, 0.42, t)) * smoothstep(0.0, 0.02, t);
    let fill = (1.0 - smoothstep(-0.02, 0.03, sd)) * flash_env;
    let rim = exp(-pow(sd - 0.015 - 0.07 * t, 2.0) / (2.0 * 0.03 * 0.03)) * flash_env;

    // 2. Soft smoke: noise that rises round the card and thins, dark gold in colour.
    let smoke_t = clamp((t - 0.03) / 0.97, 0.0, 1.0);
    let smoke_n = vfx_fbm(p * 4.2 + vec2<f32>(seed * 11.0, -smoke_t * 0.9));
    let smoke_reach = exp(-max(sd, 0.0) / (0.10 + 0.08 * smoke_t));
    let smoke_env = smoothstep(0.0, 0.14, smoke_t) * (1.0 - smoothstep(0.30, 1.0, smoke_t));
    let smoke = smoke_reach * smoothstep(0.30, 0.80, smoke_n) * smoke_env;

    // 3. 10 gold sparks leaving the card's outline, each at its own angle and speed, slowing as
    // they go.
    var sparks = 0.0;
    for (var i = 0u; i < 10u; i++) {
        let fi = f32(i);
        let h = vfx_hash(vec2<f32>(fi * 6.17 + seed * 29.0, fi * 2.31 + 0.9));
        let h2 = vfx_hash(vec2<f32>(fi * 2.53 + 4.1, seed * 13.0 + fi * 0.43));
        let born = 0.02 + h2 * 0.12;
        let age = clamp((t - born) / (0.38 + 0.30 * h), 0.0, 1.0);
        let live = step(born, t) * (1.0 - age);
        let a = (fi + h * 0.7) * 0.62831853;
        let dir = vec2<f32>(cos(a), sin(a));
        let start = dir * STACK_CARD_HALF * 0.85;
        let reach = (0.04 + 0.16 * h2) * (1.0 - (1.0 - age) * (1.0 - age));
        sparks += stack_speck(p, start + dir * reach, 0.012) * live;
    }

    // 4. Flecks of dark debris that fall away from the outline. These are the only dark part.
    var debris = 0.0;
    for (var i = 0u; i < 6u; i++) {
        let fi = f32(i);
        let h = vfx_hash(vec2<f32>(fi * 4.43 + seed * 19.0, fi * 1.19 + 2.3));
        let h2 = vfx_hash(vec2<f32>(fi * 3.71 + 1.7, seed * 7.0 + fi * 0.83));
        let age = clamp((t - 0.04) / (0.50 + 0.2 * h), 0.0, 1.0);
        let live = step(0.04, t) * (1.0 - age) * smoothstep(0.0, 0.10, age);
        let a = (fi + h) * 1.04719755;
        let dir = vec2<f32>(cos(a), sin(a));
        let at = dir * STACK_CARD_HALF * 0.95 + vec2<f32>(0.0, 0.10 * age * age + 0.03 * h2);
        debris += stack_speck(p, at, 0.014) * live;
    }

    // 5. White glitter that builds over the card from the half, a few flecks winking at a time.
    let glit_env = smoothstep(0.06, 0.32, t) * (1.0 - smoothstep(0.50, 1.0, t));
    let cell = floor(p * 16.0 + vec2<f32>(seed * 40.0, 0.0));
    let wink = vfx_hash(cell + vec2<f32>(floor(t * 10.0), 3.0));
    let inside = 1.0 - smoothstep(-0.03, 0.01, sd);
    let local = fract(p * 16.0 + vec2<f32>(seed * 40.0, 0.0)) - vec2<f32>(0.5);
    let glitter = step(0.94, wink) * exp(-dot(local, local) / 0.04) * glit_env * inside;

    let add = (gold * (fill * 0.42 + smoke * 0.30 + rim * 0.55)
        + hot * min(sparks, 1.5) * 1.0
        + vec3<f32>(1.0) * glitter * 0.75) * edge * tint.a;
    let alpha = clamp(smoke * 0.22 + debris * 0.5, 0.0, 0.55) * edge * tint.a;
    return vec4<f32>(add, alpha);
}

fn shade_stack_exit(
    uv: vec2<f32>,
    t: f32,
    seed: f32,
    aspect: f32,
    tint: vec4<f32>,
) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, max(aspect, 0.1));
    let pol = vfx_polar(p);
    let angle = pol.x;
    let edge = vfx_quad_edge(uv, 0.82);
    let smoke_col = tint.rgb;
    let sd = stack_box(p, STACK_CARD_HALF, 0.03);

    // 1. A smaller puff of smoke over the card, thinner than the arrival's.
    let puff_t = clamp(t / 0.9, 0.0, 1.0);
    let puff_n = vfx_fbm(p * 3.6 + vec2<f32>(seed * 7.0, puff_t * 0.8));
    let puff_reach = exp(-max(sd, 0.0) / (0.07 + 0.07 * puff_t));
    let puff_env = smoothstep(0.0, 0.10, puff_t) * (1.0 - smoothstep(0.25, 1.0, puff_t));
    let puff = puff_reach * smoothstep(0.28, 0.76, puff_n) * puff_env;

    // 2. Five wisps that curl outward from the middle, each in its own direction.
    var wisps = 0.0;
    for (var i = 0u; i < 5u; i++) {
        let fi = f32(i);
        let h = vfx_hash(vec2<f32>(fi * 5.77 + seed * 17.0, fi * 1.43 + 0.5));
        let born = 0.02 + h * 0.10;
        let age = clamp((t - born) / (0.55 + 0.2 * h), 0.0, 1.0);
        let a = (fi + h * 0.6) * 1.2566371 + age * 0.9;
        let dir = vec2<f32>(cos(a), sin(a));
        let head = dir * (0.05 + 0.34 * (1.0 - (1.0 - age) * (1.0 - age)));
        let along = vfx_segment(p, head * 0.55, head);
        let live = step(born, t) * (1.0 - age) * smoothstep(0.0, 0.12, age);
        wisps += exp(-along * along / (2.0 * 0.018 * 0.018)) * live;
    }

    // 3. The card's ghost outline: a thin line round it that thins and is gone by the half.
    let ghost_env = (1.0 - smoothstep(0.05, 0.50, t)) * smoothstep(0.0, 0.03, t);
    let ghost = exp(-pow(sd - 0.01 * t, 2.0) / (2.0 * 0.008 * 0.008)) * ghost_env;

    // 4. Pale spores that drift up from the card.
    var spores = 0.0;
    for (var i = 0u; i < 7u; i++) {
        let fi = f32(i);
        let h = vfx_hash(vec2<f32>(fi * 3.19 + seed * 23.0, fi * 2.77 + 1.1));
        let h2 = vfx_hash(vec2<f32>(fi * 1.61 + 3.3, seed * 5.0 + fi * 0.67));
        let born = 0.05 + h2 * 0.2;
        let age = clamp((t - born) / (0.55 + 0.25 * h), 0.0, 1.0);
        let live = step(born, t) * (1.0 - age) * smoothstep(0.0, 0.2, age);
        let at = vec2<f32>((h - 0.5) * 2.0 * STACK_CARD_HALF.x, (h2 - 0.5) * STACK_CARD_HALF.y - 0.16 * age);
        spores += stack_speck(p, at, 0.010) * live;
    }

    let pale = mix(smoke_col, vec3<f32>(1.0), 0.55);
    let add = (smoke_col * (puff * 0.28 + min(wisps, 1.5) * 0.55)
        + pale * (ghost * 0.60 + spores * 0.75)) * edge * tint.a;
    let alpha = clamp(puff * 0.20 + wisps * 0.12, 0.0, 0.40) * edge * tint.a;
    return vec4<f32>(add, alpha);
}
