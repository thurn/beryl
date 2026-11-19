use super::*;

fn routing() -> Routing {
    let machine = |name: &str, os: Os| RoutedMachine {
        name: name.to_owned(),
        os: Some(os),
    };
    Routing {
        machines: vec![
            machine("cygnus", Os::Windows),
            machine("jarvis", Os::Macos),
            machine("multivac", Os::Macos),
        ],
        apps: vec!["claude".to_owned(), "codex".to_owned()],
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
fn an_owner_admits_its_app_on_its_machine_or_any_app_when_it_names_none() {
    let cygnus = owner("cygnus");
    assert!(cygnus.admits("CYGNUS", "codex"));
    assert!(cygnus.admits("cygnus", "claude"));
    assert!(!cygnus.admits("jarvis", "codex"));
    let multivac_claude = owner("multivac/claude");
    assert!(multivac_claude.admits("multivac", "Claude"));
    assert!(!multivac_claude.admits("multivac", "codex"));
    assert!(!multivac_claude.admits("jarvis", "claude"));
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
fn a_ticket_goes_to_its_owner_and_otherwise_to_any_worker() {
    let routing = routing();
    let never = |_: &str| false;
    let own = owner("multivac/codex");
    assert_eq!(
        routing.destinations(Some(&own), false, None, never),
        std::slice::from_ref(&own)
    );
    assert!(routing.destinations(None, false, None, never).is_empty());
}

#[test]
fn a_ticket_holding_master_falls_back_only_when_its_machine_is_silent() {
    let routing = routing();
    let cygnus = owner("cygnus");
    let silent = |machine: &str| machine == "cygnus";
    // Not holding master: it waits for its machine.
    assert_eq!(
        routing.destinations(Some(&cygnus), false, None, silent),
        std::slice::from_ref(&cygnus)
    );
    // Portable: any machine that is heard from may take it.
    assert_eq!(
        routing.destinations(Some(&cygnus), true, None, silent),
        [cygnus.clone(), owner("jarvis"), owner("multivac")]
    );
    // Needs macOS: the Macs may take it.
    assert_eq!(
        routing.destinations(Some(&cygnus), true, Some(Os::Macos), silent),
        [cygnus.clone(), owner("jarvis"), owner("multivac")]
    );
    // Needs Windows, and only the silent machine runs it: nothing more.
    assert_eq!(
        routing.destinations(Some(&cygnus), true, Some(Os::Windows), silent),
        std::slice::from_ref(&cygnus)
    );
    // A Mac regression owned by a silent multivac goes to the other Mac.
    let multivac = owner("multivac/codex");
    assert_eq!(
        routing.destinations(Some(&multivac), true, Some(Os::Macos), |machine| {
            machine == "multivac"
        }),
        [multivac, owner("jarvis")]
    );
}
