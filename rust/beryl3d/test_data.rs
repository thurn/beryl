//! The repository's tuning, shared by the clients and tables in an integration test binary.

use std::sync::{Arc, OnceLock};

use kit_base::data::Data;

pub fn data() -> Arc<Data> {
    static DATA: OnceLock<Arc<Data>> = OnceLock::new();
    Arc::clone(DATA.get_or_init(|| {
        let space =
            beryl_test_files::read_to_string("data/beryl3d/space.json").expect("read space.json");
        let gates =
            beryl_test_files::read_to_string("data/beryl3d/gates.json").expect("read gates.json");
        Arc::new(Data::parse(&space, &gates).expect("repository tuning"))
    }))
}
