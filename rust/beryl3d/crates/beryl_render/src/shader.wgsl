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
    // In x, 1 when a clip of the frame has a mask (`paints.masks`), or 0 when none does and the
    // masks were not written this frame.
    masked: vec4<u32>,
};

// One gradient's stops (`GradientEntry`): eight colors as RGBA bytes, red in the low byte, not
// premultiplied, and their offsets along the gradient line. Stops past the last repeat it.
struct Gradient {
    colors: array<vec4<u32>, 2>,
    offsets: array<vec4<f32>, 2>,
};

// One clip's mask (`MaskEntry`): a linear gradient over `rect` (physical pixels: x, y, width,
// height), and in `line` its angle in radians, its gradient's entry and 1 when it is in use.
struct Mask {
    rect: vec4<f32>,
    line: vec4<f32>,
};

// The frame's paint table (`PaintTable`): the masks by clip entry, then the gradients.
struct Paints {
    masks: array<Mask, 16>,
    gradients: array<Gradient, 32>,
};

// Whether this pipeline's records are shaken: those drawn with the depth test are, the flat
// layer's are not.
override SHAKEN: bool = true;

const TABLE: u32 = 0u;
const CARD: u32 = 1u;
const CONTACT_SHADOW: u32 = 2u;
const GLOW: u32 = 3u;
const STREAK: u32 = 4u;
const PANEL: u32 = 5u;
const GLYPH: u32 = 6u;
const ICON: u32 = 7u;
const BOX_SHADOW: u32 = 9u;
const GRADIENT: u32 = 10u;
// What a gradient record's layer is raised by when its gradient is radial (`frame::RADIAL`).
const RADIAL: u32 = 32768u;

// A card's printed size and corner radius, in millimetres, as the card mesh is built.
const CARD_SIZE: vec2<f32> = vec2<f32>(63.0, 88.0);
const CARD_RADIUS: f32 = 3.0;

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var pictures: texture_2d_array<f32>;
@group(0) @binding(2) var picture_sampler: sampler;
@group(0) @binding(3) var<uniform> paints: Paints;

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
    // The record's clip entry when the frame has masks, whose mask scales its opacity, or 0.
    @location(8) @interpolate(flat) masked_by: u32,
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
    // The clip is bits 4 to 7 of the flags, the high byte.
    out.masked_by = select(0u, (v.layer_material.y >> 12u) & 15u, u.masked.x != 0u);
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

// How much of the tabletop's color the web's grain (`--grain` in `data/design-tokens.json`, black
// fractal noise at 9% opacity) leaves on average. Measured over the web's tabletop, every pixel
// is the gradient's color times 0.952, give or take a level or two of noise; the noise itself is
// left out, since no pattern drawn here would fall on the same pixels as the web's.
const GRAIN: f32 = 0.952;

// `color` carried on toward `next` across the stretch of the tabletop's gradient from `low` to
// `high`: unchanged before `low`, `next` past `high`. Applied to each pair of stops in turn, from
// the middle out, it gives the gradient's color at `t`.
fn tabletop_stop(color: vec3<f32>, t: f32, low: f32, high: f32, next: vec3<f32>) -> vec3<f32> {
    return mix(color, next / 255.0, clamp((t - low) / (high - low), 0.0, 1.0));
}

// The web's `--tabletop` (`data/design-tokens.json`) at `t`, how far a pixel is from the middle
// of the screen as a share of half the screen's width and height (CSS's `radial-gradient(ellipse
// 50% 50% at 50% 50%, ...)`): its stops' colors and offsets, from the warm brown of the middle to
// the near black of the corners at t = 1.41. The render test `the_cloth_paints_the_tabletop`
// holds these stops to the token's.
fn tabletop_at(t: f32) -> vec3<f32> {
    var color = vec3<f32>(133.0, 113.0, 105.0) / 255.0;
    color = tabletop_stop(color, t, 0.0, 0.42, vec3<f32>(131.0, 112.0, 105.0));
    color = tabletop_stop(color, t, 0.42, 0.52, vec3<f32>(126.0, 108.0, 101.0));
    color = tabletop_stop(color, t, 0.52, 0.61, vec3<f32>(117.0, 100.0, 93.0));
    color = tabletop_stop(color, t, 0.61, 0.71, vec3<f32>(112.0, 96.0, 90.0));
    color = tabletop_stop(color, t, 0.71, 0.81, vec3<f32>(104.0, 89.0, 83.0));
    color = tabletop_stop(color, t, 0.81, 0.90, vec3<f32>(89.0, 76.0, 70.0));
    color = tabletop_stop(color, t, 0.90, 1.0, vec3<f32>(74.0, 63.0, 59.0));
    color = tabletop_stop(color, t, 1.0, 1.23, vec3<f32>(59.0, 51.0, 49.0));
    color = tabletop_stop(color, t, 1.23, 1.33, vec3<f32>(42.0, 38.0, 36.0));
    color = tabletop_stop(color, t, 1.33, 1.41, vec3<f32>(27.0, 24.0, 24.0));
    return color;
}

// The table: its tint, or where `params.x` is 1 (`Draw::tabletop`), the web's tabletop times the
// tint. The tabletop lies on the screen rather than on the table: its gradient spans the target,
// centred on it, and moves with the table when the table shakes, as the web's cloth moves with
// its stage. `fragment` is the pixel's position in physical pixels.
fn shade_table(tint: vec4<f32>, params: vec4<f32>, fragment: vec2<f32>) -> vec4<f32> {
    let from_middle = (fragment - u.shake) / u.target_size * 2.0 - vec2<f32>(1.0);
    let painted = tabletop_at(length(from_middle)) * GRAIN;
    let color = tint.rgb * select(vec3<f32>(1.0), painted, params.x > 0.5);
    return vec4<f32>(color * tint.a, tint.a);
}

// A card's broken edge as it dissolves: the cells of a 24 by 34 grid over the card, each with
// a fixed pseudo-random value, so that the edge is stippled rather than straight.
const DISSOLVE_GRID: vec2<f32> = vec2<f32>(24.0, 34.0);
// How much of the stipple is the cell's random value; the rest is how far down the card the
// pixel is, so that the card breaks up from the top down.
const DISSOLVE_SCATTER: f32 = 0.25;
// How wide the soft edge between kept and dropped pixels is, on the stipple's scale of 0 to 1.
const DISSOLVE_EDGE: f32 = 0.05;

// A color's luminance, by the weights of CSS's `grayscale()` filter.
const LUMINANCE: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// A card: its face times its tint, moved toward its luminance by `desaturate` (params.z) as
// CSS's `grayscale()` moves it, turned toward white by `whiten` (params.y), and broken up
// from the top down by `dissolve` (params.x): a pixel is dropped where its stipple, from the
// card's own texture coordinates `mesh_uv` (0 at the top), is under the threshold. A dropped
// pixel has zero opacity. No second texture is read and nothing is discarded, since a
// dissolving card is among the blended records. No lighting.
fn shade_card(face: vec4<f32>, mesh_uv: vec2<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {
    let tinted = face.rgb * tint.rgb;
    let gray = mix(tinted, vec3<f32>(dot(tinted, LUMINANCE)), params.z);
    let color = mix(gray, vec3<f32>(1.0), params.y);
    let cell = floor(mesh_uv * DISSOLVE_GRID);
    let scatter = fract(sin(dot(cell, vec2<f32>(12.9898, 78.233))) * 43758.547);
    let stipple = mix(mesh_uv.y, scatter, DISSOLVE_SCATTER);
    // At `dissolve` 0 every stipple, from 0 up, is kept; at 1 none is.
    let threshold = params.x * (1.0 + DISSOLVE_EDGE);
    let a = tint.a * smoothstep(threshold - DISSOLVE_EDGE, threshold, stipple);
    return vec4<f32>(color * a, a);
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

// A streak: the sprite's coverage in the tint's color, along the quad's length (mesh_uv.x) from
// its tail at `params.x - params.y` to its head at `params.x`, nothing ahead of the head or
// behind the tail, and fading from full at the head to nothing at the tail. Like a glow it adds
// to what is beneath it.
fn shade_streak(sprite: vec4<f32>, mesh_uv: vec2<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {
    let head = params.x;
    let span = max(params.y, 1e-4);
    let behind = (head - mesh_uv.x) / span;
    let fade = select(0.0, 1.0 - behind, behind >= 0.0 && behind <= 1.0);
    let a = sprite.r * tint.a * fade;
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
    // The fill reaches under the border, which is drawn over it, as CSS's background reaches
    // to the border box's edge (`background-clip: border-box`): a translucent rim shows the
    // panel's fill through it, not what lies behind the panel.
    let edge = vec4<f32>(border) / 255.0;
    let ring = edge.a * (1.0 - inner);
    let fill = vec4<f32>(tint.rgb, 1.0) * tint.a;
    return (vec4<f32>(edge.rgb, 1.0) * ring + fill * (1.0 - ring)) * outer;
}

// The error function, to within 5e-4 (Abramowitz and Stegun's 7.1.27), for each component.
fn erf2(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    var t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    t = t * t;
    return s - s / (t * t);
}

// The Gaussian of standard deviation `sigma` at `x`.
fn gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
}

// How much of a rounded rectangle centred on the origin, `half` its half size with corners of
// `radius`, covers the point `p` once blurred by a Gaussian of standard deviation `sigma`. Across
// x the blur of each horizontal slice is exact (the difference of two error functions); down y
// it is summed at four points within three deviations, which is Evan Wallace's closed form for a
// blurred rounded rectangle. A deviation under a quarter of a pixel is drawn sharp, with the edge
// antialiased over one `pixel`.
fn blurred_box(p: vec2<f32>, half: vec2<f32>, radius: f32, sigma: f32, pixel: f32) -> f32 {
    let corner = min(radius, min(half.x, half.y));
    if (sigma < 0.25 * pixel) {
        return clamp(0.5 - rounded_box(p, half, corner) / pixel, 0.0, 1.0);
    }
    let low = p.y - half.y;
    let high = p.y + half.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i++) {
        let slice = p.y - y;
        let inward = min(half.y - corner - abs(slice), 0.0);
        let reach = half.x - corner + sqrt(max(0.0, corner * corner - inward * inward));
        let ends = 0.5 + 0.5 * erf2((p.x + vec2<f32>(-reach, reach)) * (0.70710678 / sigma));
        value += (ends.y - ends.x) * gaussian(y, sigma) * step;
        y += step;
    }
    return value;
}

// A sixteenth-of-a-point length as `Draw::box_shadow` stores it in the record's `uv`.
fn fixed_points(value: u32) -> f32 {
    return (f32(value) - 32768.0) / 16.0;
}

// A CSS box shadow of an element whose border box (padding box when inset) is `params.xy`
// logical points large with corners of `params.z`, blurred with standard deviation `params.w`.
// The record's `uv` holds the offset, the spread and the shadow shape's corner radius
// (`fixed_points`), and `inset` is its layer. An outer shadow's quad is the box moved by the
// offset and grown by the spread and three deviations, and it is cut away inside the box, as CSS
// draws an outer shadow only outside the border box. An inset shadow's quad is the box, darkened
// everywhere but under the shape (the box moved by the offset and shrunk by the spread).
fn shade_box_shadow(
    mesh_uv: vec2<f32>,
    mesh_pixel: vec2<f32>,
    tint: vec4<f32>,
    shape: vec4<u32>,
    inset: u32,
    params: vec4<f32>,
) -> vec4<f32> {
    let size = params.xy;
    let sigma = params.w;
    let offset = vec2<f32>(fixed_points(shape.x), fixed_points(shape.y));
    let spread = fixed_points(shape.z);
    let shape_radius = fixed_points(shape.w);
    var a = 0.0;
    if (inset == 1u) {
        let pixel = max(max(mesh_pixel.x * size.x, mesh_pixel.y * size.y), 1e-4);
        let p = (mesh_uv - vec2<f32>(0.5)) * size;
        let inside = clamp(0.5 - rounded_box(p, size * 0.5, params.z) / pixel, 0.0, 1.0);
        let half = max(size * 0.5 - vec2<f32>(spread), vec2<f32>(0.0));
        a = (1.0 - blurred_box(p - offset, half, shape_radius, sigma, pixel)) * inside;
    } else {
        let half = max(size * 0.5 + vec2<f32>(spread), vec2<f32>(0.0));
        let quad = 2.0 * (half + vec2<f32>(3.0 * sigma));
        let pixel = max(max(mesh_pixel.x * quad.x, mesh_pixel.y * quad.y), 1e-4);
        let p = (mesh_uv - vec2<f32>(0.5)) * quad;
        let own = clamp(0.5 - rounded_box(p + offset, size * 0.5, params.z) / pixel, 0.0, 1.0);
        a = blurred_box(p, half, shape_radius, sigma, pixel) * (1.0 - own);
    }
    a *= tint.a;
    return vec4<f32>(tint.rgb * a, a);
}

// Stop `at` of `gradient`, premultiplied by its opacity.
fn stop_color(gradient: Gradient, at: u32) -> vec4<f32> {
    let bytes = gradient.colors[at / 4u][at % 4u];
    let color = vec4<f32>(
        f32(bytes & 255u),
        f32((bytes >> 8u) & 255u),
        f32((bytes >> 16u) & 255u),
        f32(bytes >> 24u),
    ) / 255.0;
    return vec4<f32>(color.rgb * color.a, color.a);
}

// The color, premultiplied, of entry `entry` of the frame's gradients at `t` along its line,
// where one pixel spans `step` of the line. Colors between two stops are interpolated in
// premultiplied sRGB, as CSS interpolates them, and a step narrower than a pixel, as at two stops
// in one place, is spread over one pixel.
fn gradient_at(entry: u32, t: f32, step: f32) -> vec4<f32> {
    let gradient = paints.gradients[entry];
    var color = stop_color(gradient, 0u);
    for (var at = 1u; at < 8u; at++) {
        let low = gradient.offsets[(at - 1u) / 4u][(at - 1u) % 4u];
        let high = gradient.offsets[at / 4u][at % 4u];
        let span = max(high - low, step);
        let middle = (low + high) * 0.5;
        let along = clamp((t - middle) / span + 0.5, 0.0, 1.0);
        let between = mix(stop_color(gradient, at - 1u), stop_color(gradient, at), along);
        color = select(color, between, t > middle - span * 0.5);
    }
    return color;
}

// A rounded rectangle `params.xy` logical points large with corners of `params.z`, filled with
// the gradient whose entry is `layer` (less `RADIAL`) and blurred at its edges with standard
// deviation `params.w`; the tint's opacity scales it. Its quad is the rectangle grown by three
// deviations. A linear gradient's angle is `shape.x` in 65,536ths of a turn, 0 up and clockwise,
// and its line runs through the rectangle's middle as long as the rectangle's extent along it. A
// radial gradient's centre (from the top left corner) and radii are `shape`, as `fixed` lengths.
fn shade_gradient(
    mesh_uv: vec2<f32>,
    mesh_pixel: vec2<f32>,
    tint: vec4<f32>,
    shape: vec4<u32>,
    layer: u32,
    params: vec4<f32>,
) -> vec4<f32> {
    let size = params.xy;
    let sigma = params.w;
    let quad = size + vec2<f32>(6.0 * sigma);
    let pixel_size = mesh_pixel * quad;
    let pixel = max(max(pixel_size.x, pixel_size.y), 1e-4);
    let p = (mesh_uv - vec2<f32>(0.5)) * quad;
    let cover = blurred_box(p, size * 0.5, params.z, sigma, pixel);
    var t = 0.0;
    var step = 0.0;
    if ((layer & RADIAL) != 0u) {
        let centre = vec2<f32>(fixed_points(shape.x), fixed_points(shape.y));
        let radii = max(vec2<f32>(fixed_points(shape.z), fixed_points(shape.w)), vec2<f32>(1e-3));
        t = length((p + size * 0.5 - centre) / radii);
        step = pixel / min(radii.x, radii.y);
    } else {
        let angle = f32(shape.x) / 65536.0 * 6.2831853;
        let direction = vec2<f32>(sin(angle), -cos(angle));
        let line = max(dot(abs(direction), size), 1e-4);
        t = dot(p, direction) / line + 0.5;
        step = dot(abs(direction), pixel_size) / line;
    }
    return gradient_at(layer & (RADIAL - 1u), t, step) * (tint.a * cover);
}

// How much the mask of clip entry `entry` keeps of a fragment at `fragment`, in physical pixels:
// its linear gradient's opacity there, 1 for a clip with no mask.
fn mask_at(entry: u32, fragment: vec2<f32>) -> f32 {
    let mask = paints.masks[entry];
    if (mask.line.z == 0.0) {
        return 1.0;
    }
    let size = mask.rect.zw;
    let p = fragment - mask.rect.xy - size * 0.5;
    let direction = vec2<f32>(sin(mask.line.x), -cos(mask.line.x));
    let line = max(dot(abs(direction), size), 1e-4);
    return gradient_at(u32(mask.line.y), dot(p, direction) / line + 0.5, 1.0 / line).a;
}

// A glyph: its coverage (the atlas's one channel) times the tint.
fn shade_glyph(sprite: vec4<f32>, tint: vec4<f32>) -> vec4<f32> {
    let a = sprite.r * tint.a;
    return vec4<f32>(tint.rgb * a, a);
}

// An icon: its coverage (the atlas's one channel) times the tint, as a glyph. Where the icon's
// artwork has a darker fill, its coverage there is scaled down by the fill's brightness.
fn shade_icon(sprite: vec4<f32>, tint: vec4<f32>) -> vec4<f32> {
    let a = sprite.r * tint.a;
    return vec4<f32>(tint.rgb * a, a);
}

@fragment
fn fs(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // A card seen from behind shows its back. A card turns over top over bottom, so there its
    // texture coordinates run bottom to top.
    let back = in.material == CARD && !front;
    let layer = select(in.layers.x, in.layers.y, back);
    let uv = select(in.uv, vec2<f32>(in.uv.x, 1.0 - in.uv.y), back);
    // Sampled before branching on the material, which varies per instance: WGSL allows
    // `textureSample` only in uniform control flow.
    let picture = textureSample(pictures, picture_sampler, uv, i32(layer));
    // Derivatives too are allowed only in uniform control flow.
    let mesh_pixel = fwidth(in.mesh_uv);
    var color = vec4<f32>(0.0);
    switch in.material {
        case TABLE: { color = shade_table(in.tint, in.params, in.clip.xy); }
        case CARD: { color = shade_card(picture, in.mesh_uv, in.tint, in.params); }
        case CONTACT_SHADOW: { color = shade_contact_shadow(in.mesh_uv, in.tint, in.params); }
        case GLOW: { color = shade_glow(picture, in.tint, in.params); }
        case STREAK: { color = shade_streak(picture, in.mesh_uv, in.tint, in.params); }
        case PANEL: { color = shade_panel(in.mesh_uv, mesh_pixel, in.tint, in.rect, in.params); }
        case GLYPH: { color = shade_glyph(picture, in.tint); }
        case ICON: { color = shade_icon(picture, in.tint); }
        case BOX_SHADOW: { color = shade_box_shadow(in.mesh_uv, mesh_pixel, in.tint, in.rect, in.layers.x, in.params); }
        case GRADIENT: { color = shade_gradient(in.mesh_uv, mesh_pixel, in.tint, in.rect, in.layers.x, in.params); }
        default: {}
    }
    if (in.masked_by != 0u) {
        color *= mask_at(in.masked_by, in.clip.xy);
    }
    return color * (1.0 - in.edge);
}
