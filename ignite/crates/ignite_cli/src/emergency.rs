//! The subcommands of `ignite emergency`, which the daemon's `emergency` verb answers (see
//! `ignite_daemon::emergency`).

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum EmergencyCommand {
    /// Declare or renew an emergency for an incident or justiciar ticket you hold. Heavy verbs
    /// refuse across the fleet for two hours, with exceptions for the declaring session, its
    /// ticket and any allowances it grants.
    Declare { ticket: String },
    /// Let one ticket's session run a heavy verb, or every heavy verb, during the emergency.
    Allow {
        ticket: String,
        /// `start`, `test`, `check`, `land`, `run`, `rebuild` or `audit`; every one if left out.
        verb: Option<String>,
    },
    /// Lift the emergency on this machine alone when the declaring session is gone.
    Clear {
        /// Required: other machines go on enforcing it.
        #[arg(long, required = true)]
        local: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::EmergencyCommand;
    use crate::args::{Cli, Command};

    fn parse(line: &str) -> Command {
        Cli::try_parse_from(line.split_whitespace())
            .unwrap()
            .command
    }

    #[test]
    fn parses_each_emergency_command() {
        assert!(matches!(
            parse("ignite emergency"),
            Command::Emergency { command: None }
        ));
        assert!(matches!(
            parse("ignite emergency declare TJUS1"),
            Command::Emergency { command: Some(EmergencyCommand::Declare { ticket }) } if ticket == "TJUS1"
        ));
        assert!(matches!(
            parse("ignite emergency allow TWORK land"),
            Command::Emergency { command: Some(EmergencyCommand::Allow { verb: Some(v), .. }) } if v == "land"
        ));
        assert!(Cli::try_parse_from(["ignite", "emergency", "clear"]).is_err());
        assert!(
            matches!(parse("ignite wait emergency"), Command::Wait { handle } if handle == "emergency")
        );
    }
}
