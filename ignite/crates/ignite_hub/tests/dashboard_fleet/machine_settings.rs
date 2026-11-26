use super::*;

fn assert_cygnus_tree_fields(page: &str, editable: bool) {
    let cygnus = machine_settings(page, "cygnus");
    let values = if editable {
        &[
            r#"data-fleet-field="trees_root" aria-label="cygnus trees root" value="D:\trees""#,
            r#"data-fleet-field="tree_space_gb" aria-label="cygnus tree space in GB" value="90""#,
            r#"data-fleet-field="tree_ceiling_gb" aria-label="cygnus tree ceiling in GB" value="40""#,
        ][..]
    } else {
        &[r">D:\trees<", ">90<", ">40<"]
    };
    for value in values {
        assert!(cygnus.contains(value), "{value} in {cygnus}");
    }
}

#[test]
fn fleet_settings_are_editable_only_on_the_private_listener() {
    let config = Config::parse("ignite.toml", FLEET_CONFIG).unwrap();
    let server = HubServer {
        config,
        listen: "127.0.0.1:0".to_owned(),
        database: ignite_core::test_files::temp_dir().join("fleet.sqlite"),
        wordmark: None,
    };
    let mut conn = hub::open(&server.database).unwrap();
    seed(&conn);
    let private = server.start().unwrap();
    let public = server.start_read_only().unwrap();
    let now = Timestamp::parse(NOW).unwrap();
    let mut settings = hub::fleet_settings::settings(&conn).unwrap();
    configure_providers(&mut settings);
    settings.machines.extend([
        (
            "cygnus".to_owned(),
            MachineSetting {
                state: MachineState::ChecksOnly,
                state_until: None,
                trees: TreeSettings {
                    trees_root: Some(r"D:\trees".to_owned()),
                    tree_space_gb: Some(TreeSpace::Gigabytes(90)),
                    tree_ceiling_gb: 40,
                },
            },
        ),
        (
            "multivac".to_owned(),
            MachineSetting {
                state: MachineState::Off,
                ..MachineSetting::default()
            },
        ),
    ]);
    let original_version = settings.version;
    let hub::fleet_settings::PutOutcome::Stored(settings) = hub::fleet_settings::put(
        &mut conn,
        original_version,
        settings,
        "seed@example.com",
        now,
    )
    .unwrap() else {
        panic!("seeding the fleet settings conflicted");
    };

    let (status, private_page) = browser_request(private.address(), "GET", "/fleet", "");
    assert_eq!(status, 200);
    assert!(private_page.contains(r#"data-api="/proj/api/fleet""#));
    assert!(private_page.contains(&format!("&quot;version&quot;:{}", settings.version)));
    assert_provider_controls(&private_page);
    assert_machine_controls(&private_page);
    assert_cygnus_tree_fields(&private_page, true);

    let (status, public_page) = browser_request(public.address(), "GET", "/fleet", "");
    assert_eq!(status, 200);
    assert_cygnus_tree_fields(&public_page, false);
    assert!(machine_settings(&public_page, "multivac").contains(">Repository default<"));
    assert!(!public_page.contains("data-fleet-field"));
    assert!(!public_page.contains("data-settings="));

    let mut desired = settings.clone();
    desired
        .machines
        .get_mut("cygnus")
        .unwrap()
        .trees
        .tree_ceiling_gb = 50;
    desired.providers.get_mut("codex").unwrap().share = 36;
    let body = serde_json::to_string(&FleetUpdate {
        version: settings.version,
        settings: desired,
    })
    .unwrap();
    let (status, reply) = browser_request(public.address(), "PUT", "/proj/api/fleet", &body);
    assert_eq!(status, 405, "{reply}");
    assert_eq!(fleet_view(private.address()).settings, settings);

    let (status, saved) = browser_request(private.address(), "PUT", "/proj/api/fleet", &body);
    assert_eq!(status, 200, "{saved}");
    let saved: FleetSettings = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved.version, original_version + 2);
    let mut expected_trees = settings.machine("cygnus").trees;
    expected_trees.tree_ceiling_gb = 50;
    assert_eq!(saved.machine("cygnus").trees, expected_trees);
    assert_eq!(
        fleet_view(private.address())
            .settings
            .provider("codex")
            .share,
        36
    );
    let (status, updated) = browser_request(private.address(), "GET", "/fleet", "");
    assert_eq!(status, 200);
    assert!(machine_settings(&updated, "cygnus").contains(r#"value="50" min="1""#));
    assert!(provider_row(&updated, "codex").contains(">75%<"));
    assert!(updated.contains(
        r#"c-timeline-actor">operator</span>machine cygnus: checks-only, tree ceiling 50 GB"#
    ));
    assert!(updated.contains(r#"datetime="2026-11-17T12:00:00.000Z""#));
    assert!(!updated.contains("@example.com"));

    private.stop();
    public.stop();
}

fn fleet_view(address: SocketAddr) -> FleetView {
    let (status, reply) = browser_request(address, "GET", "/proj/api/fleet", "");
    assert_eq!(status, 200);
    serde_json::from_str(&reply).unwrap()
}
