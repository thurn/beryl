//! The repository's tuning, shared by the clients and tables in an integration test binary.

use std::sync::{Arc, OnceLock};

use kit_base::data::{Data, profiles};

pub fn data() -> Arc<Data> {
    static DATA: OnceLock<Arc<Data>> = OnceLock::new();
    Arc::clone(DATA.get_or_init(|| {
        let files = profiles::FILES.map(|name| {
            beryl_test_files::read_to_string(format!("data/beryl3d/{name}"))
                .expect("read layout profile")
        });
        let contents = std::array::from_fn::<_, { profiles::FILES.len() }, _>(|index| {
            (profiles::FILES[index], files[index].as_str())
        });
        let directory = std::path::Path::new("data/beryl3d/effects");
        let effects: Vec<_> = beryl_test_files::read_dir(directory)
            .expect("read effect definitions")
            .map(|entry| entry.expect("effect file").file_name())
            .filter_map(|file| file.into_string().ok())
            .filter_map(|file| {
                let name = file.strip_suffix(".json")?.to_owned();
                let text = beryl_test_files::read_to_string(directory.join(file))
                    .expect("read effect file");
                Some((name, text))
            })
            .collect();
        let effects = effects
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()));
        Arc::new(
            Data::parse(
                |name| {
                    beryl_test_files::read_to_string(format!("data/beryl3d/{name}"))
                        .map_err(|error| format!("{name}: {error}"))
                },
                &contents,
                effects,
            )
            .expect("repository tuning"),
        )
    }))
}
