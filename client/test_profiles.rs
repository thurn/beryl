//! The shipped profile inputs for tests that construct or edit profiles.

use kit_base::data::profiles::FILES;

pub const SHIPPED: [(&str, &str); 2] = [
    (FILES[0], include_str!("../data/client/profiles/phone.json")),
    (
        FILES[1],
        include_str!("../data/client/profiles/desktop.json"),
    ),
];
