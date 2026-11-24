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
fn documentation_and_trailing_derive_commas_do_not_affect_the_wire_fingerprint() {
    let documented = r"//! Module docs.
        #[derive(Serialize, Deserialize,)]
        enum Message {
            /// Variant docs.
            Update(u32),
        }";
    let rust = "#[derive(Serialize, Deserialize)] enum Message { Update(u32), }";
    assert_eq!(fingerprint(documented), fingerprint(rust));
}

#[test]
fn the_wire_hash_covers_additional_definitions() {
    use beryl_protocol::{WIRE_HASH, wire_hash};

    assert_eq!(WIRE_HASH.len(), 16);
    assert!(WIRE_HASH.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(wire_hash(&[]), WIRE_HASH);
    assert_ne!(wire_hash(&["action"]), WIRE_HASH);
    assert_ne!(
        wire_hash(&["action", "lobby"]),
        wire_hash(&["action", "log"])
    );
    assert_ne!(wire_hash(&["act", "ion"]), wire_hash(&["action"]));
}

#[test]
fn each_prompt_purpose_round_trips_as_its_name() {
    use beryl_protocol::prompt::{PartName, PromptView, Purpose, ShapeBody, Words};

    for &purpose in Purpose::ALL {
        let view = PromptView {
            revision: 0,
            purpose,
            part: PartName::Pick,
            title: String::new(),
            words: Words::default(),
            count: None,
            about: Vec::new(),
            from: None,
            marks: Vec::new(),
            previews: Vec::new(),
            buttons: Vec::new(),
            body: ShapeBody::Candidates,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["purpose"], purpose.name());
        assert_eq!(
            serde_json::from_value::<PromptView>(json).unwrap().purpose,
            purpose
        );
    }
}
