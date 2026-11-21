//! The daemon's HTTP client for the hub, on `ureq`.
//!
//! Every request carries the machine's token as `Authorization: Bearer <token>` and the writer as
//! `X-Actor`. A request that cannot reach the hub, or that the hub answers with a server error,
//! is tried again a few times; after that it fails with [`Error::Unreachable`], which verbs
//! report with exit code 4.
//!
//! A request the hub answers with 401 is sent once more after the client reads the token again
//! from the machine's identity file, when it was given one ([`HubClient::reading_token_from`]):
//! an `ignite install` that registered the machine again while the daemon or a verb process ran
//! may have saved a token other than the one they read at their start.
//!
//! A body larger than the hub reads ([`MAX_BODY_BYTES`]), or one the hub answers with 413, fails
//! with [`Error::Invalid`] whose message starts with [`TOO_LARGE`] (see [`too_large`]), so that a
//! caller can send the same content again in smaller pieces.

use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use ignite_core::config::HubConfig;
use ignite_core::ids::{Commit, RecordId, Seq, TicketId};
use ignite_core::protocol::cli::TranscriptAppend;
use ignite_core::protocol::hub::{
    AdhocClaim, AuditReport, CachedResult, Claim, Close, ExplorationFind, FlakyReport, Heartbeat,
    HeartbeatReply, IncompleteReport, JusticiarReport, Landed, LandedCommit, LandedReply,
    LandingsReply, LandingsRequest, ListingKey, ListingLookup, ListingReply, LookupReply,
    LookupRequest, MAX_BODY_BYTES, MachineFree, MachineRegister, MachineRegistered, MetricsImport,
    NewResult, OpenRegression, OpenRegressions, Pause, Record, RecordBatch, RegressionFiled,
    RegressionReport, Release, ReportFiled, Reservation, ResultKey, RevertFiled, Reverted,
    SessionTitle, StoreRequest, StoredListing, TicketBrief, TipClaim, TipClaimReply, TipVerified,
    TipWaiting, TipWaitingFiled, Verdict, VerdictKey,
};
use ignite_core::protocol::tickets::{
    BatchReply, BatchRequest, FeedBatch, ImportReply, ImportRequest, Lease, LeaseBusy, LeaseOp,
    LeaseReply, Ticket, TicketDetail, TicketQuery, WholeTicket,
};
use ignite_core::tipround::{
    LastTipRound, TipCosts, TipCostsRequest, TipCoverage, TipCoverageUpdate, TipRound,
    TipRoundFiled,
};
use ignite_core::traits::{Error, Hub, Result};

use crate::update::IdentityFile;

/// How an error for a body too large for the hub begins; see [`too_large`].
pub const TOO_LARGE: &str = "413 ";

/// Whether `error` says that a request's body was too large for the hub: the hub answered 413,
/// or the body was over [`MAX_BODY_BYTES`] and was not sent. Sending the same body again can
/// never succeed, but sending its content in smaller pieces can.
pub fn too_large(error: &Error) -> bool {
    matches!(error, Error::Invalid(message) if message.starts_with(TOO_LARGE))
}

/// What the hub's read-only listener says in the body of its 405 answer to a write.
const READ_ONLY_BODY: &str = "this listener is read-only";

/// How an error for a write that reached the hub's read-only listener begins; see
/// [`read_only`].
pub const READ_ONLY: &str = "hub URL reaches the read-only listener: ";

/// Whether `error` says that the configured hub URL reaches the hub's read-only listener. That
/// listener refuses every write and never stops doing so, so a request does not try again (a 405
/// is never retried), and a loop that repeats the write reports it once (see [`FailureLog`]).
pub fn read_only(error: &Error) -> bool {
    matches!(error, Error::Unreachable(message) if message.starts_with(READ_ONLY))
}

/// Reports failed writes to the hub on standard error. A read-only listener is said once and
/// then not again until a write succeeds, so a wrong hub URL fills the log with one line and
/// not one per round.
#[derive(Debug, Default)]
pub struct FailureLog {
    said_read_only: std::sync::atomic::AtomicBool,
}

impl FailureLog {
    /// Prints `ignite: <what>: <error>`, except that a read-only listener's error is printed
    /// once, until [`FailureLog::succeeded`].
    pub fn failed(&self, what: &str, error: &Error) {
        if self.worth_saying(error) {
            eprintln!("ignite: {what}: {error}");
        }
    }

    /// Whether `error` is to be printed: always, except for a read-only listener already said.
    fn worth_saying(&self, error: &Error) -> bool {
        !(read_only(error)
            && self
                .said_read_only
                .swap(true, std::sync::atomic::Ordering::Relaxed))
    }

    /// Notes that a write reached the hub, so a later read-only answer is reported again.
    pub fn succeeded(&self) {
        self.said_read_only
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The failures of the daemon's loops writing to the hub, which share one line.
pub static HUB_WRITES: FailureLog = FailureLog {
    said_read_only: std::sync::atomic::AtomicBool::new(false),
};

/// How long an ordinary request may take before it counts as unreachable.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a request goes on trying a hub that is restarting: one whose port refuses
/// connections, or whose proxy (`tailscale serve`) answers 502, 503 or 504 because nothing
/// listens behind it. The hub's own service restarts it in well under a second, and a daemon
/// that served the hub took longer, so a restart within this window is invisible to callers.
pub const RESTART_WINDOW: Duration = Duration::from_secs(15);

/// The longest pause between two tries.
const MAX_RETRY_PAUSE: Duration = Duration::from_secs(2);

/// The hub at `base` (`<url>/<project>/api`), writing as `actor` with the machine's token.
#[derive(Clone, Debug)]
pub struct HubClient {
    pub base: String,
    /// The token sent as the bearer, shared by the client's clones so that a token read again
    /// serves all of them.
    token: Arc<RwLock<String>>,
    /// The file the token is read from again after a 401 (see the module documentation).
    token_file: Option<IdentityFile>,
    pub actor: String,
    /// How many times a failed request is tried again.
    pub retries: u32,
    /// The pause before the first retry; each later retry waits one pause longer, up to two
    /// seconds.
    pub retry_pause: Duration,
    /// How long a hub that looks restarting is tried, beyond `retries` (see [`RESTART_WINDOW`]).
    pub restart_window: Duration,
    agent: ureq::Agent,
}

/// A response the hub sent, with its status.
struct Answer {
    status: u16,
    body: Vec<u8>,
}

impl HubClient {
    /// The client for the hub that `ignite.toml` names.
    pub fn new(hub: &HubConfig, token: &str, actor: &str) -> HubClient {
        let base = format!("{}/{}/api", hub.url.trim_end_matches('/'), hub.project);
        HubClient::with_base(base, token, actor)
    }

    /// The client for the API at `base`.
    pub fn with_base(base: impl Into<String>, token: &str, actor: &str) -> HubClient {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build();
        HubClient {
            base: base.into(),
            token: Arc::new(RwLock::new(token.to_owned())),
            token_file: None,
            actor: actor.to_owned(),
            retries: 3,
            retry_pause: Duration::from_millis(500),
            restart_window: RESTART_WINDOW,
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// The client, reading the token again from `file` when the hub answers 401.
    #[must_use]
    pub fn reading_token_from(mut self, file: IdentityFile) -> HubClient {
        self.token_file = Some(file);
        self
    }

    /// The token the client sends.
    pub fn token(&self) -> String {
        self.token
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Reads the token again from the identity file, and says whether a request refused with
    /// 401 is worth sending again: there is a file and it holds a token. The token in the file
    /// replaces the one held even when they are the same, since the file is what
    /// `ignite install` writes.
    fn reread_token(&self) -> bool {
        let Some(file) = &self.token_file else {
            return false;
        };
        let token = match file.load() {
            Ok(Some(identity)) => identity.token,
            Ok(None) => None,
            Err(error) => {
                eprintln!("ignite: cannot read the machine's token again: {error}");
                None
            }
        };
        let Some(token) = token else {
            return false;
        };
        *self.token.write().unwrap_or_else(PoisonError::into_inner) = token;
        true
    }

    /// `POST /machines/register`.
    pub fn register(&self, machine: &MachineRegister) -> Result<MachineRegistered> {
        self.post("/machines/register", machine)
    }

    /// `POST /machines/heartbeat`.
    pub fn heartbeat(&self, heartbeat: &Heartbeat) -> Result<HeartbeatReply> {
        self.post("/machines/heartbeat", heartbeat)
    }

    /// `POST /claims` with a summary: files an `adhoc` ticket and claims it.
    pub fn claim_adhoc(&self, claim: &AdhocClaim) -> Result<TicketBrief> {
        self.post("/claims", claim)
    }

    /// `POST /release`: gives a held ticket back with its note and saved branch.
    pub fn release(&self, release: &Release) -> Result<()> {
        let _: serde_json::Value = self.post("/release", release)?;
        Ok(())
    }

    /// `POST /close`: closes a held ticket without a landing.
    pub fn close(&self, close: &Close) -> Result<()> {
        let _: serde_json::Value = self.post("/close", close)?;
        Ok(())
    }

    /// `GET <path>` with the query's pairs.
    fn get<R: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<R> {
        let url = format!("{}{path}", self.base);
        let answer = self.with_retries(path, || {
            let mut request = self.authorized(self.agent.get(&url));
            for (key, value) in query {
                request = request.query(*key, value);
            }
            request.call()
        })?;
        decode(path, &self.ok(path, answer)?.body)
    }

    fn post<B: Serialize, R: DeserializeOwned>(&self, path: &str, body: &B) -> Result<R> {
        let answer = self.post_raw(path, body)?;
        decode(path, &self.ok(path, answer)?.body)
    }

    fn post_raw<B: Serialize>(&self, path: &str, body: &B) -> Result<Answer> {
        let bytes = serde_json::to_vec(body).map_err(|e| Error::Invalid(e.to_string()))?;
        // The hub answers an oversized body with 413 and closes the connection before reading
        // it, so the client, still writing, sees a broken pipe instead of the 413. Checking the
        // size first gives the caller the verdict without sending the body.
        if bytes.len() as u64 > MAX_BODY_BYTES {
            return Err(Error::Invalid(format!(
                "{TOO_LARGE}{path}: the body is {} bytes, over the hub's limit of \
                 {MAX_BODY_BYTES}; it was not sent",
                bytes.len()
            )));
        }
        let url = format!("{}{path}", self.base);
        self.with_retries(path, || {
            self.authorized(self.agent.post(&url))
                .header("Content-Type", "application/json")
                .send(&bytes[..])
        })
    }

    fn authorized<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        request
            .header("Authorization", &format!("Bearer {}", self.token()))
            .header("X-Actor", &self.actor)
    }

    /// Sends a request, trying again while the hub cannot be reached or answers with a server
    /// error: `retries` times, and for as long as [`HubClient::restart_window`] while the hub
    /// looks like it is restarting ([`restarting_status`], [`restarting_error`]). A 401 is sent
    /// again once, after the token is read again ([`HubClient::reading_token_from`]); `send`
    /// builds each request afresh, so the new token goes with it.
    fn with_retries(
        &self,
        path: &str,
        send: impl Fn() -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Answer> {
        let began = std::time::Instant::now();
        let mut last;
        let mut attempt: u32 = 0;
        let mut reread = false;
        loop {
            if attempt > 0 {
                std::thread::sleep((self.retry_pause * attempt).min(MAX_RETRY_PAUSE));
            }
            let restarting = match send() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let body = response
                        .into_body()
                        .read_to_vec()
                        .map_err(|e| Error::Unreachable(format!("{path}: {e}")));
                    match body {
                        Ok(_) if status == 401 && !reread && self.reread_token() => {
                            reread = true;
                            eprintln!(
                                "ignite: the hub refused this machine's token at {path}; \
                                 sending it again with the token read again"
                            );
                            continue;
                        }
                        Ok(body) if status < 500 => return Ok(Answer { status, body }),
                        Ok(body) => {
                            last = format!("{status} {}", String::from_utf8_lossy(&body));
                            restarting_status(status)
                        }
                        Err(Error::Unreachable(message)) => {
                            last = message;
                            false
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => {
                    last = error.to_string();
                    restarting_error(&error)
                }
            };
            attempt += 1;
            let again =
                attempt <= self.retries || (restarting && began.elapsed() < self.restart_window);
            if !again {
                break;
            }
        }
        Err(Error::Unreachable(format!(
            "the hub at {}{path}: {last}",
            self.base
        )))
    }
}

/// Whether a status says that nothing listens behind the hub's proxy, as `tailscale serve`
/// answers while the hub restarts.
fn restarting_status(status: u16) -> bool {
    matches!(status, 502..=504)
}

/// Whether a failed request says that nothing listens at the hub's address yet: the connection
/// was refused, or cut before an answer, as when the hub stops mid-request.
fn restarting_error(error: &ureq::Error) -> bool {
    use std::io::ErrorKind;
    match error {
        ureq::Error::ConnectionFailed => true,
        ureq::Error::Io(io) => matches!(
            io.kind(),
            ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::UnexpectedEof
        ),
        _ => false,
    }
}

impl HubClient {
    /// The answer if its status is a success, else the error its status means.
    fn ok(&self, path: &str, answer: Answer) -> Result<Answer> {
        let message = || format!("{path}: {}", String::from_utf8_lossy(&answer.body));
        match answer.status {
            200..=299 => Ok(answer),
            404 => Err(Error::NotFound(message())),
            409 => Err(Error::Conflict(message())),
            // A refused token is the machine's problem to fix (`ignite install` registers it again),
            // not a fault of what was sent, so it is reported like an unreachable hub: callers that
            // queue work keep it queued instead of dropping it as malformed.
            401 | 403 => Err(Error::Unreachable(format!(
                "the hub refused this machine's token ({}){}; run `ignite install` to register it \
             again: {}",
                answer.status,
                if self.token_file.is_some() && answer.status == 401 {
                    " even after it was read again"
                } else {
                    ""
                },
                message()
            ))),
            // Only 400 and 422 say that what was sent is malformed, which is the one answer after
            // which a caller may drop a queued entry, and 413 says that it is too large. Every
            // other refusal (a timeout, a rate limit, a proxy's 4xx) is about the hub's state, not
            // the entry, so the entry is kept and sent again later.
            400 | 422 => Err(Error::Invalid(format!("{} {}", answer.status, message()))),
            // A body too large is a verdict on what was sent, but not that it is malformed: the
            // same content may go in smaller pieces (see `too_large`).
            413 => Err(Error::Invalid(format!("{TOO_LARGE}{}", message()))),
            // The hub's read-only listener answers every write with 405. Pointing `[hub] url` at it
            // is a mistake that no retry fixes, so it gets a message of its own (see `read_only`).
            405 if String::from_utf8_lossy(&answer.body).contains(READ_ONLY_BODY) => {
                Err(Error::Unreachable(format!(
                    "{READ_ONLY}the hub answered {path} with 405 \"{READ_ONLY_BODY}\"; \
                 point [hub] url at the writable listener ({}), since the read-only one is for \
                 the public address",
                    self.base
                )))
            }
            status => Err(Error::Unreachable(format!(
                "the hub answered {status}, which is no verdict on what was sent: {}",
                message()
            ))),
        }
    }
}

fn decode<R: DeserializeOwned>(path: &str, body: &[u8]) -> Result<R> {
    serde_json::from_slice(body).map_err(|e| Error::Invalid(format!("{path}: bad reply: {e}")))
}

impl Hub for HubClient {
    fn lookup(&self, keys: &[ResultKey]) -> Result<Vec<Option<CachedResult>>> {
        let request = LookupRequest {
            keys: keys.to_vec(),
        };
        let reply: LookupReply = self.post("/results/lookup", &request)?;
        Ok(reply.results)
    }

    fn store(&self, results: &[NewResult]) -> Result<()> {
        let request = StoreRequest {
            results: results.to_vec(),
            areas: Vec::new(),
        };
        let _: serde_json::Value = self.post("/results/store", &request)?;
        Ok(())
    }

    /// The hub answers `POST /next` with a reservation, or `null` when it has nothing.
    fn next(&self, free: &MachineFree) -> Result<Option<Reservation>> {
        self.post("/next", free)
    }

    fn refuse_reservation(&self, reservation: &Reservation) -> Result<()> {
        let _: serde_json::Value = self.post("/reservations/refuse", reservation)?;
        Ok(())
    }

    fn next_explained(&self, free: &MachineFree) -> Result<ignite_core::protocol::hub::NextReply> {
        let free = MachineFree {
            room: None,
            explain: true,
            ..free.clone()
        };
        self.post("/next", &free)
    }

    fn claim(&self, claim: &Claim) -> Result<TicketBrief> {
        self.post("/claims", claim)
    }

    fn landed(&self, landed: &Landed) -> Result<LandedReply> {
        self.post("/landed", landed)
    }

    fn lease(&self, name: &str, op: LeaseOp) -> Result<LeaseReply> {
        let path = format!("/leases/{name}");
        let answer = self.post_raw(&path, &op)?;
        if answer.status == 409 {
            return Ok(LeaseReply::Busy(decode::<LeaseBusy>(&path, &answer.body)?));
        }
        Ok(LeaseReply::Held(decode::<Lease>(
            &path,
            &self.ok(&path, answer)?.body,
        )?))
    }

    fn feed(&self, since: Seq, wait_s: u32) -> Result<FeedBatch> {
        let url = format!("{}/feed", self.base);
        let timeout = REQUEST_TIMEOUT + Duration::from_secs(u64::from(wait_s));
        let answer = self.with_retries("/feed", || {
            self.authorized(self.agent.get(&url))
                .query("since", since.0.to_string())
                .query("wait", wait_s.to_string())
                .config()
                .timeout_global(Some(timeout))
                .build()
                .call()
        })?;
        decode("/feed", &self.ok("/feed", answer)?.body)
    }

    fn records(&self, batch: &[Record]) -> Result<()> {
        self.numbered_records(batch).map(drop)
    }

    fn numbered_records(&self, batch: &[Record]) -> Result<Vec<RecordId>> {
        // A reply without IDs still means the records were taken.
        #[derive(serde::Deserialize)]
        struct Numbered {
            #[serde(default)]
            ids: Vec<RecordId>,
        }
        let request = RecordBatch {
            records: batch.to_vec(),
        };
        let numbered: Numbered = self.post("/records", &request)?;
        Ok(numbered.ids)
    }

    fn metrics(&self, import: &MetricsImport) -> Result<()> {
        let _: serde_json::Value = self.post("/metrics/import", import)?;
        Ok(())
    }

    fn flaky(&self, report: &FlakyReport) -> Result<()> {
        let _: serde_json::Value = self.post("/flaky", report)?;
        Ok(())
    }

    fn transcripts(&self, append: &TranscriptAppend) -> Result<()> {
        let _: serde_json::Value = self.post("/transcripts/append", append)?;
        Ok(())
    }

    fn session_title(&self, title: &SessionTitle) -> Result<()> {
        let _: serde_json::Value = self.post("/session-titles", title)?;
        Ok(())
    }

    fn tip_claim(&self, claim: &TipClaim) -> Result<TipClaimReply> {
        self.post("/tips/claim", claim)
    }

    fn tip_coverage(&self) -> Result<TipCoverage> {
        match self.get("/tips/coverage", &[]) {
            Err(Error::NotFound(_)) => Ok(TipCoverage::default()),
            reply => reply,
        }
    }

    fn update_tip_coverage(&self, update: &TipCoverageUpdate) -> Result<bool> {
        self.post("/tips/coverage/update", update)
    }

    fn tip_verified(&self, verified: &TipVerified) -> Result<()> {
        let _: serde_json::Value = self.post("/tips/verified", verified)?;
        Ok(())
    }

    fn regression(&self, report: &RegressionReport) -> Result<RegressionFiled> {
        self.post("/regressions", report)
    }

    fn reverted(&self, reverted: &Reverted) -> Result<RevertFiled> {
        self.post("/reverted", reverted)
    }

    fn open_regressions(&self) -> Result<Vec<OpenRegression>> {
        let reply: OpenRegressions = self.get("/regressions/open", &[])?;
        Ok(reply.regressions)
    }

    fn audit(&self, report: &AuditReport) -> Result<ReportFiled> {
        self.post("/audits", report)
    }

    fn find(&self, find: &ExplorationFind) -> Result<ReportFiled> {
        self.post("/finds", find)
    }

    fn justiciar(&self, report: &JusticiarReport) -> Result<ReportFiled> {
        self.post("/justiciar", report)
    }

    fn incomplete(&self, report: &IncompleteReport) -> Result<ReportFiled> {
        self.post("/incomplete", report)
    }

    fn pause(&self, pause: &Pause) -> Result<()> {
        let _: serde_json::Value = self.post("/pause", pause)?;
        Ok(())
    }

    fn set_fleet_machine_state(
        &self,
        machine: &ignite_core::ids::MachineId,
        state: ignite_core::fleet::MachineState,
        actor: Option<&str>,
    ) -> Result<()> {
        use ignite_core::fleet::{FleetUpdate, FleetView};

        let mut client = self.clone();
        if let Some(actor) = actor {
            actor.clone_into(&mut client.actor);
        }
        let view: FleetView = client.get("/fleet", &[])?;
        let name = view
            .machines
            .iter()
            .find(|host| host.id == *machine)
            .ok_or_else(|| Error::NotFound(format!("machine {machine} is not registered")))?
            .name
            .clone();
        let mut settings = view.settings;
        for attempt in 0..2 {
            if settings.machine(&name).state == state {
                return Ok(());
            }
            let version = settings.version;
            settings.machines.entry(name.clone()).or_default().state = state;
            let bytes = serde_json::to_vec(&FleetUpdate { version, settings })
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let url = format!("{}/fleet", client.base);
            let reply = client.with_retries("/fleet", || {
                client
                    .authorized(client.agent.put(&url))
                    .header("Content-Type", "application/json")
                    .send(&bytes[..])
            })?;
            if attempt == 0 && reply.status == 409 {
                settings = decode("/fleet", &reply.body)?;
            } else {
                client.ok("/fleet", reply)?;
                return Ok(());
            }
        }
        unreachable!("the second request returns its result")
    }

    fn emergency(
        &self,
        change: &ignite_core::protocol::emergency::EmergencyChange,
    ) -> Result<Option<ignite_core::protocol::emergency::Emergency>> {
        let reply: ignite_core::protocol::emergency::EmergencyReply =
            self.post("/emergency", change)?;
        Ok(reply.emergency)
    }

    fn landed_commits(&self, commits: &[Commit]) -> Result<Vec<LandedCommit>> {
        let request = LandingsRequest {
            commits: commits.to_vec(),
        };
        let reply: LandingsReply = self.post("/landings/lookup", &request)?;
        Ok(reply.commits)
    }

    fn listings(&self, keys: &[ListingKey]) -> Result<Vec<Option<String>>> {
        let request = ListingLookup {
            keys: keys.to_vec(),
        };
        let reply: ListingReply = self.post("/listings/lookup", &request)?;
        Ok(reply.listings)
    }

    fn verdict(&self, key: &VerdictKey) -> Result<Option<Verdict>> {
        self.post("/verdicts/lookup", key)
    }

    fn store_verdict(&self, verdict: &Verdict) -> Result<()> {
        let _: serde_json::Value = self.post("/verdicts/store", verdict)?;
        Ok(())
    }

    fn store_listing(&self, listing: &StoredListing) -> Result<()> {
        let _: serde_json::Value = self.post("/listings/store", listing)?;
        Ok(())
    }

    fn tip_waiting(&self, waiting: &TipWaiting) -> Result<TipWaitingFiled> {
        self.post("/tips/waiting", waiting)
    }

    fn tip_costs(&self, request: &TipCostsRequest) -> Result<TipCosts> {
        self.post("/tips/costs", request)
    }

    fn tip_round(&self, round: &TipRound) -> Result<TipRoundFiled> {
        self.post("/tips/rounds", round)
    }

    fn last_tip_round(&self) -> Result<Option<TipRound>> {
        let reply: LastTipRound = self.get("/tips/rounds/last", &[])?;
        Ok(reply.round)
    }

    fn rates(&self) -> Result<ignite_core::budget::ClassRates> {
        self.get("/rates", &[])
    }

    fn fleet_versions(&self) -> Result<ignite_core::protocol::fleet::FleetVersions> {
        self.get("/machines/versions", &[])
    }

    /// `GET /tickets/<id>` with its comments and history.
    fn ticket(&self, id: &TicketId) -> Result<TicketDetail> {
        let query = [("comments", "1".to_owned()), ("history", "1".to_owned())];
        self.get(&format!("/tickets/{id}"), &query)
    }

    fn tickets(&self, query: &TicketQuery) -> Result<Vec<Ticket>> {
        self.get("/tickets", &ticket_query(query))
    }

    fn batch(&self, request: &BatchRequest) -> Result<BatchReply> {
        self.post("/batch", request)
    }

    /// `GET /export`, whose body is JSON Lines: one whole ticket per line.
    fn export(&self, after: Option<&TicketId>, ids: &[TicketId]) -> Result<Vec<WholeTicket>> {
        let mut query: Vec<(&str, String)> = ids.iter().map(|id| ("id", id.to_string())).collect();
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        let url = format!("{}/export", self.base);
        let answer = self.with_retries("/export", || {
            let mut request = self.authorized(self.agent.get(&url));
            for (key, value) in &query {
                request = request.query(*key, value);
            }
            request.call()
        })?;
        let body = self.ok("/export", answer)?.body;
        body.split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| decode("/export", line))
            .collect()
    }

    fn import(&self, request: &ImportRequest) -> Result<ImportReply> {
        self.post("/import", request)
    }
}

/// The query string of `GET /tickets`, in the names the hub reads.
fn ticket_query(query: &TicketQuery) -> Vec<(&'static str, String)> {
    let spelled = |value: serde_json::Value| value.as_str().map(str::to_owned);
    let mut pairs = Vec::new();
    if let Some(status) = query.status.and_then(|s| spelled(serde_json::json!(s))) {
        pairs.push(("status", status));
    }
    if let Some(kind) = query
        .ticket_type
        .and_then(|t| spelled(serde_json::json!(t)))
    {
        pairs.push(("type", kind));
    }
    let named = [
        ("parent", query.parent.as_ref().map(ToString::to_string)),
        ("ancestor", query.ancestor.as_ref().map(ToString::to_string)),
        ("search", query.search.clone()),
        ("assignee", query.assignee.clone()),
        ("limit", query.limit.map(|l| l.to_string())),
    ];
    pairs.extend(named.into_iter().filter_map(|(k, v)| Some((k, v?))));
    pairs.extend(query.label.iter().map(|l| ("label", l.clone())));
    pairs.extend(query.label_any.iter().map(|l| ("label_any", l.clone())));
    for (flag, set) in [("ready", query.ready), ("all", query.all)] {
        if set {
            pairs.push((flag, "1".to_owned()));
        }
    }
    pairs
}

#[cfg(test)]
#[path = "hubclient_tests.rs"]
pub(crate) mod tests;
