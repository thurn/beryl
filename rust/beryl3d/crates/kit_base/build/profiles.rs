//! Keeps the old constant paths available while the table and HUD ports move to profiles.

use std::fs;
use std::path::Path;

use serde_json::Value;

pub(super) fn legacy(data: &Path, space: &mut Value, audit: &mut Value) {
    for name in ["desktop", "phone"] {
        let path = data.join("profiles").join(format!("{name}.json"));
        println!("cargo:rerun-if-changed={}", path.display());
        let mut data: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        // Export the values still used by the HUD and solver tests at their original paths.
        let mut table = data["table"].take();
        if let Some(mut banded) = table.as_object_mut().unwrap().remove("banded") {
            for key in ["piles", "stack"] {
                table[key] = banded[key].take();
            }
            let hud = &mut data["legacy"]["hud.phone"];
            for key in ["dock_gap", "button_height"] {
                hud[key] = banded[key].take();
            }
        }
        if name == "phone" {
            data["legacy"]["hud.phone"]["orb_scale"] = data["play"]["life"]["orb_scale"].take();
        }
        insert(space, &format!("layout.{name}"), table);
        for (path, value) in data["legacy"].as_object_mut().unwrap() {
            insert(space, path, value.take());
        }
        space["counts"]
            .as_array_mut()
            .unwrap()
            .append(data["counts"].as_array_mut().unwrap());
        if let Some(limit) = data.get_mut("shorter_side_below") {
            insert(space, "flat.phone_under", limit.take());
            insert(
                space,
                "hud.phone.button_min_width",
                data["settings_quick_action_min_width"].take(),
            );
            for field in ["action_height", "action_gap", "action_text"] {
                insert(
                    space,
                    &format!("flat.phone.{field}"),
                    data["screen"][field].take(),
                );
            }
            audit["phone_target_points"] = data["checks"]["target_points"].take();
            audit["phone_text_points"] = data["checks"]["text_points"].take();
        } else {
            audit["target_points"] = data["checks"]["target_points"].take();
        }
    }
}

fn insert(root: &mut Value, path: &str, value: Value) {
    let mut parent = root;
    let (parents, leaf) = path.rsplit_once('.').unwrap_or(("", path));
    for key in parents.split('.').filter(|key| !key.is_empty()) {
        parent = &mut parent[key];
    }
    assert!(parent[leaf].is_null(), "duplicate spatial tuning: {path}");
    parent[leaf] = value;
}
