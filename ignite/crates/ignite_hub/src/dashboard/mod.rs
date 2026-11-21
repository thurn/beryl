//! The dashboard, the one page meant for a person: the ticket views, each machine's cores and
//! trees, the pool, the landing queue, tip verification, open regressions and exploration finds.
//! Pages are server-rendered HTML; each page module renders itself and its fragments, and
//! [`serve::serve`] routes a request to them.

pub mod assets;
pub mod attachments;
pub mod attention;
pub mod available;
pub mod common;
pub mod components;
pub mod data;
pub mod edit;
pub mod explore;
pub mod history;
pub mod home;
pub mod landings;
pub mod machines;
pub mod markdown;
pub mod milestones;
mod pins;
pub mod pool;
pub mod references;
pub mod serve;
pub mod session;
pub mod spend;
pub mod spend_view;
pub mod status;
pub mod ticket;
pub mod tips;
