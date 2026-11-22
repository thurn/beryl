//! Progress for the Git operations that keep the main checkout current.

use ignite_core::refname;
use ignite_core::traits::Result;
use ignite_git::fastforward::{self, FastForward, Step};

use super::Trees;
use crate::steps;

impl Trees {
    /// Fast-forwards the main checkout's local integration branches to the remote, only while
    /// the checkout has no local changes.
    pub fn fast_forward_main(&self) -> Result<FastForward> {
        let config = &self.inner.config;
        let remote = &config.remote;
        fastforward::main_checkout_with_progress(&config.repo, remote, &config.branches, |step| {
            let text = match step {
                Step::CheckingCheckout => "checking the main checkout for local changes".to_owned(),
                Step::ReadingHead => "identifying the branch in the main checkout".to_owned(),
                Step::Fetching(branch) => {
                    let reference = refname::remote_ref(branch);
                    format!("fetching {reference} from {remote} for the main checkout")
                }
                Step::Comparing(reference) => {
                    format!("checking whether {reference} can fast-forward")
                }
                Step::Merging(reference) => {
                    format!("fast-forwarding {reference} in the main checkout")
                }
                Step::UpdatingRef(reference) => format!("updating local ref {reference}"),
            };
            steps::step(text);
        })
    }
}
