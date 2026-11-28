// Dedicated Enhanced-tier procedural VFX entry points. Runs in its own pipeline so procedural
// effects never add branches or shadow/atlas texture samples to Baseline's `shader.wgsl` or
// Enhanced's lit `fs_lit`.

struct VfxUniforms {
    view_projection: mat4x4<f32>,
    // The target's size in physical pixels.
    target_size: vec2<f32>,
    // How far the picture of the table is shaken, in physical pixels, x right and y down.
    shake: vec2<f32>,
    // Unused here; kept to match bind group 0's uniform layout.
    masked: vec4<u32>,
};

override SHAKEN: bool = true;

@group(0) @binding(0) var<uniform> u: VfxUniforms;

struct VfxVertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(4) row0: vec4<f32>,
    @location(5) row1: vec4<f32>,
    @location(6) row2: vec4<f32>,
    @location(7) rect: vec4<u32>,
    @location(9) tint: vec4<f32>,
    @location(10) params: vec4<f32>,
};

struct VfxOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) mesh_uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
    @location(3) @interpolate(flat) style: u32,
    @location(4) @interpolate(flat) params: vec4<f32>,
};

@vertex
fn vs_vfx(v: VfxVertexIn) -> VfxOut {
    let kind = v.rect.x;
    var p = v.position;
    var kept = 1.0;
    if (kind == VFX_LINE_RIBBON) {
        let t_start = max(f32(v.rect.y & 255u) / 255.0, 1e-3);
        let t_end = max(f32((v.rect.y >> 8u) & 255u) / 255.0, 1e-3);
        let along = v.position.x + 0.5;
        kept = mix(t_start, t_end, along);
        p.y *= kept;

        let s_start = f32(v.rect.z & 255u) / 127.5 - 1.0;
        let s_end = f32((v.rect.z >> 8u) & 255u) / 127.5 - 1.0;
        let across = length(vec3<f32>(v.row0.y, v.row1.y, v.row2.y));
        let long = max(length(vec3<f32>(v.row0.x, v.row1.x, v.row2.x)), 1e-6);
        p.x += p.y * mix(s_start, s_end, along) * across / long;
    }
    let local = vec4<f32>(p, 0.0, 1.0);
    let position = vec3<f32>(dot(v.row0, local), dot(v.row1, local), dot(v.row2, local));
    var clip = u.view_projection * vec4<f32>(position, 1.0);
    if (SHAKEN) {
        clip = vec4<f32>(
            clip.xy + vec2<f32>(u.shake.x, -u.shake.y) * 2.0 / u.target_size * clip.w,
            clip.zw,
        );
    }
    var out: VfxOut;
    out.clip = clip / kept;
    out.mesh_uv = v.uv;
    out.tint = v.tint;
    out.kind = kind;
    out.style = v.rect.w & 255u;
    out.params = v.params;
    return out;
}

fn dispatch_vfx(in: VfxOut) -> vec4<f32> {
    let time_s = in.params.x;
    let fade = in.params.y * in.tint.a;
    let seed = in.params.z;
    let aspect = in.params.w;
    let tint = vec4<f32>(in.tint.rgb, fade);
    switch in.kind {
        case VFX_CHEVRON: {
            return shade_attacker_chevron(in.mesh_uv, fade, time_s, seed, aspect, false);
        }
        case VFX_CHEVRON_BLOCKED: {
            return shade_attacker_chevron(in.mesh_uv, fade, time_s, seed, aspect, true);
        }
        case VFX_SHIELD_DOME: {
            return shade_blocker_shield(in.mesh_uv, fade, time_s, seed, aspect);
        }
        case VFX_LINE_RIBBON: {
            return shade_line_ribbon(in.mesh_uv, in.tint.a, in.style, in.params);
        }
        case VFX_LINE_HEAD: {
            return shade_line_head(in.mesh_uv, fade, in.style, in.params);
        }
        case VFX_PROBE: {
            return shade_probe(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_COUNTER_ADD: {
            return shade_counter_add(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_COUNTER_ADD_NEG: {
            return shade_counter_add_neg(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_COUNTER_REMOVE: {
            return shade_counter_remove(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_SHIELD_ADD: {
            return shade_shield_add(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_SHIELD_REMOVE: {
            return shade_shield_remove(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_SETTLE: {
            return shade_settle(in.mesh_uv, time_s, seed, aspect, tint, in.style);
        }
        case VFX_HIGHLIGHT_SHIMMER: {
            return shade_highlight_shimmer(in.mesh_uv, time_s, seed, aspect, tint, in.style);
        }
        case VFX_HIGHLIGHT_POP: {
            return shade_highlight_pop(in.mesh_uv, time_s, seed, aspect, tint, in.style);
        }
        case VFX_MYTHIC_ARRIVAL: {
            return shade_mythic_arrival(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_MANA_BIRTH: {
            return shade_mana_birth(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_MANA_HIT: {
            return shade_mana_hit(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_STACK_ENTER: {
            return shade_stack_enter(in.mesh_uv, time_s, seed, aspect, tint);
        }
        case VFX_STACK_EXIT: {
            return shade_stack_exit(in.mesh_uv, time_s, seed, aspect, tint);
        }
        default: {
            return vec4<f32>(0.0);
        }
    }
}

fn vfx_bloom_gain(kind: u32, style: u32) -> f32 {
    switch kind {
        case VFX_CHEVRON: {
            return BLOOM_GAIN_UNBLOCKED;
        }
        case VFX_CHEVRON_BLOCKED, VFX_SHIELD_DOME: {
            return BLOOM_GAIN_GOLDEN;
        }
        case VFX_LINE_RIBBON, VFX_LINE_HEAD: {
            return line_bloom_gain(style);
        }
        case VFX_PROBE: {
            return BLOOM_GAIN_PROBE;
        }
        case VFX_COUNTER_ADD: {
            return BLOOM_GAIN_COUNTER_ADD;
        }
        case VFX_COUNTER_ADD_NEG: {
            return BLOOM_GAIN_COUNTER_NEG;
        }
        case VFX_COUNTER_REMOVE: {
            return BLOOM_GAIN_COUNTER_REMOVE;
        }
        case VFX_SHIELD_ADD: {
            return BLOOM_GAIN_SHIELD_ADD;
        }
        case VFX_SHIELD_REMOVE: {
            return BLOOM_GAIN_SHIELD_REMOVE;
        }
        case VFX_SETTLE: {
            return BLOOM_GAIN_SETTLE;
        }
        case VFX_HIGHLIGHT_SHIMMER: {
            return BLOOM_GAIN_HIGHLIGHT_SHIMMER;
        }
        case VFX_HIGHLIGHT_POP: {
            return BLOOM_GAIN_HIGHLIGHT_POP;
        }
        case VFX_MYTHIC_ARRIVAL: {
            return BLOOM_GAIN_MYTHIC_ARRIVAL;
        }
        case VFX_MANA_BIRTH: {
            return BLOOM_GAIN_MANA_BIRTH;
        }
        case VFX_MANA_HIT: {
            return BLOOM_GAIN_MANA_HIT;
        }
        case VFX_STACK_ENTER: {
            return BLOOM_GAIN_STACK_ENTER;
        }
        case VFX_STACK_EXIT: {
            return BLOOM_GAIN_STACK_EXIT;
        }
        default: {
            return 0.0;
        }
    }
}

@fragment
fn fs_vfx(in: VfxOut) -> @location(0) vec4<f32> {
    return dispatch_vfx(in);
}

@fragment
fn fs_vfx_bloom(in: VfxOut) -> @location(0) vec4<f32> {
    let color = dispatch_vfx(in);
    return vec4<f32>(color.rgb * vfx_bloom_gain(in.kind, in.style), 0.0);
}
