//! [`FakeBuilds`]: builds that succeed and remember what was asked for.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::ids::TreeId;
use crate::protocol::Usage;
use crate::traits::{BuildFor, BuildResult, Builds};

use super::lock;

/// Builds that succeed the first time a (tree, product) is asked for and are current after.
#[derive(Debug, Default)]
pub struct FakeBuilds {
    scripted: Mutex<HashMap<(TreeId, String), BuildResult>>,
    built: Mutex<Vec<(TreeId, String, Vec<String>)>>,
    whom: Mutex<Vec<(String, BuildFor)>>,
    never_built: Mutex<Vec<(TreeId, String)>>,
    forgotten: Mutex<Vec<TreeId>>,
}

impl FakeBuilds {
    /// The trees whose builds the caller said were deleted, in order.
    pub fn forgotten(&self) -> Vec<TreeId> {
        lock(&self.forgotten).clone()
    }

    /// Makes `tree` report that it has never built `product`, until it is asked to build it.
    /// Every other product counts as built before.
    pub fn never_built(&self, tree: &TreeId, product: &str) {
        lock(&self.never_built).push((tree.clone(), product.to_owned()));
    }

    /// Answers every later request for `product` in `tree` with `result`.
    pub fn script(&self, tree: &TreeId, product: &str, result: BuildResult) {
        lock(&self.scripted).insert((tree.clone(), product.to_owned()), result);
    }

    /// Every request so far: tree, product and packages.
    pub fn requests(&self) -> Vec<(TreeId, String, Vec<String>)> {
        lock(&self.built).clone()
    }

    /// Every request made through `ensure_for`: the product and whom it was built for.
    pub fn requests_for(&self) -> Vec<(String, BuildFor)> {
        lock(&self.whom).clone()
    }
}

impl Builds for FakeBuilds {
    fn forget(&self, tree: &TreeId) {
        lock(&self.forgotten).push(tree.clone());
    }

    fn has_built(&self, tree: &TreeId, product: &str) -> bool {
        let never = lock(&self.never_built)
            .iter()
            .any(|(t, p)| t == tree && p == product);
        !never
            || lock(&self.built)
                .iter()
                .any(|(t, p, _)| t == tree && p == product)
    }

    fn ensure_for(
        &self,
        tree: &TreeId,
        product: &str,
        pkgs: &[String],
        who: &BuildFor,
    ) -> BuildResult {
        lock(&self.whom).push((product.to_owned(), who.clone()));
        self.ensure(tree, product, pkgs)
    }

    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult {
        let mut built = lock(&self.built);
        let key = (tree.clone(), product.to_owned());
        if let Some(result) = lock(&self.scripted).get(&key) {
            built.push((key.0, key.1, packages.to_vec()));
            return result.clone();
        }
        let seen = built.iter().any(|(t, p, _)| *t == key.0 && *p == key.1);
        built.push((key.0, key.1, packages.to_vec()));
        if seen {
            BuildResult::Current
        } else {
            BuildResult::Built {
                usage: Usage::default(),
                units: Vec::new(),
            }
        }
    }
}
