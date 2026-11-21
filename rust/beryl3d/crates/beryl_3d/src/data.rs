//! The embedded tuning shared by the app's client and its startup probe.

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
        "pointer.json" => include_str!("../../../../../data/beryl3d/pointer.json"),
        _ => return Err(format!("{name}: missing embedded tuning")),
    };
    Ok(text.to_owned())
}
