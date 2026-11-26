//! Checked-in campaign plans parse, validate, and resolve every family name.

use std::path::Path;

use beryl_cli::campaign::plan::Plan;

#[test]
fn checked_in_campaign_plans_validate() {
    let dir = Path::new("data/campaigns");
    let mut found = 0usize;
    let entries =
        beryl_test_files::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let name = entry.unwrap().file_name();
        let rel = dir.join(&name);
        if rel.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let text = beryl_test_files::read_to_string(&rel)
            .unwrap_or_else(|e| panic!("{}: {e}", rel.display()));
        let plan: Plan = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} failed to parse: {e}", rel.display()));
        plan.validate()
            .unwrap_or_else(|e| panic!("{} failed validation:\n{e}", rel.display()));
        plan.smoke()
            .validate()
            .unwrap_or_else(|e| panic!("{} smoke plan failed validation:\n{e}", rel.display()));
        found += 1;
    }
    assert!(
        found > 0,
        "expected at least one campaign plan in {}",
        dir.display()
    );
}
