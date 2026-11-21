use super::*;
use clap::CommandFactory;

#[test]
fn the_definition_is_consistent() {
    Cli::command().debug_assert();
}

#[test]
fn help_lists_every_agent_verb() {
    let help = Cli::command().render_help().to_string();
    for verb in [
        "start", "test", "check", "sync", "land", "run", "ticket", "why", "status", "doctor",
        "prime", "wait",
    ] {
        assert!(
            help.lines().any(|l| l.trim_start().starts_with(verb)),
            "{verb} in {help}"
        );
    }
    assert!(
        !help.contains("rustc-wrap"),
        "hidden subcommands stay hidden"
    );
}

#[test]
fn hub_serves_without_a_subcommand_and_restores_with_one() {
    let parse = |line: &str| {
        Cli::try_parse_from(line.split_whitespace())
            .unwrap()
            .command
    };
    assert!(matches!(
        parse("ignite hub"),
        Command::Hub { command: None }
    ));
    assert!(matches!(
        parse("ignite hub restore --from-mirror --at 2026-11-18T10:00:00Z --actor rogue"),
        Command::Hub {
            command: Some(HubCommand::Restore {
                at: Some(_),
                actor: Some(_),
                ..
            })
        }
    ));
    assert!(Cli::try_parse_from(["ignite", "hub", "restore"]).is_err());
}

#[test]
fn land_needs_no_message() {
    let parsed = Cli::try_parse_from(["ignite", "land"]).unwrap().command;
    assert!(matches!(parsed, Command::Land { note: None }));
    let parsed = Cli::try_parse_from(["ignite", "land", "-m", "n"])
        .unwrap()
        .command;
    assert!(matches!(parsed, Command::Land { note: Some(_) }));
}

#[test]
fn a_raw_ack_carries_the_tool_result_as_given() {
    let line = r#"ignite dispatch ack 4814 raw {"sessionId":"local_a","isRunning":true}"#;
    let Ok(Cli {
        command:
            Command::Dispatch {
                command: Some(DispatchCommand::Ack { result, reason, .. }),
                ..
            },
        ..
    }) = Cli::try_parse_from(line.split_whitespace())
    else {
        panic!("not ack");
    };
    assert_eq!(
        (result, reason.join(" ")),
        (
            AckResult::Raw,
            r#"{"sessionId":"local_a","isRunning":true}"#.to_owned()
        )
    );
}

#[test]
fn dispatch_waits_the_default_when_no_wait_is_given() {
    let command = Cli::try_parse_from(["ignite", "dispatch"]).unwrap().command;
    assert!(matches!(
        command,
        Command::Dispatch {
            wait: ignite_core::protocol::client::DISPATCH_WAIT_S,
            command: None,
        }
    ));
}

/// `ignite pool usage` reaches the daemon's `pool` verb, which prints the plan's usage.
#[test]
fn parses_pool_usage() {
    assert!(matches!(
        Cli::try_parse_from(["ignite", "pool", "usage"])
            .unwrap()
            .command,
        Command::Pool {
            command: PoolCommand::Usage
        }
    ));
}

#[test]
fn parses_pool_mode() {
    assert!(matches!(
        Cli::try_parse_from(["ignite", "pool", "mode", "claude", "config"])
            .unwrap()
            .command,
        Command::Pool {
            command: PoolCommand::Mode { app, setting }
        } if app == "claude" && setting == "config"
    ));
}

#[test]
fn parses_the_documented_commands() {
    let parse = |line: &str| {
        Cli::try_parse_from(line.split_whitespace())
            .unwrap()
            .command
    };
    assert!(
        matches!(parse("ignite stop L-2"), Command::Stop { handle, force: false } if handle == "L-2")
    );
    assert!(matches!(
        parse("ignite start Tq44s.9.48 --reservation 7f3a"),
        Command::Start { ticket, reservation: Some(code) } if ticket == "Tq44s.9.48" && code == "7f3a"
    ));
    assert!(matches!(
        parse("ignite release -m blocked --needs person"),
        Command::Release { note: Some(_), needs: Some(needs) } if needs == "person"
    ));
    assert!(
        matches!(parse("ignite close -m obsolete"), Command::Close { reason } if reason == "obsolete")
    );
    let Command::Run {
        class,
        ticket,
        command,
        ..
    } = parse("ignite run --class batch --ticket Tix1t.12 -- just tournament --games 2000")
    else {
        panic!("not run");
    };
    assert_eq!(class, ClassArg::Batch);
    assert_eq!(ticket.as_deref(), Some("Tix1t.12"));
    assert_eq!(command, ["just", "tournament", "--games", "2000"]);
    let Command::Ticket { command } =
        parse("ignite ticket create Split -p P2 --parent Tig9a --labels tier:opus --deps Tig9a.1")
    else {
        panic!("not ticket");
    };
    let TicketCommand::Create {
        fields,
        labels,
        deps,
        ..
    } = *command
    else {
        panic!("not ticket create");
    };
    assert_eq!(fields.priority, Some(2));
    assert_eq!(
        (labels, deps),
        (vec!["tier:opus".to_owned()], vec!["Tig9a.1".to_owned()])
    );
    assert!(matches!(
        parse("ignite dispatch ack 4812 ok"),
        Command::Dispatch {
            command: Some(DispatchCommand::Ack { id: 4812, .. }),
            ..
        }
    ));
    assert!(matches!(
        parse("ignite dispatch --wait 480"),
        Command::Dispatch { wait: 480, .. }
    ));
    let Command::Dispatch {
        command: Some(DispatchCommand::Ack { result, reason, .. }),
        ..
    } = parse("ignite dispatch ack 4813 failed clear refused")
    else {
        panic!("not ack");
    };
    assert_eq!(
        (result, reason.join(" ")),
        (AckResult::Failed, "clear refused".to_owned())
    );
    assert!(matches!(
        parse("ignite tell --all Skip"),
        Command::Tell { all: true, now: false, words } if words == ["Skip"]
    ));
    assert!(matches!(
        parse("ignite pool pause --all"),
        Command::Pool {
            command: PoolCommand::Pause { all: true }
        }
    ));
    assert!(matches!(
        parse("ignite status --admission"),
        Command::Status {
            admission: true,
            ..
        }
    ));
    assert!(matches!(
        parse("ignite rustc-wrap rustc --crate-name x"),
        Command::RustcWrap(Passthrough { args }) if args.len() == 3
    ));
    assert!(matches!(
        parse("ignite internal selftest"),
        Command::Internal {
            command: InternalCommand::Selftest
        }
    ));
    assert!(Cli::try_parse_from(["ignite", "ticket", "create", "x", "-p", "P7"]).is_err());
}

#[test]
fn parses_a_forced_stop() {
    let line = ["ignite", "stop", "T-3", "--force"];
    assert!(matches!(
        Cli::try_parse_from(line).unwrap().command,
        Command::Stop { handle, force: true } if handle == "T-3"
    ));
}

#[test]
fn parses_the_cli_turn_command_with_the_program_untouched() {
    let line = [
        "ignite", "internal", "cli-turn", "--home", "/h", "--", "claude", "-p", "--model", "m",
    ];
    assert!(matches!(
        Cli::try_parse_from(line).unwrap().command,
        Command::Internal {
            command: InternalCommand::CliTurn { home, command }
        } if home == std::path::Path::new("/h") && command == ["claude", "-p", "--model", "m"]
    ));
}

#[test]
fn parses_the_verb_process_command() {
    let turn = Cli::try_parse_from([
        "ignite",
        "internal",
        "justiciar-turn",
        "/h/j.lock",
        "/h/j-1.log",
        "claude",
        "-p",
        "--model",
        "opus",
    ])
    .unwrap();
    let Command::Internal {
        command: InternalCommand::JusticiarTurn { lock, log, command },
    } = turn.command
    else {
        panic!("not a justiciar turn");
    };
    assert_eq!(
        (lock.to_str(), log.to_str()),
        (Some("/h/j.lock"), Some("/h/j-1.log"))
    );
    assert_eq!(command, ["claude", "-p", "--model", "opus"]);
    let line = ["ignite", "internal", "verb", "L-6"];
    assert!(matches!(
        Cli::try_parse_from(line).unwrap().command,
        Command::Internal {
            command: InternalCommand::Verb { handle }
        } if handle == "L-6"
    ));
    assert!(matches!(
        Cli::try_parse_from(["ignite", "internal", "restart"])
            .unwrap()
            .command,
        Command::Internal {
            command: InternalCommand::Restart
        }
    ));
}

#[test]
fn tell_takes_now() {
    let line = "ignite tell --now T.1 Stop";
    assert!(matches!(
        Cli::try_parse_from(line.split_whitespace()).unwrap().command,
        Command::Tell { all: false, now: true, words } if words == ["T.1", "Stop"]
    ));
}
