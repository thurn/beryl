use super::*;

fn routing() -> Routing {
    let machine = |name: &str, os: Os, home: &str| RoutedMachine {
        name: name.to_owned(),
        os: Some(os),
        home_app: Some(home.to_owned()),
    };
    Routing {
        machines: vec![
            machine("cygnus", Os::Windows, "codex"),
            machine("jarvis", Os::Macos, "claude"),
            machine("multivac", Os::Macos, "codex"),
        ],
        apps: vec!["claude".to_owned(), "codex".to_owned()],
        default_owner: Some(Owner::new("jarvis", Some("claude"))),
    }
}

fn owner(text: &str) -> Owner {
    text.parse().unwrap()
}

#[test]
fn an_owner_reads_and_writes_as_machine_and_app() {
    assert_eq!(owner("Jarvis/Codex"), Owner::new("jarvis", Some("codex")));
    assert_eq!(owner("cygnus").to_string(), "cygnus");
    assert_eq!(owner("jarvis/claude").to_string(), "jarvis/claude");
    assert!("jarvis/".parse::<Owner>().is_err());
    assert!("a b".parse::<Owner>().is_err());
    let json = serde_json::to_string(&owner("multivac/codex")).unwrap();
    assert_eq!(json, "\"multivac/codex\"");
    assert_eq!(
        serde_json::from_str::<Owner>(&json).unwrap(),
        owner("multivac/codex")
    );
}

#[test]
fn an_owner_without_an_app_admits_its_machines_home_app() {
    let routing = routing();
    let cygnus = owner("cygnus");
    let home = routing.home_app("cygnus");
    assert!(cygnus.admits("CYGNUS", "codex", home));
    assert!(!cygnus.admits("cygnus", "claude", home));
    assert!(!cygnus.admits("jarvis", "codex", routing.home_app("jarvis")));
    // A machine with no home app admits any app.
    assert!(cygnus.admits("cygnus", "claude", None));
    assert_eq!(cygnus.resolved(&routing), owner("cygnus/codex"));
}

#[test]
fn the_choices_are_every_machine_with_every_app() {
    let names: Vec<String> = routing()
        .choices()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        names,
        [
            "cygnus/claude",
            "cygnus/codex",
            "jarvis/claude",
            "jarvis/codex",
            "multivac/claude",
            "multivac/codex"
        ]
    );
}

#[test]
fn a_ticket_goes_to_its_owner_and_otherwise_the_default() {
    let routing = routing();
    let default = routing.default_owner.clone();
    let never = |_: &str| false;
    assert_eq!(
        routing.destinations(None, default.as_ref(), false, None, never),
        [owner("jarvis/claude")]
    );
    let own = owner("multivac/codex");
    assert_eq!(
        routing.destinations(Some(&own), default.as_ref(), false, None, never),
        std::slice::from_ref(&own)
    );
    assert!(
        routing
            .destinations(None, None, false, None, never)
            .is_empty()
    );
}

#[test]
fn a_ticket_holding_master_falls_back_only_when_its_machine_is_silent() {
    let routing = routing();
    let default = routing.default_owner.clone();
    let cygnus = owner("cygnus");
    let silent = |machine: &str| machine == "cygnus";
    // Not holding master: it waits for its machine.
    assert_eq!(
        routing.destinations(Some(&cygnus), default.as_ref(), false, None, silent),
        std::slice::from_ref(&cygnus)
    );
    // Portable: the default owner may take it.
    assert_eq!(
        routing.destinations(Some(&cygnus), default.as_ref(), true, None, silent),
        [cygnus.clone(), owner("jarvis/claude")]
    );
    // Needs macOS: the default runs it.
    assert_eq!(
        routing.destinations(
            Some(&cygnus),
            default.as_ref(),
            true,
            Some(Os::Macos),
            silent
        ),
        [cygnus.clone(), owner("jarvis/claude")]
    );
    // Needs Windows, and only the silent machine runs it: nothing more.
    assert_eq!(
        routing.destinations(
            Some(&cygnus),
            default.as_ref(),
            true,
            Some(Os::Windows),
            silent
        ),
        std::slice::from_ref(&cygnus)
    );
    // A Mac regression owned by multivac while the default is on Windows goes to jarvis.
    let windows_default = owner("cygnus/codex");
    let multivac = owner("multivac");
    assert_eq!(
        routing.destinations(
            Some(&multivac),
            Some(&windows_default),
            true,
            Some(Os::Macos),
            |machine| machine == "multivac"
        ),
        [multivac, owner("jarvis")]
    );
}
