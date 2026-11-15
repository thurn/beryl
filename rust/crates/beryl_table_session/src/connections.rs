//! The linked table commands shared by Tauri and the local HTTP server.

mod incoming;
#[cfg(feature = "iroh")]
mod remote;

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use beryl_link::{Invite, TcpHost, TcpLink};
use beryl_session::ipc::{
    LobbyChange, LobbyUpdate, Platform, SavedTable, TableActReply, TableConnectionError,
    TableHostReply, TableHostRequest, TableJoinRequest, TableLink, TableLobbyRequest, TableRefused,
    TableResumeRequest, TableSettings, TableSoloRequest,
};
use beryl_session::protocol::{Answer, TableAction};
use beryl_session::{GameUpdate, SessionId, SessionIds};

use crate::{HostSettings, TableEvent, TableSessions};
use incoming::{Incoming, MemoryInvite};

pub use incoming::MemoryInvites;

const NO_INTERNET: &str = "This build has no internet play.";
const NO_HOSTING: &str = "Hosting is not available on this device.";

pub struct TableConnections {
    tables: TableSessions,
    data_dir: PathBuf,
    memory: Option<MemoryInvites>,
    // Serialize changing the current table with replacing its listener. Table
    // actions and views use the session's own locks.
    listener: Mutex<Option<Incoming>>,
}

impl TableConnections {
    pub fn new(
        ids: SessionIds,
        data_dir: PathBuf,
        emit: impl Fn(GameUpdate) + Send + Sync + 'static,
        emit_event: impl Fn(TableEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            tables: TableSessions::with_data_dir(ids, &data_dir, emit).with_events(emit_event),
            data_dir,
            memory: None,
            listener: Mutex::default(),
        }
    }

    /// Enable in-process links for two separate views in the HTTP server.
    /// The server supports local links even if its build includes iroh.
    #[must_use]
    pub fn for_server(mut self, memory: MemoryInvites) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn solo(&self, request: TableSoloRequest) -> Result<SessionId, TableRefused> {
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        let session = self.tables.solo(request.deck).map_err(TableRefused::from)?;
        *listener = None;
        Ok(session)
    }

    pub fn lobby(&self, request: TableLobbyRequest) -> Result<Option<LobbyUpdate>, TableRefused> {
        match request.change {
            LobbyChange::View => return Ok(self.tables.lobby()),
            LobbyChange::Solo => {
                let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
                self.tables.solo_lobby()?;
                *listener = None;
            }
            _ => self.tables.change_lobby(request)?,
        }
        Ok(None)
    }

    pub fn act(&self, action: TableAction) -> TableActReply {
        self.tables.act(action).into()
    }

    pub fn answer(&self, answer: Answer) -> TableActReply {
        self.tables.answer(answer).into()
    }

    #[cfg(debug_assertions)]
    pub fn open_test_sort(&self) -> TableActReply {
        use crate::prompt::PromptState;
        use beryl_table::action::Flow;
        use beryl_table::assist::text::{Flow as Effect, flow_result};
        use beryl_table::{Refusal, Seat};

        self.tables
            .open_library_prompt(Seat(0), "Scry".into(), Flow::Scry, 2, |state| {
                let PromptState::Sort(sort) = state else {
                    return Err(Refusal::INVALID);
                };
                flow_result(Effect::Scry(2), sort.cards(), &sort.choice())
            })
            .into()
    }

    pub fn current(&self) -> Option<GameUpdate> {
        self.tables.current()
    }

    pub fn leave(&self) {
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        self.tables.leave();
        *listener = None;
    }

    pub fn host(
        &self,
        platform: Platform,
        request: TableHostRequest,
    ) -> Result<TableHostReply, TableConnectionError> {
        check_hosting(platform)?;
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        let (incoming, links, invite) = match request.link {
            TableLink::Local => {
                let (host, invite) = TcpHost::listen().map_err(error)?;
                let (incoming, links) = Incoming::tcp(host).map_err(error)?;
                (incoming, links, ConnectionInvite::from(invite))
            }
            TableLink::Memory => {
                let invite = MemoryInvite::new();
                let (incoming, links) = self.memory()?.listen(&invite).map_err(error)?;
                (incoming, links, ConnectionInvite::Memory(invite))
            }
            TableLink::Iroh => {
                self.check_internet()?;
                #[cfg(feature = "iroh")]
                {
                    let (host, invite) =
                        remote::open(&self.data_dir, self.settings()?.relay).map_err(error)?;
                    let (incoming, links) = remote::incoming(host);
                    (incoming, links, ConnectionInvite::from(invite))
                }
                #[cfg(not(feature = "iroh"))]
                return Err(error(NO_INTERNET));
            }
        };
        let invite_text = invite.text();
        let mut settings = HostSettings::new(request.settings.name, &invite_text, invite.secret()?);
        if let Some(life) = request.settings.starting_life {
            settings.starting_life = life;
        }
        if let Some(hand_size) = request.settings.starting_hand_size {
            settings.starting_hand_size = hand_size;
        }
        self.tables.host(links, settings).map_err(error)?;
        *listener = Some(incoming);
        Ok(TableHostReply {
            invite: invite_text,
        })
    }

    pub fn join(&self, request: TableJoinRequest) -> Result<SessionId, TableConnectionError> {
        self.join_with_wire(request, beryl_session::WIRE_HASH)
    }

    #[cfg(debug_assertions)]
    pub fn join_mismatched_for_test(
        &self,
        request: TableJoinRequest,
    ) -> Result<SessionId, TableConnectionError> {
        self.join_with_wire(request, "different test wire")
    }

    #[cfg(debug_assertions)]
    pub fn cut_host_for_test(&self) {
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        // Close admission before cutting the connection so the guest cannot redial
        // until the host resumes the saved table.
        *listener = None;
        self.tables.cut_host_for_test();
    }

    fn join_with_wire(
        &self,
        request: TableJoinRequest,
        wire: &str,
    ) -> Result<SessionId, TableConnectionError> {
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        let invite = ConnectionInvite::parse(request.invite.trim())
            .map_err(|_| error("Invalid invite code."))?;
        let session = match invite {
            ConnectionInvite::Link(ref invite @ Invite::Local { ref secret, .. }) => {
                let invite = invite.clone();
                let secret = secret.clone();
                self.tables.join_checked_with_wire(
                    move || TcpLink::connect(&invite),
                    request.name,
                    secret,
                    wire,
                )
            }
            ConnectionInvite::Memory(invite) => {
                let registry = self.memory()?.clone();
                let secret = invite.secret().to_owned();
                self.tables.join_checked_with_wire(
                    move || registry.connect(&invite),
                    request.name,
                    secret,
                    wire,
                )
            }
            ConnectionInvite::Link(invite @ Invite::Remote(_)) => {
                self.check_internet()?;
                #[cfg(feature = "iroh")]
                {
                    let secret = remote::secret(&invite).map_err(error)?;
                    let connect = remote::connector(&self.data_dir, invite).map_err(error)?;
                    self.tables
                        .join_checked_with_wire(connect, request.name, secret, wire)
                }
                #[cfg(not(feature = "iroh"))]
                {
                    let _ = invite;
                    return Err(error(NO_INTERNET));
                }
            }
        }
        .map_err(error)?;
        *listener = None;
        Ok(session)
    }

    pub fn resume(
        &self,
        platform: Platform,
        request: &TableResumeRequest,
    ) -> Result<TableHostReply, TableConnectionError> {
        check_hosting(platform)?;
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        let invite_text = self.tables.saved_invite(&request.id).map_err(error)?;
        let (incoming, links) = match ConnectionInvite::parse(&invite_text)? {
            ConnectionInvite::Link(invite @ Invite::Local { .. }) => {
                Incoming::tcp(TcpHost::resume(&invite).map_err(error)?)
            }
            ConnectionInvite::Memory(invite) => self.memory()?.listen(&invite),
            ConnectionInvite::Link(invite @ Invite::Remote(_)) => {
                self.check_internet()?;
                #[cfg(feature = "iroh")]
                {
                    Ok(remote::incoming(
                        remote::resume(&self.data_dir, &invite).map_err(error)?,
                    ))
                }
                #[cfg(not(feature = "iroh"))]
                {
                    let _ = invite;
                    return Err(error(NO_INTERNET));
                }
            }
        }
        .map_err(error)?;
        self.tables.resume(&request.id, links).map_err(error)?;
        *listener = Some(incoming);
        Ok(TableHostReply {
            invite: invite_text,
        })
    }

    pub fn saved(&self) -> Result<Vec<SavedTable>, TableConnectionError> {
        self.tables.list_saved().map_err(error)
    }

    pub fn settings(&self) -> Result<TableSettings, TableConnectionError> {
        match File::open(self.settings_path()) {
            Ok(file) => serde_json::from_reader(io::BufReader::new(file)).map_err(error),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(TableSettings::default()),
            Err(cause) => Err(error(cause)),
        }
    }

    pub fn set_settings(&self, settings: &TableSettings) -> Result<(), TableConnectionError> {
        if let Some(relay) = &settings.relay {
            validate_relay(relay)?;
        }
        write_settings(&self.settings_path(), settings).map_err(error)
    }

    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    fn memory(&self) -> Result<&MemoryInvites, TableConnectionError> {
        self.memory
            .as_ref()
            .ok_or_else(|| error("This build cannot open in-process tables."))
    }

    fn check_internet(&self) -> Result<(), TableConnectionError> {
        if self.memory.is_some() || !cfg!(feature = "iroh") {
            return Err(error(NO_INTERNET));
        }
        Ok(())
    }
}

fn check_hosting(platform: Platform) -> Result<(), TableConnectionError> {
    if matches!(platform, Platform::Ios | Platform::Android) {
        return Err(error(NO_HOSTING));
    }
    Ok(())
}

fn validate_relay(text: &str) -> Result<(), TableConnectionError> {
    let valid = url::Url::parse(text).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    });
    if valid {
        Ok(())
    } else {
        Err(error("The Beryl relay URL is invalid."))
    }
}

fn write_settings(path: &Path, settings: &TableSettings) -> io::Result<()> {
    let directory = path
        .parent()
        .expect("settings are inside the data directory");
    fs::create_dir_all(directory)?;
    let mut pending = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(&mut pending, settings)?;
    pending.as_file().sync_all()?;
    pending.persist(path)?;
    Ok(())
}

fn error(error: impl std::fmt::Display) -> TableConnectionError {
    TableConnectionError::from_error(error)
}

enum ConnectionInvite {
    Link(Invite),
    Memory(MemoryInvite),
}

impl From<Invite> for ConnectionInvite {
    fn from(value: Invite) -> Self {
        Self::Link(value)
    }
}

impl ConnectionInvite {
    fn parse(text: &str) -> Result<Self, TableConnectionError> {
        if let Some(invite) = MemoryInvite::parse(text).map_err(error)? {
            return Ok(Self::Memory(invite));
        }
        text.parse::<Invite>().map(Self::Link).map_err(error)
    }

    fn text(&self) -> String {
        match self {
            Self::Link(invite) => invite.to_string(),
            Self::Memory(invite) => invite.text(),
        }
    }

    fn secret(&self) -> Result<String, TableConnectionError> {
        match self {
            Self::Link(Invite::Local { secret, .. }) => Ok(secret.clone()),
            Self::Memory(invite) => Ok(invite.secret().to_owned()),
            #[cfg(feature = "iroh")]
            Self::Link(invite @ Invite::Remote(_)) => remote::secret(invite).map_err(error),
            #[cfg(not(feature = "iroh"))]
            Self::Link(Invite::Remote(_)) => Err(error(NO_INTERNET)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phones_cannot_host_or_resume_tables() {
        for platform in [Platform::Ios, Platform::Android] {
            let directory = tempfile::tempdir().unwrap();
            let tables = TableConnections::new(
                SessionIds::default(),
                directory.path().into(),
                |_| {},
                |_| {},
            );
            let result = tables.host(
                platform,
                TableHostRequest {
                    link: TableLink::Local,
                    settings: beryl_session::ipc::TableHostSettings {
                        name: "Host".into(),
                        starting_life: None,
                        starting_hand_size: None,
                    },
                },
            );
            assert_eq!(result.unwrap_err().reason, NO_HOSTING);
            let result = tables.resume(
                platform,
                &TableResumeRequest {
                    id: "missing".into(),
                },
            );
            assert_eq!(result.unwrap_err().reason, NO_HOSTING);
            assert!(tables.current().is_none());
        }
    }
}
