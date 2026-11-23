#[path = "../../src/wire_fingerprint.rs"]
mod wire_fingerprint;
#[path = "../../wire_schema.rs"]
mod wire_schema;

fn fingerprint(source: &str) -> u64 {
    wire_schema::fingerprint(0xcbf2_9ce4_8422_2325, source).unwrap()
}

#[test]
fn serde_names_and_types_affect_the_wire_fingerprint() {
    let action = r#"#[derive(Serialize, Deserialize)]
        enum Action { Draw { #[serde(rename = "seat")] player: u8 } }"#;
    let baseline = fingerprint(action);
    assert_ne!(
        fingerprint(&action.replace("\"seat\"", "\"player\"")),
        baseline
    );
    assert_ne!(fingerprint(&action.replace("u8", "u32")), baseline);
}

#[test]
fn legacy_export_attributes_and_documentation_do_not_affect_the_wire_fingerprint() {
    let exported = r#"//! Module docs.
        use ts_rs::TS;
        #[derive(Serialize, Deserialize, TS)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "Export"))]
        enum Message {
            /// Variant docs.
            Update(#[ts(type = "number")] u32),
        }"#;
    let rust = "#[derive(Serialize, Deserialize)] enum Message { Update(u32), }";
    assert_eq!(fingerprint(exported), fingerprint(rust));
    assert_ne!(
        fingerprint(&exported.replace("ts(rename", "serde(rename")),
        fingerprint(rust)
    );
}
