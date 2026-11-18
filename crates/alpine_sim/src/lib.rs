//! Alpine Front simulation core.
//!
//! Plain Rust, headless and deterministic: a fixed update order, seeded randomness and an order log,
//! so any run can be repeated exactly. Nothing here depends on Bevy or on drawing; the game crate
//! only reads the state to draw it.

pub mod flow;
pub mod hash;
pub mod los;
pub mod replay;
pub mod report;
pub mod roster;
pub mod rng;
pub mod scenario;
pub mod sim;
pub mod sky;
pub mod spatial;
pub mod supply;
pub mod terrain;
pub mod tuning;
pub mod weather;

use std::path::PathBuf;

/// The repository's `data` folder (override with the `ALPINE_DATA` environment variable).
pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("ALPINE_DATA") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("data")
}
