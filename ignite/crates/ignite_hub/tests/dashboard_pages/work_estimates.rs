use super::{json, seeded};

#[test]
fn epic_percentage_counts_unfiled_work_and_discloses_the_estimate_on_hover() {
    let mut hub = seeded();
    hub.conn
        .execute_batch(
            "DELETE FROM tickets WHERE parent = 'TQ44S' AND status = 'closed' AND id NOT IN (
                 SELECT id FROM tickets WHERE parent = 'TQ44S' AND status = 'closed' ORDER BY id LIMIT 9
             );
             UPDATE tickets SET estimate_minutes = 10 WHERE parent = 'TQ44S' AND status = 'closed';
             UPDATE tickets SET estimate_minutes = 30, unfiled_minutes = 600 WHERE id = 'TQ44S.7.24';
             UPDATE tickets SET estimate_minutes = 50 WHERE id = 'TQ44S.7.24.1';
             UPDATE tickets SET estimate_minutes = 66 WHERE id = 'TWAIT';",
        )
        .unwrap();

    let home = hub.text("/");
    assert!(
        home.contains(
            "1h 30m estimated closed · 11h 20m estimated left · 10h unfiled (agent time)"
        )
    );
    assert!(home.contains(">11%</span>"));
    assert!(home.contains(r#"class="c-progress-fill" style="width:11%""#));
    let milestone = hub.text("/TQ44S.7");
    assert!(
        milestone
            .contains("0m estimated closed · 11h 20m estimated left · 10h unfiled (agent time)")
    );
    assert!(milestone.contains(">0%</span>"));

    hub.batch(&json!({"op": "close", "id": "TQ44S.7.24", "reason": "Review complete"}));
    assert!(
        hub.text("/TQ44S.7")
            .contains("30m estimated closed · 10h 50m estimated left · 10h unfiled")
    );
    hub.batch(&json!({"op": "update", "id": "TQ44S.7.24", "set": {"unfiled_minutes": null}}));
    assert!(hub.text("/TQ44S.7").contains(">37%</span>"));

    hub.batch(&json!({"op": "update", "id": "TQ44S.7.24.1", "set": {"estimate_minutes": null}}));
    let milestone = hub.text("/TQ44S.7");
    assert!(milestone.contains("30m estimated closed · 17m estimated left · 1 unestimated ticket"));
    assert!(milestone.contains("Repository average of 17m per unestimated ticket"));
    assert!(milestone.contains(r#"class="c-progress-fill" style="width:63%""#));
    assert!(milestone.contains("≈63%"));

    let sized = hub.text("/TQ44S.7.24");
    assert!(sized.contains("<dt>Work estimate</dt><dd>30m</dd>"));
    let defaulted = hub.text("/TQ44S.7.24.1");
    assert!(defaulted.contains("<dt>Work estimate</dt>"));
    assert!(defaulted.contains("≈17m"));
}
