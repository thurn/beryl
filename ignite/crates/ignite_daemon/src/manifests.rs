//! The check manifests as the long-running daemon holds them, read again whenever it
//! fast-forwards the main checkout.
//!
//! A change to the manifests builds no release and restarts no daemon (see
//! [`crate::update::scope::ignite_change`]), so the daemon cannot rely on a restart to see new
//! ones. Most readers never needed one: each verb process, including a landing's gate and a tip
//! round, reads the manifests from the main checkout when it starts. What the daemon process
//! itself reads from them is the build products, which its build manager runs for every verb
//! process and which decide what a tree reset keeps and what is prebuilt, and the exploration
//! families its idle loop chooses from. Those components hold a [`Live`] value, and the tip
//! loop calls [`Reload::run`] after each fast-forward of the main checkout, so the daemon reads
//! the manifests `master` has, as a restart into the new release did.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use ignite_core::config::Config;
use ignite_core::manifest::Manifest;

use crate::trees::Trees;

/// A value shared by every clone, which [`Live::set`] replaces for all of them. A reader takes
/// the current value with [`Live::get`] and keeps it for the rest of its work, so it never sees
/// half of an old manifest and half of a new one.
pub struct Live<T> {
    value: Arc<RwLock<Arc<T>>>,
}

/// The check manifests, replaced when those in the main checkout change.
pub type LiveManifest = Live<Manifest>;

impl<T> Live<T> {
    pub fn new(value: T) -> Live<T> {
        Live {
            value: Arc::new(RwLock::new(Arc::new(value))),
        }
    }

    /// The current value.
    pub fn get(&self) -> Arc<T> {
        self.value
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Replaces the value for every clone.
    pub fn set(&self, value: T) {
        *self.value.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(value);
    }
}

impl<T> Clone for Live<T> {
    fn clone(&self) -> Live<T> {
        Live {
            value: self.value.clone(),
        }
    }
}

impl<T> From<T> for Live<T> {
    fn from(value: T) -> Live<T> {
        Live::new(value)
    }
}

impl<T: fmt::Debug> fmt::Debug for Live<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

/// Two values are equal when their current values are.
impl<T: PartialEq> PartialEq for Live<T> {
    fn eq(&self, other: &Live<T>) -> bool {
        *self.get() == *other.get()
    }
}

impl<T: Eq> Eq for Live<T> {}

/// Reads the main checkout's manifests again and hands any change to the daemon's components.
pub struct Reload {
    /// The main checkout.
    pub repo: PathBuf,
    pub config: Config,
    /// The manifests the build manager and the idle work read.
    pub manifest: LiveManifest,
    /// The trees, whose reset keeps and prebuild lists come from the products.
    pub trees: Trees,
}

impl Reload {
    /// Reads the manifests from the main checkout. Returns whether they differed from those
    /// held, which are then replaced. Manifests that cannot be read or parsed, such as a
    /// checkout halfway through a merge, leave the held ones in place.
    pub fn run(&self) -> Result<bool, String> {
        let fresh = crate::main::read_manifest(&self.repo, &self.config)?;
        Ok(self.take(fresh))
    }

    /// Holds `fresh` if it differs from the manifests held; returns whether it did.
    pub fn take(&self, fresh: Manifest) -> bool {
        if *self.manifest.get() == fresh {
            return false;
        }
        self.trees.reload_products(&fresh);
        self.manifest.set(fresh);
        true
    }
}

#[cfg(test)]
#[path = "manifests_tests.rs"]
mod tests;
