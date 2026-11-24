use beryl_table::Action;
use serde_json::{Value, json};

const REGISTRY: &str = include_str!("../../../../data/table-commands.json");

fn example(placeholder: &str, variant: &str) -> Value {
    match placeholder {
        "$objects" => json!([5, 9]),
        "$in" => json!("battlefield"),
        "$seat" | "$owner" | "$winner" => json!(0),
        "$other_seat" => json!(1),
        "$number" | "$count" | "$entry" => json!(2),
        "$text" => json!("example text"),
        "$target" if variant == "arrow" || variant == "clear_arrows" => json!({"player": 1}),
        "$target" | "$source" => json!({"id": 3, "in": "battlefield"}),
        "$stats" => json!([3, 2]),
        "$tapped" | "$on" => json!(true),
        "$face" => json!("front"),
        "$card" => json!({"named": "Treasure"}),
        "$actions" => json!([{"draw": {"seat": 0, "count": 1}}]),
        "$first_counter" => json!("+1/+1"),
        "$second_counter" => json!("−1/−1"),
        "$third_counter" => json!("charge"),
        "$phase" => json!("first_main"),
        unknown => panic!("unknown placeholder {unknown}"),
    }
}

fn fill(value: &mut Value, variant: &str) {
    match value {
        Value::String(text) if text.starts_with('$') => *value = example(text, variant),
        Value::Array(items) => items.iter_mut().for_each(|item| fill(item, variant)),
        Value::Object(fields) => fields.values_mut().for_each(|field| fill(field, variant)),
        _ => {}
    }
}

#[test]
fn registry_templates_deserialize_as_table_actions() {
    let commands: Vec<Value> = serde_json::from_str(REGISTRY).unwrap();
    for command in commands {
        let id = command["id"].as_str().unwrap();
        match (command.get("action"), command.get("client")) {
            (Some(template), None) => {
                let mut action = template.clone();
                let fields = action
                    .as_object_mut()
                    .filter(|fields| fields.len() == 1)
                    .unwrap_or_else(|| panic!("{id}: expected one action variant"));
                for (variant, payload) in fields {
                    fill(payload, variant);
                }
                let parsed: Action = serde_json::from_value(action.clone())
                    .unwrap_or_else(|error| panic!("{id}: {action}: {error}"));
                // Serde can otherwise accept stale fields it silently ignores.
                let canonical = serde_json::to_value(parsed).unwrap();
                for (variant, payload) in action.as_object().unwrap() {
                    for (field, expected) in payload.as_object().unwrap() {
                        assert_eq!(&canonical[variant][field], expected, "{id}: {field}");
                    }
                }
            }
            (None, Some(Value::String(client))) if !client.is_empty() => {}
            _ => panic!("{id}: expected one action or client operation"),
        }
    }
}
