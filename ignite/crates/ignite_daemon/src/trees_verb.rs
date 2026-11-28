//! `ignite trees evict <tree> [--force]`, for incident commanders, justiciars and the operator:
//! frees a build slot by deleting one tree's build output (see `Trees::evict`), and says so on
//! the ticket that leases the tree.

use ignite_core::ids::{TicketId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::tickets::{BatchRequest, Operation};

use crate::server::{Context, Verb};
use crate::trees::Trees;

const USAGE: &str = "usage    ignite trees evict <tree> [--force]";

/// The daemon's `trees` verb. Provisioning a volume (`ignite trees volume`) is answered by the
/// client itself.
pub struct TreesVerb(pub Trees);

impl Verb for TreesVerb {
    fn name(&self) -> &'static str {
        "trees"
    }

    /// It acts on the tree it names, which the caller does not hold.
    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let words: Vec<&str> = req.args.iter().map(String::as_str).collect();
        let force = words.contains(&"--force");
        let named: Vec<&str> = words.iter().copied().filter(|w| *w != "--force").collect();
        let ["evict", tree] = named.as_slice() else {
            return Reply::text(Exit::Usage, USAGE);
        };
        let Ok(tree) = TreeId::new(*tree) else {
            return Reply::text(Exit::Usage, format!("failed   not a tree: {tree}"));
        };
        if let Err(reason) = crate::stop::emergency_or_operator(ctx) {
            return Reply::text(
                Exit::Conflict,
                format!(
                    "refused  `ignite trees evict` is for incident commanders, justiciars and \
                     the operator: {reason}"
                ),
            );
        }
        let evicted = match self.0.evict(&tree, force) {
            Ok(evicted) => evicted,
            Err(error) => {
                return Reply::text(
                    crate::dispatchd::hub_exit(&error),
                    format!("failed   {error}"),
                );
            }
        };
        let mut lines = Vec::new();
        if evicted.stopped > 0 {
            lines.push(format!(
                "stopped  {} command(s) running in {tree}",
                evicted.stopped
            ));
        }
        lines.push(format!(
            "evicted  {tree}'s build output; its build slot is free"
        ));
        let by = ctx
            .session
            .clone()
            .or_else(|| req.session.clone())
            .map_or_else(|| "The operator".to_owned(), |session| session.to_string());
        if let Some(ticket) = evicted
            .ticket
            .as_deref()
            .and_then(|t| TicketId::new(t).ok())
        {
            let stopped = if evicted.stopped > 0 {
                format!(
                    " It first stopped {} command(s) running there.",
                    evicted.stopped
                )
            } else {
                String::new()
            };
            let text = format!(
                "{by} deleted the build output of tree {tree}, which this ticket leases, to free \
                 a build slot (`ignite trees evict`).{stopped} The lease is kept; the next build \
                 in the tree starts from nothing."
            );
            let request = BatchRequest {
                preconditions: Vec::new(),
                ops: vec![Operation::Comment {
                    id: ticket.to_string(),
                    text,
                }],
            };
            match ctx.hub.batch(&request) {
                Ok(_) => lines.push(format!("told     {ticket}, which leases the tree")),
                Err(error) => lines.push(format!("warning  {ticket} was not told: {error}")),
            }
        }
        Reply::text(Exit::Success, lines.join("\n"))
    }
}
