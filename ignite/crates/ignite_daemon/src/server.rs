//! Requests from clients. A [`Verb`] answers one verb; the [`Registry`] finds the verb a
//! request names. The listener that reads requests from the local socket is still a stub.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::config::Config;
use ignite_core::ids::MachineId;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::traits::{Builds, Clock, Git, Hub, Launcher};

/// What every verb may use. Tests build one from the fakes in `ignite_core::fakes`.
#[derive(Clone)]
pub struct Context {
    pub config: Config,
    /// The repository's main checkout.
    pub repo: PathBuf,
    pub machine: MachineId,
    pub clock: Arc<dyn Clock>,
    pub hub: Arc<dyn Hub>,
    pub git: Arc<dyn Git>,
    pub launcher: Arc<dyn Launcher>,
    pub builds: Arc<dyn Builds>,
}

/// One verb of the `ignite` command, run inside the daemon.
pub trait Verb: Send + Sync {
    /// The verb as the client sends it, such as `land`.
    fn name(&self) -> &str;
    fn run(&self, ctx: &Context, req: &Request) -> Reply;
}

/// The verbs the daemon answers, by name.
#[derive(Default)]
pub struct Registry {
    verbs: BTreeMap<String, Box<dyn Verb>>,
}

impl Registry {
    /// Adds a verb. Registering two verbs under one name is a programming error.
    pub fn register(&mut self, verb: Box<dyn Verb>) {
        let name = verb.name().to_owned();
        let previous = self.verbs.insert(name.clone(), verb);
        assert!(previous.is_none(), "the verb {name:?} is registered twice");
    }

    /// The registered verbs' names, sorted.
    pub fn names(&self) -> Vec<&str> {
        self.verbs.keys().map(String::as_str).collect()
    }

    /// Runs the verb the request names, or answers with a usage error if none is registered.
    pub fn handle(&self, ctx: &Context, req: &Request) -> Reply {
        match self.verbs.get(&req.verb) {
            Some(verb) => verb.run(ctx, req),
            None => Reply::text(Exit::Usage, format!("unknown verb: {}", req.verb)),
        }
    }
}

/// Accepts clients on the machine's local socket and answers each from `registry`.
///
/// Stub. The ticket Thrh4.23 (Daemon core: requests, handles and the hub client) owns it.
pub fn serve(ctx: &Context, registry: &Registry, socket: &str) -> std::io::Result<()> {
    let _ = (ctx, registry, socket);
    todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
    use ignite_core::time::Timestamp;

    struct Echo;

    impl Verb for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn run(&self, _ctx: &Context, req: &Request) -> Reply {
            Reply::text(Exit::Success, req.args.join(" "))
        }
    }

    fn context() -> Context {
        let text = r#"
            [repository]
            remote = "origin"
            staging = "staging"
            master = "master"
            trees = "trees"
            checks = "checks"
            [hub]
            url = "http://localhost:1"
            project = "test"
            [gate]
            threshold = 0.01
            cap = 10
            [machine.local]
            ticket_trees = 1
            landing_trees = 1
        "#;
        Context {
            config: Config::parse("ignite.toml", text).unwrap(),
            repo: PathBuf::from("/repo"),
            machine: MachineId::new("m-1").unwrap(),
            clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
            hub: Arc::new(FakeHub::default()),
            git: Arc::new(FakeGit::default()),
            launcher: Arc::new(FakeLauncher::default()),
            builds: Arc::new(FakeBuilds::default()),
        }
    }

    fn request(verb: &str, args: &[&str]) -> Request {
        Request {
            verb: verb.to_owned(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            cwd: PathBuf::from("/repo"),
            session: None,
            wait_s: 1,
        }
    }

    #[test]
    fn dispatches_to_the_named_verb() {
        let mut registry = Registry::default();
        registry.register(Box::new(Echo));
        let reply = registry.handle(&context(), &request("echo", &["a", "b"]));
        assert_eq!((reply.exit, reply.text.as_str()), (Exit::Success, "a b"));
        assert_eq!(registry.names(), ["echo"]);
    }

    #[test]
    fn an_unknown_verb_is_a_usage_error() {
        let reply = Registry::default().handle(&context(), &request("nope", &[]));
        assert_eq!(reply.exit, Exit::Usage);
        assert_eq!(reply.text, "unknown verb: nope");
    }

    #[test]
    #[should_panic(expected = "registered twice")]
    fn a_verb_cannot_be_registered_twice() {
        let mut registry = Registry::default();
        registry.register(Box::new(Echo));
        registry.register(Box::new(Echo));
    }
}
