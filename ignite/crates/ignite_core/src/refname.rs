//! What a ref named in `ignite.toml` is on the remote and in a local checkout.
//!
//! `[repository]` names the refs Ignite moves: `staging`, `master`, `green`, `release` and the
//! prefix of released tickets' saved work. A bare name such as `staging` is the branch
//! `refs/heads/staging`. A full ref name such as `refs/ignite/staging` is that ref itself, which
//! lets Ignite keep its refs on the shared remote without listing them as branches: GitHub
//! stores, fetches and pushes a custom ref like a branch but shows only `refs/heads/*` in its
//! branch list.
//!
//! A fetch copies the remote ref to a remote-tracking ref under `refs/remotes/<remote>/`: the
//! branch `staging` to `refs/remotes/origin/staging`, as git's default refspec does, and
//! `refs/ignite/staging` to `refs/remotes/origin/ignite/staging`, so that `origin/ignite/staging`
//! names it on the command line. `git fetch --prune` with the default refspec deletes a tracking
//! ref of the second kind, because the remote has no branch of that name; Ignite fetches the ref
//! again before every use that needs it to be current.

/// The ref `name` is on the remote: `name` itself when it is a full ref (it starts with
/// `refs/`), else the branch `refs/heads/<name>`.
pub fn remote_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

/// Whether `name` refers to a branch under `refs/heads/`: either a bare branch name such as
/// `staging` or an explicit `refs/heads/<name>`. A custom ref such as `refs/ignite/green`
/// returns `false`.
pub fn is_branch(name: &str) -> bool {
    remote_ref(name).starts_with("refs/heads/")
}

/// The remote-tracking ref a fetch of `name` from `remote` updates: `refs/remotes/<remote>/`
/// followed by the branch name, or by the full ref without its leading `refs/`.
pub fn tracking_ref(remote: &str, name: &str) -> String {
    format!("refs/remotes/{}", tracking_name(remote, name))
}

/// [`tracking_ref`] as a person types it: `origin/staging`, or `origin/ignite/staging` for
/// `refs/ignite/staging`.
pub fn tracking_name(remote: &str, name: &str) -> String {
    let full = remote_ref(name);
    let rest = full
        .strip_prefix("refs/heads/")
        .or_else(|| full.strip_prefix("refs/"))
        .unwrap_or(&full);
    format!("{remote}/{rest}")
}

/// The local branch of the main checkout that is kept at `name`: the branch itself, or for a
/// full ref outside `refs/heads/` the rest after its namespace, so `refs/ignite/staging` keeps
/// the local branch `staging`. People and new sessions read the main checkout on that branch.
pub fn local_branch(name: &str) -> String {
    let full = remote_ref(name);
    if let Some(branch) = full.strip_prefix("refs/heads/") {
        return branch.to_owned();
    }
    let rest = full.strip_prefix("refs/").unwrap_or(&full);
    rest.split_once('/')
        .map_or(rest, |(_, after)| after)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_is_a_branch_and_keeps_its_tracking_ref() {
        assert!(is_branch("staging"));
        assert!(is_branch("refs/heads/master"));
        assert_eq!(remote_ref("staging"), "refs/heads/staging");
        assert_eq!(
            tracking_ref("origin", "staging"),
            "refs/remotes/origin/staging"
        );
        assert_eq!(tracking_name("origin", "ignite/T1"), "origin/ignite/T1");
        assert_eq!(local_branch("staging"), "staging");
        assert_eq!(remote_ref("refs/heads/master"), "refs/heads/master");
        assert_eq!(
            tracking_ref("origin", "refs/heads/master"),
            "refs/remotes/origin/master"
        );
        assert_eq!(local_branch("refs/heads/master"), "master");
    }

    #[test]
    fn a_custom_ref_is_itself_and_tracks_under_the_remote() {
        assert!(!is_branch("refs/ignite/staging"));
        assert!(!is_branch("refs/ignite/green"));
        assert_eq!(remote_ref("refs/ignite/staging"), "refs/ignite/staging");
        assert_eq!(
            tracking_ref("origin", "refs/ignite/staging"),
            "refs/remotes/origin/ignite/staging"
        );
        assert_eq!(
            tracking_name("origin", "refs/ignite/tickets/TNFZ6"),
            "origin/ignite/tickets/TNFZ6"
        );
        assert_eq!(local_branch("refs/ignite/staging"), "staging");
        assert_eq!(local_branch("refs/ignite/tickets/T1"), "tickets/T1");
    }
}
