//! What each material's shading costs per fragment, held to the limits under `materials` in
//! `data/beryl3d/gates.json` (docs/beryl-3d.md, "What is counted"). GPU time cannot be counted,
//! so the shader's work is: each material is shaded by a WGSL function named `shade_` and the
//! material, and this test parses the shader with naga, the compiler wgpu uses, and counts each
//! such function's texture samples and expressions. The fragment entry point's own counts are
//! added to every material's, since it runs for every fragment whatever the material.
//!
//! No Baseline material may take more than one texture sample per fragment.
//!
//! The Enhanced shader's materials, `shade_card_lit`, `shade_table_lit` and `shade_particle`,
//! are counted as `card_lit`, `table_lit` and `particle`. Their functions call others, such as
//! the Baseline function a lit material starts from, so each one's count adds every function it
//! calls, transitively, and the work its entry point `fs_lit` does for every fragment: its own
//! expressions and the functions it calls that are not materials, such as the shadow map's
//! `lit_by_key`. An Enhanced material may take five samples: the face and four taps of the
//! shadow map.
//!
//! The counts are printed as one line, `materials {...}`, which `just b3d gate --gpu` reads.

use std::collections::BTreeMap;

use std::collections::BTreeSet;

use naga::{Block, Expression, Function, Handle, Module, ShaderStage, Statement};
use serde_json::{Value, json};

const SHADER: &str = include_str!("../../src/shader.wgsl");
const ENHANCED: &str = beryl_render::ENHANCED_SHADER;
const GATES: &str = include_str!("../../../../../../data/beryl3d/gates.json");
/// The most texture samples a Baseline material may take per fragment.
const MOST_SAMPLES: u64 = 1;
/// The most texture samples an Enhanced material may take per fragment.
const MOST_ENHANCED_SAMPLES: u64 = 5;
/// The Enhanced materials' functions, each with the name its counts go under.
const ENHANCED_MATERIALS: [(&str, &str); 3] = [
    ("shade_card_lit", "card_lit"),
    ("shade_table_lit", "table_lit"),
    ("shade_particle", "particle"),
];
/// The FNV-1a hash of `src/shader.wgsl`'s bytes. The Baseline shader is frozen by the Enhanced
/// tier, which reuses its functions and repeats its entry points; a deliberate change to it
/// updates this constant in the same commit, after checking that the Enhanced shader still
/// matches it.
const BASELINE_SHADER_HASH: u64 = 0xc645_5c72_cb5c_2521;

/// A material's counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Cost {
    samples: u64,
    expressions: u64,
}

fn cost(function: &Function) -> Cost {
    let samples = function
        .expressions
        .iter()
        .filter(|(_, expression)| matches!(expression, Expression::ImageSample { .. }))
        .count();
    Cost {
        samples: samples as u64,
        expressions: function.expressions.len() as u64,
    }
}

/// Each material's cost, by the name after `shade_`.
fn costs(source: &str) -> BTreeMap<String, Cost> {
    let module = naga::front::wgsl::parse_str(source).expect("the shader parses");
    let entry = module
        .entry_points
        .iter()
        .find(|entry| entry.stage == ShaderStage::Fragment)
        .expect("the shader has a fragment entry point");
    let own = cost(&entry.function);
    module
        .functions
        .iter()
        .filter_map(|(_, function)| {
            let material = function.name.as_deref()?.strip_prefix("shade_")?;
            let Cost {
                samples,
                expressions,
            } = cost(function);
            Some((
                material.to_owned(),
                Cost {
                    samples: samples + own.samples,
                    expressions: expressions + own.expressions,
                },
            ))
        })
        .collect()
}

/// The functions `function` calls, directly.
fn callees(function: &Function) -> Vec<Handle<Function>> {
    fn walk(block: &Block, found: &mut Vec<Handle<Function>>) {
        for statement in block {
            match statement {
                Statement::Call { function, .. } => found.push(*function),
                Statement::Block(inner) => walk(inner, found),
                Statement::If { accept, reject, .. } => {
                    walk(accept, found);
                    walk(reject, found);
                }
                Statement::Switch { cases, .. } => {
                    for case in cases {
                        walk(&case.body, found);
                    }
                }
                Statement::Loop {
                    body, continuing, ..
                } => {
                    walk(body, found);
                    walk(continuing, found);
                }
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    walk(&function.body, &mut found);
    found
}

/// The cost of `start` and of every function it calls, transitively, each counted once, leaving
/// out the callees `skip` accepts.
fn transitive(module: &Module, start: &Function, skip: &dyn Fn(&Function) -> bool) -> Cost {
    let mut total = cost(start);
    let mut seen = BTreeSet::new();
    let mut next = callees(start);
    while let Some(handle) = next.pop() {
        let function = &module.functions[handle];
        if skip(function) || !seen.insert(handle) {
            continue;
        }
        let Cost {
            samples,
            expressions,
        } = cost(function);
        total.samples += samples;
        total.expressions += expressions;
        next.extend(callees(function));
    }
    total
}

/// The Enhanced materials' costs, by the names in `ENHANCED_MATERIALS`.
fn enhanced_costs(source: &str) -> BTreeMap<String, Cost> {
    let module = naga::front::wgsl::parse_str(source).expect("the Enhanced shader parses");
    let entry = module
        .entry_points
        .iter()
        .find(|entry| entry.name == "fs_lit")
        .expect("the Enhanced shader has `fs_lit`");
    let material = |function: &Function| {
        function
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("shade_"))
    };
    let own = transitive(&module, &entry.function, &material);
    ENHANCED_MATERIALS
        .iter()
        .map(|&(function, name)| {
            let (_, found) = module
                .functions
                .iter()
                .find(|(_, f)| f.name.as_deref() == Some(function))
                .unwrap_or_else(|| panic!("the Enhanced shader has `{function}`"));
            let cost = transitive(&module, found, &|_| false);
            (
                name.to_owned(),
                Cost {
                    samples: cost.samples + own.samples,
                    expressions: cost.expressions + own.expressions,
                },
            )
        })
        .collect()
}

/// The most texture samples the material counted under `name` may take per fragment.
fn most_samples(name: &str) -> u64 {
    if ENHANCED_MATERIALS
        .iter()
        .any(|&(_, enhanced)| enhanced == name)
    {
        MOST_ENHANCED_SAMPLES
    } else {
        MOST_SAMPLES
    }
}

#[test]
fn each_material_is_within_its_limits() {
    let mut costs = costs(SHADER);
    assert!(!costs.is_empty(), "the shader has no `shade_` functions");
    costs.extend(enhanced_costs(ENHANCED));
    assert!(!costs.is_empty(), "the shader has no `shade_` functions");
    let printed: BTreeMap<_, _> = costs
        .iter()
        .map(|(name, cost)| {
            (
                name.clone(),
                json!({ "samples": cost.samples, "expressions": cost.expressions }),
            )
        })
        .collect();
    println!("materials {}", json!(printed));

    let gates: Value = serde_json::from_str(GATES).expect("gates.json is JSON");
    let limits = &gates["materials"];
    let mut faults = Vec::new();
    for (name, cost) in &costs {
        let most = most_samples(name);
        if cost.samples > most {
            faults.push(format!(
                "{name} takes {} texture samples per fragment; it may take {most}",
                cost.samples
            ));
        }
        for (key, value) in [("samples", cost.samples), ("expressions", cost.expressions)] {
            match limits[name][key].as_u64() {
                None => faults.push(format!(
                    "{name}: {key} is {value} with no limit under materials.{name}.{key}; \
                     `just b3d gate --gpu --update` sets it"
                )),
                Some(limit) if value > limit => {
                    faults.push(format!("{name}: {key} is {value}, over its limit {limit}"));
                }
                Some(_) => {}
            }
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

/// The count sees a sample taken inside a material's function, and adds the entry point's.
#[test]
fn a_second_sample_in_a_material_is_counted() {
    let before = costs(SHADER)["card"];
    let planted = SHADER.replacen(
        "fn shade_card(face: vec4<f32>, mesh_uv: vec2<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {",
        "fn shade_card(face: vec4<f32>, mesh_uv: vec2<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {\n    \
         let again = textureSample(pictures, picture_sampler, vec2<f32>(0.0), 0);",
        1,
    );
    assert_ne!(planted, SHADER, "the planted line found `shade_card`");
    let after = costs(&planted)["card"];
    assert_eq!(after.samples, before.samples + 1);
    assert!(after.samples > MOST_SAMPLES);
}

/// The Enhanced count adds the samples of a function a material calls, and of one `fs_lit`
/// calls for every fragment.
#[test]
fn the_enhanced_count_follows_calls() {
    let before = enhanced_costs(ENHANCED);
    let sample = "textureSample(pictures, picture_sampler, vec2<f32>(0.0), 0)";
    let planted = ENHANCED.replacen(
        "    let color = shade_card(face, mesh_uv, tint, params);",
        "    let color = shade_card(face, mesh_uv, tint, params) + extra();",
        1,
    ) + &format!("\nfn extra() -> vec4<f32> {{ return {sample}; }}\n");
    let after = enhanced_costs(&planted);
    assert_eq!(after["card_lit"].samples, before["card_lit"].samples + 1);
    assert_eq!(after["table_lit"], before["table_lit"]);
    let shadowed = ENHANCED.replacen(
        "fn lit_by_key(light_position: vec4<f32>) -> f32 {",
        &format!("fn lit_by_key(light_position: vec4<f32>) -> f32 {{\n    let tap = {sample};"),
        1,
    );
    assert_ne!(shadowed, ENHANCED, "the planted line found `lit_by_key`");
    let after = enhanced_costs(&shadowed);
    for (_, name) in ENHANCED_MATERIALS {
        assert_eq!(after[name].samples, before[name].samples + 1, "{name}");
    }
}

/// FNV-1a, 64 bits.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn the_baseline_shader_does_not_change() {
    assert_eq!(
        fnv1a(SHADER.as_bytes()),
        BASELINE_SHADER_HASH,
        "src/shader.wgsl changed; the Enhanced tier reuses it (see BASELINE_SHADER_HASH)"
    );
}
