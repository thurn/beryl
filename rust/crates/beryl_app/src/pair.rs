//! Starts a debug localhost table when a window launched by `just table-pair` reports ready.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, Instant};

use beryl_session::ipc::decks::DeckImportRequest;
use beryl_session::ipc::{
    Platform, TableActReply, TableHostRequest, TableHostSettings, TableJoinRequest, TableLink,
};
use beryl_session::journal;
use beryl_session::protocol::TableAction;
use beryl_table::Seat;
use beryl_table_session::connections::TableConnections;
use beryl_table_session::decks::{self, Decks};
use serde_json::json;
use tauri::{AppHandle, Manager};

const SAMPLE_DECK: &str =
    include_str!("../../../../docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt");
const WAIT_LIMIT: Duration = Duration::from_secs(60);
static STARTED: Once = Once::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Host,
    Guest,
}

impl Role {
    fn from_env(value: Option<&OsStr>) -> Option<Self> {
        match value.and_then(OsStr::to_str) {
            Some("host") => Some(Self::Host),
            Some("guest") => Some(Self::Guest),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Host => "Host",
            Self::Guest => "Guest",
        }
    }

    fn seat(self) -> Seat {
        match self {
            Self::Host => Seat(0),
            Self::Guest => Seat(1),
        }
    }

    fn open(
        self,
        tables: &TableConnections,
        library: &Decks,
        mailbox: &Path,
    ) -> Result<(), String> {
        let invite = mailbox.join("invite");
        let ready = mailbox.join("guest-ready");
        match self {
            Self::Host => {
                let host = tables
                    .host(
                        Platform::current(),
                        TableHostRequest {
                            link: TableLink::Local,
                            settings: TableHostSettings {
                                name: self.name().into(),
                                starting_life: None,
                                starting_hand_size: None,
                            },
                        },
                    )
                    .map_err(|error| error.reason)?;
                self.load_deck(tables, library)?;
                // The guest can see the invite as soon as it exists, so publish it whole.
                let pending = mailbox.join("invite.tmp");
                fs::write(&pending, host.invite).map_err(message)?;
                fs::rename(pending, invite).map_err(message)?;
                wait_for("the guest to load its deck", || read_signal(&ready))?;
                act(tables, TableAction::Start { first: self.seat() })
            }
            Self::Guest => {
                let invite = wait_for("the host's invite", || read_signal(&invite))?;
                tables
                    .join(TableJoinRequest {
                        invite,
                        name: self.name().into(),
                    })
                    .map_err(|error| error.reason)?;
                // Joining returns after Welcome, before the first view enables guest actions.
                wait_for("the guest's first view", || Ok(tables.current()))?;
                self.load_deck(tables, library)?;
                // This is written after the host accepts the load, so Start can deal both decks.
                fs::write(ready, "").map_err(message)?;
                wait_for("the opening hand", || {
                    Ok(tables.current().filter(|update| {
                        update
                            .view
                            .table
                            .as_ref()
                            .is_some_and(|table| !table.orders.you.hand.is_empty())
                    }))
                })?;
                Ok(())
            }
        }
    }

    fn load_deck(self, tables: &TableConnections, library: &Decks) -> Result<(), String> {
        let deck = decks::import(
            library,
            DeckImportRequest {
                name: "Table pair sample deck".into(),
                text: Some(SAMPLE_DECK.into()),
                cod: None,
            },
        )
        .map_err(message)?;
        act(
            tables,
            TableAction::LoadDeck {
                seat: self.seat(),
                main: deck.main,
                side: deck.side,
            },
        )
    }
}

pub(crate) fn start(app: &AppHandle) {
    let Some(role) = Role::from_env(std::env::var_os("BERYL_TABLE_PAIR").as_deref()) else {
        return;
    };
    // React may report ready twice, including after a reload.
    STARTED.call_once(|| {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(reason) = open_window(&app, role) {
                let reason = format!("table-pair ({}): {reason}", role.name());
                eprintln!("{reason}");
                journal::error("app", reason, json!(null));
                app.exit(1);
            }
        });
    });
}

fn open_window(app: &AppHandle, role: Role) -> Result<(), String> {
    let cache = std::env::var_os("BERYL_CACHE").ok_or("BERYL_CACHE is required by table-pair")?;
    role.open(
        &app.state::<TableConnections>(),
        &crate::decks::library(app).map_err(message)?,
        &PathBuf::from(cache).join("table-pair"),
    )?;
    let window = app
        .get_webview_window("main")
        .ok_or("could not find the table window")?;
    window
        .set_title(&format!("Beryl — {}", role.name()))
        .map_err(message)?;
    window
        .eval("window.location.hash = '#/table'")
        .map_err(message)
}

fn act(tables: &TableConnections, action: TableAction) -> Result<(), String> {
    match tables.act(action) {
        TableActReply::Ok { .. } => Ok(()),
        TableActReply::Refused(refusal) => {
            Err(format!("table action refused: {:?}", refusal.refused))
        }
    }
}

fn read_signal(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

fn wait_for<T>(
    description: &str,
    mut poll: impl FnMut() -> Result<Option<T>, String>,
) -> Result<T, String> {
    let deadline = Instant::now() + WAIT_LIMIT;
    loop {
        if let Some(value) = poll()? {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {description}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use beryl_session::SessionIds;
    use beryl_session::protocol::LinkState;

    #[test]
    fn only_explicit_pair_roles_enable_startup() {
        let role = |value: &str| Role::from_env(Some(OsStr::new(value)));
        assert_eq!(Role::from_env(None), None);
        assert_eq!(role("host"), Some(Role::Host));
        assert_eq!(role("guest"), Some(Role::Guest));
        for invalid in ["", "HOST", "guest ", "solo"] {
            assert_eq!(role(invalid), None);
        }
    }

    #[test]
    fn a_local_pair_deals_both_opening_hands() {
        let directory = tempfile::tempdir().unwrap();
        let mailbox = directory.path();
        std::thread::scope(|scope| {
            let start = |role: Role| {
                let profile = mailbox.join(role.name());
                let tables =
                    TableConnections::new(SessionIds::default(), profile.clone(), |_| {}, |_| {});
                let library = Decks::new(profile.join("decks"));
                role.open(&tables, &library, mailbox).unwrap();

                let view = tables.current().unwrap().view.table.unwrap();
                assert_eq!(view.seat.unwrap_or(0), role.seat().0);
                assert!(matches!(
                    view.link,
                    LinkState::Connected { direct: true, .. }
                ));
                assert_eq!(view.orders.you.hand.len(), 7);
                assert_eq!(view.orders.opponent.hand.len(), 7);
                assert_eq!(decks::list(&library).unwrap().len(), 1);
                tables
            };

            let host = scope.spawn(move || start(Role::Host));
            let _guest = start(Role::Guest);
            let _host = host.join().unwrap();
        });
    }
}
