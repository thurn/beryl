//! The embedded tuning shared by the app's client and its startup probe.

use std::sync::{Arc, OnceLock};

use kit_base::data::{Data, profiles};

const EFFECTS: &[(&str, &str)] = &include!(concat!(env!("OUT_DIR"), "/effects.rs"));

pub(crate) fn embedded() -> Result<Arc<Data>, String> {
    static DATA: OnceLock<Result<Arc<Data>, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        Data::parse(
            include_str!("../../../../../data/beryl3d/space.json"),
            include_str!("../../../../../data/beryl3d/gates.json"),
            &profiles::SHIPPED,
            EFFECTS.iter().copied(),
        )
        .map(Arc::new)
    })
    .clone()
}
