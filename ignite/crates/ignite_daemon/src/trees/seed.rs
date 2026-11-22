//! Seeding a newly leased tree's Cargo build directories from another tree's.
//!
//! Each tree builds in its own directories, so when an epic started eight tickets at once,
//! eight trees each built the same products from nothing: on 2026-11-18 one product (about
//! 950 CPU-s) was built in seven trees at once and another (about 1,400 CPU-s) in six. A tree
//! is leased at the tip, and another tree usually holds a build near it.
//!
//! So a lease for a compiling ticket first copies the build of the tree that leaves the least to
//! compile at the tip, when that is less than the leased tree's own ([`Trees::take_donor`]). The
//! copy runs after the lease has returned, on the
//! thread that then prebuilds the tree ([`Trees::start_builds`]): a copy of large target
//! directories under load took up to 141 s, and the session could do nothing while `ignite
//! start` waited for it. Every build in the tree waits for the copy ([`wait_until_seeded`]),
//! and a file the session edits during the copy keeps its own modification time. On macOS each
//! profile directory is cloned in one APFS `clonefile` call (`seed_copy.rs`): it takes seconds
//! and almost no disk until either tree writes. A seed that a daemon restart cut short is started
//! again by the next daemon (`seed_swap.rs`, [`Trees::resume_seeds`]). Measured
//! on 2026-11-18 with the two trees 14 commits apart (none of them touching a crate), three
//! products whose cold builds took 947, 1,070 and 1,390 CPU-s then built in 5.4, 6.3 and
//! 0.2 CPU-s.
//!
//! Cargo decides whether a unit is current from the modification times of the files it read,
//! and from a few absolute paths. Three things make the copy look to Cargo exactly as the
//! donor's build looks to it in the donor:
//!
//! - Every file git lists in the leased tree whose content is the donor's takes the donor
//!   file's modification time, and every other file (changed between the two commits, or
//!   uncommitted in either tree) is touched now, so the units that read it are rebuilt. A
//!   build script that declares no `rerun-if-changed` records its package's newest
//!   modification time itself, so without this step it would run again in every copy.
//! - Absolute paths into the donor in Cargo's dependency files (`.fingerprint/*/dep-*`, where
//!   an `include_str!` of a file outside the package is recorded with its full path) and in
//!   build scripts' recorded output (`build/*/output` and `root-output`) are rewritten to the
//!   leased tree, as is the donor's git directory, which a build script watching the commit
//!   names.
//! - Every Cargo target directory of the leased tree is replaced, not merged: a unit of its own
//!   left beside the copy could have been built from content older than the time a file now
//!   carries, and would be taken as current.
//!
//! A donor is a free tree, which is marked `resetting` for the copy so that nothing leases,
//! sweeps or prebuilds it meanwhile, or a leased ticket tree. Either way the copy holds Cargo's
//! own lock files in each profile directory, shared, so no build writes the donor while it is
//! copied but two leases may copy it at once; a donor a build holds is passed over. A tree
//! being seeded is never a donor, since its copied lock files are held by nobody. Directories a product writes that are not Cargo
//! target directories (`node_modules`, a Python environment) are left alone: they are current
//! by their input hash, not by modification times.
//!
//! On Windows the copy is a clone only on a volume that counts block references, such as a Dev
//! Drive (`ReFS`); elsewhere a copy would write every byte again, so nothing is seeded there.

use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::Result;
use ignite_git::{changed, refs, tracked};
use ignite_os::Native;
use ignite_os::clones::Clones as _;
use ignite_store::local::{rows, trees as store};

use super::disk::{id_of, profile_dirs};
use super::{Kind, LeaseRequest, Trees, store_error};
#[cfg(test)]
pub(super) use copy::clone_dir;
use copy::clone_target;
pub use swap::wait_for_seed_on_disk;
use swap::{clear, delete, move_dir, set_aside, staging, swap_in, tree_locks, tree_profiles};

/// The lock files Cargo holds in a profile directory while it builds there: Cargo 1.98 holds the
/// build and artifact locks exclusively for the whole build, and older versions `.cargo-lock`.
const CARGO_LOCKS: [&str; 3] = [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"];

/// The trees being seeded now.
static SEEDING: Mutex<Vec<Marker>> = Mutex::new(Vec::new());

/// Signalled whenever a tree's seed ends, for builds waiting on it.
static SEEDED: Condvar = Condvar::new();

/// How often a build waiting on a seed checks whether its work was asked to stop.
const STOP_RECHECK: Duration = Duration::from_secs(1);

/// A tree being seeded: its path, its ID, and the donor once one is chosen.
struct Marker {
    path: PathBuf,
    tree: TreeId,
    donor: Option<TreeId>,
}

fn seeding() -> MutexGuard<'static, Vec<Marker>> {
    SEEDING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks a tree as being seeded until it is dropped. While the mark is held, every build in the
/// tree waits ([`wait_until_seeded`]), the tree is never a donor, and a release of the tree waits
/// before it resets it.
pub struct Seeding(PathBuf);

impl Seeding {
    /// Marks the tree `tree` at `path`.
    pub fn start(path: PathBuf, tree: TreeId) -> Seeding {
        seeding().push(Marker {
            path: path.clone(),
            tree,
            donor: None,
        });
        Seeding(path)
    }

    /// Records the tree the copy comes from, for the message a waiting build shows.
    fn set_donor(&self, donor: &TreeId) {
        if let Some(marker) = seeding().iter_mut().find(|m| m.path == self.0) {
            marker.donor = Some(donor.clone());
        }
    }

    fn contains(path: &Path) -> bool {
        seeding().iter().any(|m| m.path == path)
    }

    /// Whether the tree at `path` is being seeded or is the donor of a seed now. The disk loop
    /// leaves such a tree alone even while no command runs in it: deleting its build
    /// directories would delete the copy, or what is being copied.
    ///
    /// A donor is a sibling of the tree it seeds, and is matched by its path. Matching the
    /// donor's ID alone took any tree of that name in another tree directory for a donor, which
    /// made the disk-loop tests fail now and then while another test seeded from its own tree of
    /// the same name.
    pub(in crate::trees) fn involves(path: &Path) -> bool {
        seeding().iter().any(|m| {
            m.path == path
                || m.donor
                    .as_ref()
                    .is_some_and(|donor| m.path.with_file_name(donor.as_str()) == path)
        })
    }
}

impl Drop for Seeding {
    fn drop(&mut self) {
        let mut seeding = seeding();
        if let Some(at) = seeding.iter().position(|m| m.path == self.0) {
            seeding.remove(at);
        }
        SEEDED.notify_all();
    }
}

/// Returns once the tree at `path` is not being seeded, which is at once for every tree but one
/// whose lease has just copied another tree's build. The seed deletes and replaces the tree's
/// Cargo target directories, so a build that ran meanwhile would be deleted or mixed with the
/// copy. While it waits, the calling work is shown as waiting for the copy. `Err` says why it
/// stopped waiting when the work was asked to stop first.
pub fn wait_until_seeded(path: &Path) -> std::result::Result<(), String> {
    let mut seeding = seeding();
    let mut waiting = None;
    while let Some(marker) = seeding.iter().find(|m| m.path == path) {
        let what = match &marker.donor {
            Some(donor) => format!(
                "{}'s build directories to be copied from {donor}",
                marker.tree
            ),
            None => format!("{}'s build directories to be copied", marker.tree),
        };
        if crate::steps::stop_requested() {
            return Err(format!(
                "stopped waiting for {what}: its work was asked to stop"
            ));
        }
        if waiting.as_ref().is_none_or(|(shown, _)| *shown != what) {
            waiting = Some((what.clone(), crate::steps::waiting(&what)));
        }
        seeding = SEEDED
            .wait_timeout(seeding, STOP_RECHECK)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    Ok(())
}

/// The tree a build was copied from.
#[derive(Debug, PartialEq, Eq)]
pub struct Seeded {
    pub donor: TreeId,
    /// The commit the donor's build is taken to match ([`Candidate::base`]).
    pub built: Commit,
    /// How many commits the leased tree's tip is ahead of `built`.
    pub behind: usize,
    /// How many files differ between `built` (with the donor's uncommitted files) and the tip.
    pub changed: usize,
    /// The products another tree holds that the donor does not, which the copy builds cold.
    pub missing: Vec<String>,
    /// The Cargo target directories copied, relative to the tree's root.
    pub dirs: Vec<String>,
}

/// What copying a tree's build would leave to compile, compared in field order: a product the
/// tree never built is a cold build of hundreds of crates, a file that differs from the tip
/// rebuilds the crates that read it and those that depend on them, and the commit distance only
/// breaks ties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    missing: usize,
    changed: usize,
    behind: usize,
}

/// A tree that could give or take a copy, measured against the tip.
struct Candidate<'a> {
    row: &'a rows::Tree,
    leased: bool,
    /// The commit its build is taken to match: a leased ticket tree's checked-out commit, since
    /// its ticket builds what it commits and edits, and otherwise the commit its row records
    /// (a released tree records the commit its ticket last had checked out).
    base: Commit,
    behind: usize,
    changed: usize,
    held: BTreeSet<String>,
}

/// A tree that could be copied, and whether it is free, so must be taken for the copy and put
/// back after it.
struct Donor {
    row: rows::Tree,
    free: bool,
    base: Commit,
    behind: usize,
    changed: usize,
    missing: Vec<String>,
}

impl Trees {
    /// Starts a compiling lease's work on a thread of its own, so that the lease returns as soon
    /// as the branch is checked out: first the seed (where copies are clones), then the prebuild products. The
    /// tree is marked as being seeded before this returns, so a build asked for at once, by the
    /// prebuild or by the session, waits for the copy instead of running under it.
    pub(super) fn start_builds(&self, tree: &TreeId, request: &LeaseRequest) {
        let Some(path) = self.path(tree) else {
            return;
        };
        let seeding = Native
            .copies_are_clones(&path)
            .then(|| Seeding::start(path, tree.clone()));
        if let Some(seeding) = &seeding
            && let Err(error) = fs::create_dir_all(staging(&seeding.0))
        {
            // Without it the seed still runs, but a restart during it is not resumed.
            eprintln!("ignite: {tree}'s seed is not marked on disk: {error}");
        }
        self.spawn_builds(
            seeding,
            tree,
            request.at.clone(),
            request.tip.clone(),
            request.files.clone(),
        );
    }

    /// Starts the seed that `seeding` marks, if any, and then the prebuild products, on a thread
    /// of its own.
    fn spawn_builds(
        &self,
        seeding: Option<Seeding>,
        tree: &TreeId,
        at: Commit,
        tip: Commit,
        files: Vec<String>,
    ) {
        if seeding.is_none() && self.inner.config.prebuild.get().is_empty() {
            return;
        }
        let trees = self.clone();
        let tree = tree.clone();
        std::thread::spawn(move || {
            if let Some(seeding) = seeding {
                let started = trees.inner.clock.now();
                let seeded = trees.seed(&seeding, &tree, &tip);
                // Set aside before the mark is dropped, so a build that starts once it is gone
                // finds no seed directory, and deleted after, so no build waits for that.
                let left = set_aside(&staging(&seeding.0));
                drop(seeding);
                match left {
                    Ok(Some(left)) => delete(left),
                    Ok(None) => {}
                    Err(error) => eprintln!("ignite: {tree}'s seed directory stays: {error}"),
                }
                let took = trees.inner.clock.now().seconds_since(started);
                match seeded {
                    Ok(Some(seeded)) => eprintln!(
                        "ignite: {tree} starts from a copy of {}'s {} (built at {}, {} commits \
                         and {} files behind {tip}, {}), made in {took:.1} s",
                        seeded.donor,
                        seeded.dirs.join(", "),
                        seeded.built,
                        seeded.behind,
                        seeded.changed,
                        if seeded.missing.is_empty() {
                            "holding every product another tree holds".to_owned()
                        } else {
                            format!("never built {}", seeded.missing.join(", "))
                        },
                    ),
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("ignite: {tree} was not seeded from another tree: {error}");
                    }
                }
            }
            if !trees.inner.config.prebuild.get().is_empty() {
                trees.build(&tree, &at, &files);
            }
        });
    }

    /// Copies the build that leaves the least to compile at `tip` into `tree`, if one leaves
    /// less than `tree`'s own ([`Trees::take_donor`]). Returns what was copied, or `None` when
    /// nothing was.
    /// `seeding` marks `tree` for as long as the copy runs.
    ///
    /// A donor in which a build holds Cargo's lock when its turn comes is passed over for the
    /// next. The seed used to end there instead, and under load, when most leased trees were
    /// building, a lease of a tree whose build the disk loop had deleted got no copy at all.
    fn seed(&self, seeding: &Seeding, tree: &TreeId, tip: &Commit) -> Result<Option<Seeded>> {
        for ranked in self.donors(tree, tip)? {
            let row = if ranked.free {
                match self.take_for_sweep(&ranked.row.id)? {
                    Some(row) => row,
                    None => continue,
                }
            } else {
                ranked.row.clone()
            };
            let donor = id_of(&row)?;
            seeding.set_donor(&donor);
            let result = self.copy_from(&row, tree);
            if ranked.free {
                self.put_back(&row)?;
            }
            let dirs = result?;
            if dirs.is_empty() {
                continue;
            }
            self.record_build(tree, &ranked.base)?;
            return Ok(Some(Seeded {
                donor,
                built: ranked.base,
                behind: ranked.behind,
                changed: ranked.changed,
                missing: ranked.missing,
                dirs,
            }));
        }
        Ok(None)
    }

    /// The trees to copy from, best first: of the free trees and the leased ticket trees holding
    /// a Cargo target directory, those whose copy leaves less to compile at `tip` ([`Cost`])
    /// than `tree`'s own build would, the one that leaves the least first.
    ///
    /// Ranking by commit distance alone chose donors whose copies rebuilt most of the workspace:
    /// a leased tree records the commit it was leased at, so its ticket's edits were invisible,
    /// and a tree that never built a product (one ticket's tree builds only what its checks
    /// need) passed it on to be built cold.
    fn donors(&self, tree: &TreeId, tip: &Commit) -> Result<Vec<Donor>> {
        let rows = store::all(&self.lock().db).map_err(|e| store_error(&e))?;
        let own_row = rows.iter().find(|row| row.id == tree.as_str());
        let own = own_row.and_then(|row| self.candidate(row, false, tip));
        let candidates: Vec<Candidate> = rows
            .iter()
            .filter(|row| row.id != tree.as_str())
            .filter_map(|row| {
                let root = Path::new(&own_row?.path).parent()?;
                if !self.inner.config.uses(root, row) {
                    return None;
                }
                let free = row.state == store::State::Free.as_str();
                let leased =
                    row.state == store::State::Leased.as_str() && row.kind == Kind::Ticket.as_str();
                if !free && !leased {
                    return None;
                }
                let path = Path::new(&row.path);
                if Seeding::contains(path) || self.cargo_dirs(path).is_empty() {
                    return None;
                }
                self.candidate(row, leased, tip)
            })
            .collect();
        let wanted: BTreeSet<&String> = candidates
            .iter()
            .chain(&own)
            .flat_map(|c| &c.held)
            .collect();
        let cost = |c: &Candidate| Cost {
            missing: wanted.iter().filter(|p| !c.held.contains(**p)).count(),
            changed: c.changed,
            behind: c.behind,
        };
        let own_cost = own.as_ref().map(cost);
        let mut ranked: Vec<(Cost, &Candidate)> = candidates
            .iter()
            .map(|c| (cost(c), c))
            .filter(|(cost, _)| own_cost.is_none_or(|own| *cost < own))
            .collect();
        // Cheapest first, and a free tree before a leased one at the same cost.
        ranked.sort_by(|a, b| (a.0, a.1.leased, &a.1.row.id).cmp(&(b.0, b.1.leased, &b.1.row.id)));
        Ok(ranked
            .into_iter()
            .map(|(_, candidate)| Donor {
                row: candidate.row.clone(),
                free: !candidate.leased,
                base: candidate.base.clone(),
                behind: candidate.behind,
                changed: candidate.changed,
                missing: wanted
                    .iter()
                    .filter(|p| !candidate.held.contains(**p))
                    .map(|p| (*p).clone())
                    .collect(),
            })
            .collect())
    }

    /// `row`'s tree measured against `tip`, or `None` when it holds no build or git cannot
    /// compare its commit with the tip.
    fn candidate<'a>(
        &self,
        row: &'a rows::Tree,
        leased: bool,
        tip: &Commit,
    ) -> Option<Candidate<'a>> {
        let id = id_of(row).ok()?;
        let path = Path::new(&row.path);
        let recorded = Commit::new(row.built_commit.as_deref()?).ok()?;
        let base = if leased {
            refs::head(path).ok()?
        } else {
            recorded
        };
        let mut changed: HashSet<String> = changed::between(&self.inner.config.repo, &base, tip)
            .ok()?
            .into_iter()
            .collect();
        if leased {
            changed.extend(changed::uncommitted(path).ok()?);
        }
        Some(Candidate {
            row,
            leased,
            behind: self.distance(Some(base.as_str()), tip)?,
            changed: changed.len(),
            held: self.inner.builds.held_in(&id, &self.cargo_dirs(path)),
            base,
        })
    }

    /// The directories the products write under `root` that are Cargo target directories,
    /// outermost only (a target directory inside another is copied with it).
    fn cargo_dirs(&self, root: &Path) -> Vec<String> {
        let keep = self.inner.config.keep.get();
        keep.iter()
            .filter(|dir| {
                !keep
                    .iter()
                    .any(|outer| *outer != **dir && dir.starts_with(&format!("{outer}/")))
            })
            .filter(|dir| !profile_dirs(&root.join(dir)).is_empty())
            .cloned()
            .collect()
    }

    /// Replaces `tree`'s Cargo target directories with copies of the donor's and makes the
    /// tree's files look to Cargo as the donor's do. Returns the directories copied.
    ///
    /// The session works in the tree while the copy runs, and its own commands (a `cargo build`
    /// or a script that runs one) do not wait for the seed as the daemon's builds do. When the
    /// seed deleted the tree's target directories in place, such a build failed with "No such
    /// file or directory" on its own outputs, and a deletion that raced the build's writes
    /// failed with "Directory not empty" and left the tree with no target directory at all. So
    /// the copy is made beside the tree first ([`staging`]), and the tree's directories are only
    /// swapped for it by renames while the seed holds Cargo's lock files in every profile
    /// directory of the tree exclusively ([`tree_locks`]). A build that holds one of them when
    /// the copy is ready ends the seed with an error and the tree keeps its own build; a build
    /// that starts during the swap waits on the lock and then builds in the copy.
    ///
    /// A failure before the swap leaves the tree as it was; one during it deletes the tree's
    /// target directories, so it builds from nothing rather than from a copy with wrong times.
    /// The copy and the replaced directories stay in the seed directory, which the caller sets
    /// aside and deletes when the seed ends.
    fn copy_from(&self, donor: &rows::Tree, tree: &TreeId) -> Result<Vec<String>> {
        let donor_id = id_of(donor)?;
        let from = Path::new(&donor.path);
        let to = PathBuf::from(self.row(tree)?.path);
        let dirs = self.cargo_dirs(from);
        let Some(_locks) = cargo_locks(from, &dirs)? else {
            return Ok(Vec::new());
        };
        // Everything git says is read before the tree is touched.
        let pairs = [
            path_pair(from, &to),
            path_pair(&tracked::git_dir(from)?, &tracked::git_dir(&to)?),
        ];
        let times = file_times(from, &to)?;
        let staging = staging(&to);
        let (new, old) = (staging.join("new"), staging.join("old"));
        clear(&staging)?;
        for dir in &dirs {
            clone_target(&from.join(dir), &new.join(dir))?;
            rewrite_target(&new.join(dir), &pairs)?;
        }
        let own = self.cargo_dirs(&to);
        let profiles = tree_profiles(from, &to, &own, &dirs);
        let tree_locked = match tree_locks(&profiles) {
            Ok(Some(locks)) => locks,
            locked => {
                return Err(locked.err().unwrap_or_else(|| {
                    io::Error::other(format!(
                        "a build in {tree} held Cargo's lock when the copy was ready, so the \
                         tree keeps its own build"
                    ))
                }))?;
            }
        };
        // Forgotten before the tree's own target directories are moved, and whether or not the
        // swap succeeds: a swap that fails partway leaves none, and a daemon restart during it
        // would otherwise leave the record of builds whose files are gone.
        self.inner.builds.forget(tree);
        let swapped = (|| -> io::Result<()> {
            for dir in own.iter().chain(&dirs) {
                move_dir(&to.join(dir), &old.join(dir))?;
            }
            for dir in &dirs {
                swap_in(&new.join(dir), &to.join(dir), &old.join(dir))?;
            }
            // While Cargo's locks in the donor are still held, so no build there changes what
            // it holds meanwhile.
            self.inner.builds.copy_held(&donor_id, tree, &dirs);
            for (file, time, read) in &times {
                // The session works in the tree during the copy. A file it wrote since its time
                // was read keeps its newer time, so the units that read it are rebuilt.
                let path = to.join(file);
                if fs::metadata(&path).and_then(|m| m.modified()).ok() == Some(*read) {
                    set_modified(&path, *time)?;
                }
            }
            Ok(())
        })();
        drop(tree_locked);
        if let Err(error) = swapped {
            for dir in own.iter().chain(&dirs) {
                let _ = remove(&to.join(dir));
            }
            return Err(error.into());
        }
        Ok(dirs)
    }
}

/// The two roots as path prefixes, each ending in `/`.
fn path_pair(from: &Path, to: &Path) -> (Vec<u8>, Vec<u8>) {
    let prefix = |path: &Path| format!("{}/", path.to_string_lossy()).into_bytes();
    (prefix(from), prefix(to))
}

/// Holds Cargo's lock files in every profile directory of `dirs` under `root`, shared: Cargo
/// holds them exclusively while it builds. `None` when a build holds one now.
fn cargo_locks(root: &Path, dirs: &[String]) -> Result<Option<Vec<File>>> {
    let mut held = Vec::new();
    for dir in dirs {
        for profile in profile_dirs(&root.join(dir)) {
            for name in CARGO_LOCKS {
                let Ok(file) = File::open(profile.join(name)) else {
                    continue;
                };
                match file.try_lock_shared() {
                    Ok(()) => held.push(file),
                    Err(fs::TryLockError::WouldBlock) => return Ok(None),
                    Err(fs::TryLockError::Error(error)) => return Err(error.into()),
                }
            }
        }
    }
    Ok(Some(held))
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Sets a file's modification time. Cargo leaves some outputs read-only, and `fs::copy` keeps
/// that mode, so the file is made writable for the moment Windows needs to open it for writing
/// attributes. Elsewhere an open for reading is enough.
fn set_modified(path: &Path, time: SystemTime) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.set_modified(time)
    }
    #[cfg(not(unix))]
    {
        let permissions = fs::metadata(path)?.permissions();
        if permissions.readonly() {
            let mut writable = permissions.clone();
            // The lint warns that this makes a Unix file world-writable; this branch runs only
            // where there is no Unix mode, and the old permissions are restored below.
            #[allow(clippy::permissions_set_readonly_false)]
            writable.set_readonly(false);
            fs::set_permissions(path, writable)?;
        }
        let result = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(time));
        if permissions.readonly() {
            fs::set_permissions(path, permissions)?;
        }
        result
    }
}

/// Rewrites the donor's paths to the tree's in the Cargo target directory `target`: in each
/// unit's dependency files and each build script's recorded output. A dependency file that
/// cannot be rewritten has its unit's fingerprint folder deleted, so the unit is rebuilt.
fn rewrite_target(target: &Path, pairs: &[(Vec<u8>, Vec<u8>)]) -> io::Result<()> {
    for profile in profile_dirs(target) {
        for unit in read_dir_paths(&profile.join(".fingerprint")) {
            for file in read_dir_paths(&unit) {
                let named = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !named.starts_with("dep-") {
                    continue;
                }
                let data = fs::read(&file)?;
                match pairs.iter().try_fold(data.clone(), |data, (old, new)| {
                    rewrite_dep_info(&data, old, new)
                }) {
                    Some(new) if new == data => {}
                    Some(new) => rewrite_file(&file, &new)?,
                    None => {
                        remove(&unit)?;
                        break;
                    }
                }
            }
        }
        for script in read_dir_paths(&profile.join("build")) {
            for name in ["output", "root-output"] {
                let file = script.join(name);
                let Ok(data) = fs::read(&file) else {
                    continue;
                };
                let new = pairs
                    .iter()
                    .fold(data.clone(), |data, (old, new)| replace(&data, old, new));
                if new != data {
                    rewrite_file(&file, &new)?;
                }
            }
        }
    }
    Ok(())
}

fn read_dir_paths(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default()
}

/// Writes `data` to `file`, keeping its modification time: Cargo compares a dependency file's
/// time with the files it lists.
fn rewrite_file(file: &Path, data: &[u8]) -> io::Result<()> {
    let modified = fs::metadata(file)?.modified()?;
    fs::write(file, data)?;
    set_modified(file, modified)
}

fn replace(data: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut rest = data;
    while let Some(at) = find(rest, old) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(new);
        rest = &rest[at + old.len()..];
    }
    out.extend_from_slice(rest);
    out
}

fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len()).position(|w| w == needle)
}

/// `data`, a Cargo dependency file, with each path that starts with `old` starting with `new`
/// instead. Cargo writes each path as its length in four little-endian bytes followed by its
/// bytes, so a path that starts with `old` starts right after its length, and the length
/// changes with the prefix. `None` when an occurrence is not where a path starts, which means
/// the format is not the one this reads.
fn rewrite_dep_info(data: &[u8], old: &[u8], new: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    let mut done = 0;
    while let Some(found) = find(&data[done..], old) {
        let at = done + found;
        let start = at.checked_sub(4)?;
        if start < done {
            return None;
        }
        let length = usize::try_from(u32::from_le_bytes(data[start..at].try_into().ok()?)).ok()?;
        if length < old.len() || at + length > data.len() {
            return None;
        }
        out.extend_from_slice(&data[done..start]);
        let new_length = u32::try_from(length - old.len() + new.len()).ok()?;
        out.extend_from_slice(&new_length.to_le_bytes());
        out.extend_from_slice(new);
        out.extend_from_slice(&data[at + old.len()..at + length]);
        done = at + length;
    }
    out.extend_from_slice(&data[done..]);
    Some(out)
}

/// The modification time each file git lists in the tree at `to` gets: that of the file at the
/// same path in `from` when their content is the same, and the present time when it is not or
/// may not be (the paths that differ between the two trees' commits, or that either tree has
/// uncommitted). Each comes with the file's own modification time when it was read.
fn file_times(from: &Path, to: &Path) -> Result<Vec<(String, SystemTime, SystemTime)>> {
    let mut differing: HashSet<String> =
        changed::between(to, &refs::head(from)?, &refs::head(to)?)?
            .into_iter()
            .collect();
    differing.extend(changed::uncommitted(from)?);
    differing.extend(changed::uncommitted(to)?);
    let now = SystemTime::now();
    let mut times = Vec::new();
    for file in tracked::files(to)? {
        let read = match fs::symlink_metadata(to.join(&file)) {
            Ok(meta) if meta.is_file() => meta.modified()?,
            _ => continue,
        };
        let time = if differing.contains(&file) {
            now
        } else {
            match fs::symlink_metadata(from.join(&file)) {
                Ok(meta) if meta.is_file() => meta.modified()?,
                _ => now,
            }
        };
        times.push((file, time, read));
    }
    Ok(times)
}

#[path = "seed_copy.rs"]
mod copy;
#[path = "seed_resume.rs"]
mod resume;
#[path = "seed_swap.rs"]
mod swap;
#[cfg(test)]
#[path = "seed_tests.rs"]
mod tests;
