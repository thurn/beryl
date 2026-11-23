use ignite_core::protocol::tickets::TicketQuery;

/// The query string of `GET /tickets`, in the names the hub reads.
pub(super) fn ticket_query(query: &TicketQuery) -> Vec<(&'static str, String)> {
    let spelled = |value: serde_json::Value| value.as_str().map(str::to_owned);
    let mut pairs = Vec::new();
    if let Some(status) = query.status.and_then(|s| spelled(serde_json::json!(s))) {
        pairs.push(("status", status));
    }
    if let Some(kind) = query
        .ticket_type
        .and_then(|t| spelled(serde_json::json!(t)))
    {
        pairs.push(("type", kind));
    }
    let named = [
        ("parent", query.parent.as_ref().map(ToString::to_string)),
        ("ancestor", query.ancestor.as_ref().map(ToString::to_string)),
        ("search", query.search.clone()),
        ("assignee", query.assignee.clone()),
        ("limit", query.limit.map(|l| l.to_string())),
    ];
    pairs.extend(named.into_iter().filter_map(|(k, v)| Some((k, v?))));
    pairs.extend(query.label.iter().map(|l| ("label", l.clone())));
    pairs.extend(query.label_any.iter().map(|l| ("label_any", l.clone())));
    for (flag, set) in [("ready", query.ready), ("all", query.all)] {
        if set {
            pairs.push((flag, "1".to_owned()));
        }
    }
    pairs
}
