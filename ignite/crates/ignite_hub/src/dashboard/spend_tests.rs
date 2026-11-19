use super::*;

fn segment(group: &str, kind: &str, weight: f64) -> Item {
    Item::Segment(Block {
        group: group.to_owned(),
        category: "tool".to_owned(),
        kind: kind.to_owned(),
        weight,
        runs: 1,
        ..Block::default()
    })
}

fn wait(weight: f64) -> Item {
    Item::Break(Block {
        group: "person".to_owned(),
        category: "person".to_owned(),
        weight,
        runs: 1,
        ..Block::default()
    })
}

#[test]
fn coalescing_keeps_the_weight_and_every_break_and_names_the_mix() {
    let mut items = Vec::new();
    for index in 0..300 {
        if index % 100 == 50 {
            items.push(wait(500.0));
        }
        let kind = if index % 2 == 0 {
            "git"
        } else {
            "read and search"
        };
        items.push(segment("tools", kind, 1.0 + f64::from(index % 3)));
    }
    let before: f64 = items.iter().map(|item| item.block().weight).sum();
    let found = coalesced(items, 20);
    let after: f64 = found.iter().map(|item| item.block().weight).sum();
    assert!((before - after).abs() < 1e-9);
    assert_eq!(
        found
            .iter()
            .filter(|item| matches!(item, Item::Break(_)))
            .count(),
        3
    );
    assert!(found.len() <= 20 + 3 + 3);
    let mixed = found
        .iter()
        .find(|item| !item.block().mix.is_empty())
        .unwrap();
    assert!(
        mixed
            .block()
            .mix
            .iter()
            .all(|(name, _)| name == "git" || name == "read and search")
    );
}

#[test]
fn neighbouring_runs_of_one_kind_join_without_a_mix() {
    let found = coalesced(
        vec![
            segment("tools", "git", 1.0),
            segment("tools", "git", 2.0),
            segment("checks", "build", 1.0),
        ],
        10,
    );
    assert_eq!(found.len(), 2);
    assert!((found[0].block().weight - 3.0).abs() < 1e-9);
    assert!(found[0].block().mix.is_empty());
    assert_eq!(found[0].block().runs, 2);
}

#[test]
fn labels_name_categories_and_shorten_tool_names() {
    let block = |category: &str, kind: &str, part: &str| Block {
        category: category.to_owned(),
        kind: kind.to_owned(),
        part: part.to_owned(),
        ..Block::default()
    };
    assert_eq!(label(&block("model", "", "")), "Model");
    assert_eq!(label(&block("model", "reply", "")), "Model");
    assert_eq!(
        label(&block("queued", "rust tests", "")),
        "Queued: rust tests"
    );
    assert_eq!(
        label(&block("tool", "rust tests", "compile")),
        "rust tests · compile"
    );
    assert_eq!(
        label(&block("tool", "mcp__ccd_turn__update_plan", "")),
        "update_plan"
    );
    assert_eq!(label(&block("person", "question", "")), "Question");
}
