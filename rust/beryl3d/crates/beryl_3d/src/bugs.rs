//! Bug reports (docs/beryl-3d.md, "Bug reports"): the app's side of the client's bug report.
//! On `Request::FreezeFrame` the host reads back the next frame it draws, which shows the
//! frozen table with nothing of the report, and the element tree of that frame is kept beside
//! it as `just b3d tree` prints it. On `Request::SaveBug` the report is written with
//! `beryl_session::bugs::write_attached` into the shared bugs folder, off the frame
//! thread: on the session thread in a live game, so that the replay and the latest view are the
//! game's as of the commands sent before it, and on a thread of its own for a replayed
//! recording. Beside the files the web's report has, it holds `tree.txt` and the frame log.
//! `Request::ExportBugs` zips every open report into the Downloads folder.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use app_client::Client;
use app_client::Input;
use app_client::command::Request;
use beryl_host::Inbox;
use beryl_render::Picture;
use beryl_session::GameUpdate;
use beryl_session::Sessions;
use beryl_session::bugs::{self, Attached, BugReport, Environment, Images, PageInfo, Region};
use kit_probe::agent::{TreeOptions, tree};
use kit_ui::input::{Rect, Size};
use serde_json::json;

use crate::fixture::Replayed;
use crate::session::Commands;

/// The line the app prints, followed by the folder, when a report is saved.
pub const SAVED: &str = "beryl3d: bug report saved in";
/// The line the app prints, followed by the file, when the reports are exported.
pub const EXPORTED: &str = "beryl3d: bug reports exported to";

/// How many of the latest views a report keeps as its `fixture.json`.
const VIEWS: usize = 20;

/// What the app keeps for the bug report between its requests.
pub struct Bugs {
    /// Where reports are written: the web app's bugs folder.
    folder: PathBuf,
    /// Every report from this launch carries the same run ID.
    environment: Environment,
    /// Whether the report has asked for a picture the host has not been asked to read back.
    wanted: bool,
    /// Whether the frame just drawn is the report's picture, whose tree the next `after`
    /// keeps.
    tree_due: bool,
    /// For each read-back asked of the host and not yet delivered, whether the live socket and
    /// whether the report asked for it.
    takers: VecDeque<(bool, bool)>,
    picture: Option<Picture>,
    tree: Option<String>,
    /// The latest views, oldest first.
    views: VecDeque<GameUpdate>,
    /// The moment of the recording the table was frozen at, when one was replaying.
    replayed: Option<Replayed>,
    /// The host's inbox, which a report written on a thread of its own answers.
    inbox: Option<Inbox>,
}

impl Bugs {
    /// Writes reports into `folder`.
    #[must_use]
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            environment: Environment::for_beryl3d_run(),
            wanted: false,
            tree_due: false,
            takers: VecDeque::new(),
            picture: None,
            tree: None,
            views: VecDeque::new(),
            replayed: None,
            inbox: None,
        }
    }

    /// Answers reports written on a thread of their own through `inbox`.
    pub fn attach(&mut self, inbox: Inbox) {
        self.inbox = Some(inbox);
    }

    /// The desktop's bugs folder for the profile `BERYL_PROFILE` names (`beryl_shell::Dirs`),
    /// or `$BERYL_BUGS`.
    #[must_use]
    pub fn for_installed_app() -> Self {
        let dirs = beryl_shell::Dirs::for_installed_app(beryl_shell::Profile::current());
        Self::new(dirs.map_or_else(bugs::dir, |dirs| dirs.bugs))
    }

    /// Keeps a view the client was given, for the report's `fixture.json`.
    pub fn seen(&mut self, input: &Input) {
        if let Input::Update(update) = input {
            if self.views.len() == VIEWS {
                self.views.pop_front();
            }
            self.views.push_back((**update).clone());
        }
    }

    /// The client has frozen the table for a report: the next frame drawn is its picture.
    /// `replayed` is the moment of the recording being replayed, if one is, which the report's
    /// `ui.json` records so that it can be reopened with no game.
    pub fn freeze(&mut self, replayed: Option<Replayed>) {
        self.wanted = true;
        self.replayed = replayed;
        self.picture = None;
        self.tree = None;
    }

    /// Whether to read back the frame just drawn, for the live socket if `live` or for the
    /// report.
    pub fn capture_wanted(&mut self, live: bool) -> bool {
        let report = std::mem::take(&mut self.wanted);
        self.tree_due = report;
        if live || report {
            self.takers.push_back((live, report));
        }
        live || report
    }

    /// Takes a picture the host read back, and returns it if the live socket asked for it.
    pub fn captured(&mut self, picture: Picture) -> Option<Picture> {
        let (live, report) = self.takers.pop_front()?;
        match (live, report) {
            (true, true) => {
                self.picture = Some(Picture {
                    pixels: picture.pixels.clone(),
                    width: picture.width,
                    height: picture.height,
                });
                Some(picture)
            }
            (false, true) => {
                self.picture = Some(picture);
                None
            }
            _ => Some(picture),
        }
    }

    /// After a frame: keeps the element tree of the report's picture.
    pub fn after(&mut self, client: &Client) {
        if std::mem::take(&mut self.tree_due) {
            let printed = tree(client.last_frame(), &TreeOptions::default());
            self.tree = Some(printed.unwrap_or_else(|error| error.to_string()));
        }
    }

    /// Writes the report a `Request::SaveBug` asks for, with what was kept for it and the frame
    /// log at `frame_log`: on the session thread of `live`, in a live game, and otherwise on a
    /// thread of its own. Either answers with `Input::BugSaved`.
    pub fn save(&mut self, request: &Request, frame_log: Option<&Path>, live: Option<&Commands>) {
        let Request::SaveBug {
            description,
            ai,
            region,
            window,
        } = request
        else {
            return;
        };
        let job = Job {
            saving: Saving {
                description: description.clone(),
                ai: *ai,
                region: *region,
                window: *window,
            },
            folder: self.folder.clone(),
            environment: self.environment.clone(),
            picture: self.picture.take(),
            tree: self.tree.take(),
            frame_log: frame_log.map(Path::to_owned),
            views: self.views.iter().cloned().collect(),
            replayed: self.replayed.take(),
        };
        match live {
            Some(commands) => commands.save_bug(job),
            None => self.write_apart(job),
        }
    }

    /// Writes `job` on a thread of its own and sends the result to the host's inbox.
    fn write_apart(&self, job: Job) {
        let inbox = self.inbox.clone();
        std::thread::spawn(move || {
            let saved = job.write(None);
            if let Some(inbox) = inbox {
                inbox.send(Input::BugSaved(saved));
            }
        });
    }

    /// Zips every open report into `beryl-bug-reports.zip` in the Downloads folder, on a thread
    /// of its own, and prints its path; winit offers no save dialog.
    pub fn export(&self) {
        let folder = self.folder.clone();
        let destination = downloads().join("beryl-bug-reports.zip");
        std::thread::spawn(move || {
            let saved = bugs::archive::active(&folder)
                .and_then(|folders| bugs::archive::save(&folders, &destination));
            match saved {
                Ok(()) => println!("{EXPORTED} {}", destination.display()),
                Err(error) => eprintln!("beryl3d: the bug reports were not exported: {error}"),
            }
        });
    }
}

/// What `Request::SaveBug` carries.
struct Saving {
    pub description: String,
    pub ai: bool,
    /// In logical points.
    pub region: Rect,
    pub window: Size,
}

/// A report to write, off the frame thread.
pub struct Job {
    saving: Saving,
    folder: PathBuf,
    environment: Environment,
    picture: Option<Picture>,
    tree: Option<String>,
    frame_log: Option<PathBuf>,
    views: Vec<GameUpdate>,
    replayed: Option<Replayed>,
}

impl Job {
    /// Writes the report, with what `sessions` knows of the game if there are any: its replay,
    /// its latest view and the agent's latest searches.
    ///
    /// # Errors
    ///
    /// When the report cannot be written, with the reason the client shows under its
    /// description.
    pub fn write(self, sessions: Option<&Sessions>) -> Result<(), String> {
        let saving = &self.saving;
        let window = saving.window;
        // The picture's pixels to one logical point.
        let scale = (self.picture.as_ref())
            .map_or(1.0, |picture| picture.width as f32 / window.width.max(1.0));
        let region = saving.region;
        let png = |picture: &Picture| picture.png();
        let screenshot = (self.picture.as_ref())
            .map(|picture| {
                let pixels = |value: f32| (value * scale).round().max(0.0) as u32;
                png(&picture.crop(
                    pixels(region.x),
                    pixels(region.y),
                    pixels(region.width).max(1),
                    pixels(region.height).max(1),
                ))
            })
            .transpose()?;
        let images = Images {
            window: self.picture.as_ref().map(png).transpose()?,
            screenshot,
        };
        let mut attached = Vec::new();
        if let Some(tree) = self.tree {
            attached.push(Attached {
                name: "tree.txt".to_owned(),
                bytes: tree.into_bytes(),
                note: "the elements of the frozen frame, as `just b3d tree` prints them".to_owned(),
            });
        }
        if let Some(path) = &self.frame_log
            && let Ok(bytes) = std::fs::read(path)
        {
            attached.push(Attached {
                name: "frames.jsonl".to_owned(),
                bytes,
                note: format!("Beryl 3D's frame log so far, `{}`", path.display()),
            });
        }
        let report = BugReport {
            description: saving.description.clone(),
            ai: saving.ai,
            snapshot: None,
            region: Some(Region {
                x: f64::from(region.x),
                y: f64::from(region.y),
                width: f64::from(region.width),
                height: f64::from(region.height),
            }),
            page: PageInfo {
                url: "beryl3d".to_owned(),
                width: f64::from(window.width),
                height: f64::from(window.height),
                scale: f64::from(scale),
            },
            ui_log: match &self.replayed {
                Some(replayed) => json!({
                    "client": "Beryl 3D",
                    "recording": replayed.recording,
                    "seat": replayed.seat,
                    "at": replayed.at_ms,
                    "ended": replayed.ended,
                }),
                None => json!({ "client": "Beryl 3D" }),
            },
            updates: (self.views.iter())
                .map(|update| serde_json::to_value(update).unwrap_or_default())
                .collect(),
            spec: None,
            spec_viewer: false,
        };
        let none;
        let sessions = if let Some(sessions) = sessions {
            sessions
        } else {
            none = Sessions::new(&beryl_cards::CARDS, None, |_| {});
            &none
        };
        let saved = bugs::write_attached(
            &self.folder,
            &report,
            &images,
            &attached,
            sessions,
            &self.environment,
        )
        .map_err(|error| format!("Not saved: {error:?}"))?;
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        bugs::filing::enqueue(&self.folder, &saved.id, bundled_filing());
        println!("{SAVED} {}", saved.path);
        Ok(())
    }
}

/// Finder launches the app without the shell's tool paths. A locally built bundle records them.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn bundled_filing() -> Option<(&'static str, &'static str, &'static str)> {
    Some((
        option_env!("BERYL_BUNDLE_BUGS_REPO")?,
        option_env!("BERYL_BUNDLE_BUGS_UV")?,
        option_env!("BERYL_BUNDLE_BUGS_PATH")?,
    ))
}

/// The Downloads folder in the home folder.
fn downloads() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    home.map_or_else(|| Path::new(".").to_owned(), PathBuf::from)
        .join("Downloads")
}
