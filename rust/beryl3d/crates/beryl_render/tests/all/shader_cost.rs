//! What each material's shading costs per fragment, held to the limits under `materials` in
//! `data/beryl3d/gates.json` (docs/beryl-3d.md, "What is counted"). GPU time cannot be counted,
//! so the shader's work is: each material is shaded by a WGSL function named `shade_` and the
//! material, and this test parses the shader with naga, the compiler wgpu uses, and counts each
//! such function's texture samples and expressions. The fragment entry point's own counts are
//! added to every material's, since it runs for every fragment whatever the material.
//!
//! No Baseline material may take more than one texture sample per fragment.
//!
//! The counts are printed as one line, `materials {...}`, which `just b3d gate --gpu` reads.

use std::collections::BTreeMap;

use naga::{Expression, Function, ShaderStage};
use serde_json::{Value, json};

const SHADER: &str = include_str!("../../src/shader.wgsl");
const GATES: &str = include_str!("../../../../../../data/beryl3d/gates.json");
/// The most texture samples a Baseline material may take per fragment.
const MOST_SAMPLES: u64 = 1;

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

#[test]
fn each_material_is_within_its_limits() {
    let costs = costs(SHADER);
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
        if cost.samples > MOST_SAMPLES {
            faults.push(format!(
                "{name} takes {} texture samples per fragment; a Baseline material may take \
                 {MOST_SAMPLES}",
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
        "fn shade_card(face: vec4<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {",
        "fn shade_card(face: vec4<f32>, tint: vec4<f32>, params: vec4<f32>) -> vec4<f32> {\n    \
         let again = textureSample(pictures, picture_sampler, vec2<f32>(0.0), 0);",
        1,
    );
    assert_ne!(planted, SHADER, "the planted line found `shade_card`");
    let after = costs(&planted)["card"];
    assert_eq!(after.samples, before.samples + 1);
    assert!(after.samples > MOST_SAMPLES);
}
