//! The Beryl 3D client, its commands and its replay driver.

mod client;
pub mod command;
pub mod driver;
pub mod settings;
pub mod stories;

pub use app_screens::performance::FrameRates;
pub use client::{Client, Config, FeatureInfo, Input, composed_features, registered_features};
