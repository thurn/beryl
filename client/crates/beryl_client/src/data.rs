//! The app's embedded tuning and the optional watcher for checkout data.

use std::sync::{Arc, OnceLock};

use kit_base::data::{Data, profiles};

const PROFILES: [(&str, &str); 2] = [
    (
        profiles::FILES[0],
        include_str!("../../../../data/client/profiles/phone.json"),
    ),
    (
        profiles::FILES[1],
        include_str!("../../../../data/client/profiles/desktop.json"),
    ),
];
const EFFECTS: &[(&str, &str)] = &include!(concat!(env!("OUT_DIR"), "/effects.rs"));

pub(crate) fn embedded() -> Result<Arc<Data>, String> {
    static DATA: OnceLock<Result<Arc<Data>, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        Data::parse(embedded_tuning, &PROFILES, EFFECTS.iter().copied()).map(Arc::new)
    })
    .clone()
}

fn embedded_tuning(name: &str) -> Result<String, String> {
    let text = match name {
        "card.json" => include_str!("../../../../data/client/card.json"),
        "badges.json" => include_str!("../../../../data/client/badges.json"),
        "verbs.json" => include_str!("../../../../data/client/verbs.json"),
        "audit.json" => include_str!("../../../../data/client/audit.json"),
        "flat.json" => include_str!("../../../../data/client/flat.json"),
        "hud.json" => include_str!("../../../../data/client/hud.json"),
        "catch-up.json" => include_str!("../../../../data/client/catch-up.json"),
        "frame-rate.json" => include_str!("../../../../data/client/frame-rate.json"),
        "slide.json" => include_str!("../../../../data/client/slide.json"),
        "tier-probe.json" => include_str!("../../../../data/client/tier-probe.json"),
        "notices.json" => include_str!("../../../../data/client/notices.json"),
        "revealed.json" => include_str!("../../../../data/client/revealed.json"),
        "shown.json" => include_str!("../../../../data/client/shown.json"),
        "lighting.json" => include_str!("../../../../data/client/lighting.json"),
        "light.json" => include_str!("../../../../data/client/light.json"),
        "own.json" => include_str!("../../../../data/client/own.json"),
        "pointer.json" => include_str!("../../../../data/client/pointer.json"),
        _ => return Err(format!("{name}: missing embedded tuning")),
    };
    Ok(text.to_owned())
}

/// The watcher remembers the snapshot used during startup.
#[cfg(feature = "tools")]
pub(crate) struct Reload {
    watch: beryl_host::DataWatch<beryl_inspect::data::Snapshot>,
    initial_error: Option<String>,
}

#[cfg(feature = "tools")]
pub(crate) fn initial(embedded: Arc<Data>, watch: bool) -> (Arc<Data>, Option<Reload>) {
    if !watch {
        return (embedded, None);
    }
    let directory = beryl_inspect::data::directory();
    let mut watch = beryl_host::DataWatch::new(
        move || beryl_inspect::data::Snapshot::read(&directory),
        beryl_inspect::data::Snapshot::parse,
    );
    let (data, initial_error) = match watch.poll() {
        Some(Ok(data)) => (data, None),
        Some(Err(error)) => (embedded, Some(error)),
        None => (embedded, None),
    };
    (
        data,
        Some(Reload {
            watch,
            initial_error,
        }),
    )
}

#[cfg(feature = "tools")]
pub(crate) fn start_watch(
    reload: Option<Reload>,
    inbox: beryl_host::Inbox,
) -> Result<Option<beryl_host::WatchingData>, String> {
    let Some(reload) = reload else {
        return Ok(None);
    };
    if let Some(error) = reload.initial_error {
        inbox.send(app_client::Input::DataError(error));
    }
    reload
        .watch
        .start(inbox)
        .map(Some)
        .map_err(|error| format!("could not start the tuning watcher: {error}"))
}
