use super::super::tests::{commit, tempdir};
use super::*;

#[test]
fn the_release_build_is_an_agent_command_with_a_hang_limit() {
    let workspace = Path::new("/tmp/update-tree/ignite");
    let target = workspace.join(CACHED_TARGET);
    let spec = ReleaseBuilder::command(workspace, &target, &commit("bbbbbbb"));
    assert_eq!(spec.class, Class::Agent);
    assert_eq!(spec.hang_limit_s, Some(RELEASE_BUILD_HANG_LIMIT_S));
    assert_eq!(spec.cwd, Path::new("/tmp/update-tree/ignite"));
    assert!(spec.args.iter().any(|a| a == "--release"));
    assert!(spec.args.iter().any(|a| a == "ignite_cli"));
}

#[test]
fn a_failed_release_build_reports_the_end_of_its_output() {
    let home = tempdir("outcome");
    let output = home.join("output.log");
    std::fs::write(&output, "Compiling x\nerror: boom\n").unwrap();
    let result = |exit, hung| RunResult {
        exit,
        output: output.clone(),
        usage: ignite_core::protocol::Usage::default(),
        hung,
    };
    let executable = home.join("ignite");
    assert_eq!(
        ReleaseBuilder::outcome(&result(Some(0), false), executable.clone()).unwrap(),
        executable
    );
    let failed = ReleaseBuilder::outcome(&result(Some(101), false), executable.clone())
        .unwrap_err()
        .to_string();
    assert!(
        failed.contains("exit 101") && failed.contains("error: boom"),
        "{failed}"
    );
    let hung = ReleaseBuilder::outcome(&result(None, true), executable)
        .unwrap_err()
        .to_string();
    assert!(hung.contains("was ended"), "{hung}");
}

#[test]
fn other_release_build_failures_do_not_retry() {
    use ignite_core::fakes::FakeLauncher;
    use ignite_core::protocol::Usage;

    let work = tempdir("no-retry");
    let output = work.join("build.log");
    let workspace = work.join("ignite");
    let commit = commit("bbbbbbb");
    let linker_error = "lld-link: error: undefined symbol: example::release";
    for (windows, exit, hung, diagnostic) in [
        (true, Some(101), false, "error[E0425]: cannot find value"),
        (true, None, true, linker_error),
        (true, None, false, linker_error),
        (false, Some(101), false, linker_error),
    ] {
        fs::write(&output, diagnostic).unwrap();
        let cached = Arc::new(FakeLauncher::default());
        let retry = Arc::new(FakeLauncher::default());
        cached.script(
            &[],
            RunResult {
                exit,
                output: output.clone(),
                usage: Usage::default(),
                hung,
            },
        );
        let builder = ReleaseBuilder {
            tree: TreeBuilder {
                repo: work.clone(),
                work: work.clone(),
            },
            launcher: cached.clone(),
            retry_launcher: windows.then(|| retry.clone() as Arc<dyn Launcher>),
        };

        assert!(builder.build_in_tree(&workspace, &commit).is_err());
        assert_eq!(cached.runs().len(), 1);
        assert!(retry.runs().is_empty());
        assert!(!workspace.join(RETRY_TARGET).exists());
    }
}
