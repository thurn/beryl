//! `ignite import`: reads the old systems' data into the hub's database, on the hub's machine,
//! without the daemon. The run log's changes are resolved against the checkout the command runs
//! in, whose `ignite.toml` names the staging branch.

use std::path::Path;

use ignite_core::config::Config;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_git::GitCli;
use ignite_store::import::{self, ImportReport, RunLogSource, Sources};

use crate::args::Command;
use crate::manifest::{CONFIG_FILE, crate_graph};

pub fn run(command: &Command, dir: &Path) -> Reply {
    let Command::Import {
        hub,
        tickets,
        run_log,
        compile_timings,
        exploration,
        dry_run,
    } = command
    else {
        unreachable!("only `import` is sent here");
    };
    if tickets.is_none() && run_log.is_none() && compile_timings.is_none() {
        return Reply::text(
            Exit::Usage,
            "nothing to import: give --tickets, --run-log or --compile-timings",
        );
    }
    let result = (|| -> Result<Vec<ImportReport>, String> {
        let git;
        let run_log = match run_log {
            Some(folder) => {
                let root = ignite_git::tracked::toplevel(dir).map_err(|e| e.to_string())?;
                let config_path = root.join(CONFIG_FILE);
                let text = std::fs::read_to_string(&config_path)
                    .map_err(|e| format!("{}: {e}", config_path.display()))?;
                let config = Config::parse(CONFIG_FILE, &text).map_err(|e| e.to_string())?;
                let tracked = ignite_git::tracked::files(&root).map_err(|e| e.to_string())?;
                let graph = crate_graph(&root, &tracked)?;
                git = GitCli {
                    repo: root,
                    remote: config.repository.remote.clone(),
                };
                Some(RunLogSource {
                    folder: folder.clone(),
                    git: &git,
                    graph,
                    staging: config.repository.staging.clone(),
                    exploration: exploration.clone(),
                })
            }
            None => None,
        };
        let sources = Sources {
            tickets: tickets.clone(),
            run_log,
            compile_timings: compile_timings.clone(),
        };
        let mut conn = ignite_store::hub::open(hub).map_err(|e| e.to_string())?;
        import::run(&mut conn, &sources, *dry_run).map_err(|e| e.to_string())
    })();
    match result {
        Ok(reports) => reply(&reports, *dry_run),
        Err(message) => Reply::text(Exit::Usage, message),
    }
}

fn reply(reports: &[ImportReport], dry_run: bool) -> Reply {
    let mut text: String = reports.iter().map(ToString::to_string).collect();
    if dry_run {
        text.push_str("dry run: nothing was written\n");
    }
    let json = serde_json::json!({
        "dry_run": dry_run,
        "reports": reports.iter().map(|r| serde_json::json!({
            "source": r.source,
            "kinds": r.kinds.iter().map(|k| serde_json::json!({
                "kind": k.kind, "read": k.read, "written": k.written, "skipped": k.skipped,
            })).collect::<Vec<_>>(),
            "problems": r.problems,
            "more_problems": r.more_problems,
        })).collect::<Vec<_>>(),
    });
    let mut reply = Reply::text(Exit::Success, text);
    reply.json = Some(json);
    reply
}
