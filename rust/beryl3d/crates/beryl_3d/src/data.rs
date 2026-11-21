//! The app's embedded tuning and the optional watcher for checkout data.

use std::sync::{Arc, OnceLock};

use kit_base::data::{Data, profiles};

const EFFECTS: &[(&str, &str)] = &include!(concat!(env!("OUT_DIR"), "/effects.rs"));

pub(crate) fn embedded() -> Result<Arc<Data>, String> {
    static DATA: OnceLock<Result<Arc<Data>, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        Data::parse(embedded_tuning, &profiles::SHIPPED, EFFECTS.iter().copied()).map(Arc::new)
    })
    .clone()
}

fn embedded_tuning(name: &str) -> Result<String, String> {
    let text = match name {
        "card.json" => include_str!("../../../../../data/beryl3d/card.json"),
        "badges.json" => include_str!("../../../../../data/beryl3d/badges.json"),
        "verbs.json" => include_str!("../../../../../data/beryl3d/verbs.json"),
        "audit.json" => include_str!("../../../../../data/beryl3d/audit.json"),
        "catch-up.json" => include_str!("../../../../../data/beryl3d/catch-up.json"),
        "frame-rate.json" => include_str!("../../../../../data/beryl3d/frame-rate.json"),
        "slide.json" => include_str!("../../../../../data/beryl3d/slide.json"),
        "tier-probe.json" => include_str!("../../../../../data/beryl3d/tier-probe.json"),
        "notices.json" => include_str!("../../../../../data/beryl3d/notices.json"),
        "revealed.json" => include_str!("../../../../../data/beryl3d/revealed.json"),
        "shown.json" => include_str!("../../../../../data/beryl3d/shown.json"),
        "gallery.json" => include_str!("../../../../../data/beryl3d/gallery.json"),
        "lighting.json" => include_str!("../../../../../data/beryl3d/lighting.json"),
        "light.json" => include_str!("../../../../../data/beryl3d/light.json"),
        "own.json" => include_str!("../../../../../data/beryl3d/own.json"),
        "pointer.json" => include_str!("../../../../../data/beryl3d/pointer.json"),
        _ => return Err(format!("{name}: missing embedded tuning")),
    };
    Ok(text.to_owned())
}

/// A watch began before the initial load, so an edit during startup is noticed too.
#[cfg(feature = "tools")]
pub(crate) struct Reload {
    watch: beryl_host::DataWatch,
    initial_error: Option<String>,
}

#[cfg(feature = "tools")]
pub(crate) fn initial(embedded: Arc<Data>, watch: bool) -> (Arc<Data>, Option<Reload>) {
    if !watch {
        return (embedded, None);
    }
    let directory = beryl_inspect::data::directory();
    let watch = beryl_host::DataWatch::new(directory.clone());
    let (data, initial_error) = match beryl_inspect::data::load(&directory) {
        Ok(data) => (Arc::new(data), None),
        Err(error) => (embedded, Some(error)),
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
        .start(inbox, beryl_inspect::data::load)
        .map(Some)
        .map_err(|error| format!("could not start the tuning watcher: {error}"))
}
