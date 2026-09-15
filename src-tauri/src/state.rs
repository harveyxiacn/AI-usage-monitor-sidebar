//! Shared application state. [BACKEND owns this file]
//!
//! The platform layer only needs `AppState` to exist and be managed via
//! `app.manage(AppState::new(...))` in `lib.rs`; everything else is internal
//! to the backend.

use crate::model::{AppSnapshot, Settings};
use parking_lot::RwLock;
use std::path::PathBuf;

pub struct AppState {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub settings: RwLock<Settings>,
    pub snapshot: RwLock<AppSnapshot>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
            settings: RwLock::new(Settings::default()),
            snapshot: RwLock::new(AppSnapshot::default()),
        }
    }
}
